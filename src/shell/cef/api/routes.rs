use std::marker::PhantomData;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{ApiRequest, ApiResponse, NO_STORE, Wire};
use crate::app::services::Services;

pub(in crate::shell::cef) struct Routes {
    pub(super) serve: Serve,
    pub(super) table: &'static [Route],
    #[cfg(test)]
    pub(super) module: &'static str,
}

pub(in crate::shell::cef) enum Serve {
    WithServices(fn(&Arc<Services>, &[&str], &ApiRequest) -> Option<ApiResponse>),
    WithoutServices(fn(&[&str], &ApiRequest) -> Option<ApiResponse>),
}

pub(in crate::shell::cef) struct Route {
    pub(super) method: &'static str,
    pub(super) path: &'static [Segment],
    #[cfg(test)]
    pub(super) body: BodySpec,
    #[cfg(test)]
    pub(super) answer: AnswerSpec,
}

pub(in crate::shell::cef) enum Segment {
    Literal(&'static str),
    Param {
        #[cfg(test)]
        name: &'static str,
    },
}

impl Route {
    pub(super) fn matches(&self, segments: &[&str]) -> bool {
        self.path.len() == segments.len()
            && self
                .path
                .iter()
                .zip(segments)
                .all(|(part, segment)| match part {
                    Segment::Literal(literal) => literal == segment,
                    Segment::Param { .. } => true,
                })
    }
}

#[derive(Debug, Clone)]
pub struct RawBody(Vec<u8>);

impl RawBody {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

pub(super) struct Body<'a, T> {
    request: &'a ApiRequest,
    body: PhantomData<fn() -> T>,
}

impl<'a, T> Body<'a, T> {
    pub(super) fn new(request: &'a ApiRequest) -> Self {
        Self {
            request,
            body: PhantomData,
        }
    }
}

impl<T: DeserializeOwned + Wire> Body<'_, T> {
    pub(super) fn decode(self) -> Result<T, ApiResponse> {
        let raw = &self.request.body.0;
        let body: &[u8] = if raw.iter().all(u8::is_ascii_whitespace) {
            b"{}"
        } else {
            raw
        };
        serde_json::from_slice(body)
            .map_err(|error| ApiResponse::error(400, format!("invalid request body: {error}")))
    }
}

pub(super) struct Bytes;

impl<'a> Body<'a, Bytes> {
    pub(super) fn bytes(self) -> &'a [u8] {
        &self.request.body.0
    }
}

pub(super) struct Binary;

pub(super) struct BinaryAnswer(ApiResponse);

impl BinaryAnswer {
    pub(super) fn bytes(content_type: String, body: Vec<u8>, cache_control: &'static str) -> Self {
        Self(ApiResponse {
            status: 200,
            content_type,
            body,
            cache_control,
            headers: Vec::new(),
        })
    }

    pub(super) fn ranged_bytes(
        status: u16,
        content_type: String,
        body: Vec<u8>,
        content_range: Option<String>,
        accept_ranges: Option<String>,
    ) -> Self {
        let mut headers = vec![(
            "Accept-Ranges".to_string(),
            accept_ranges.unwrap_or_else(|| "bytes".to_string()),
        )];
        if let Some(content_range) = content_range {
            headers.push(("Content-Range".to_string(), content_range));
        }
        Self(ApiResponse {
            status,
            content_type,
            body,
            cache_control: NO_STORE,
            headers,
        })
    }
}

pub(super) struct Relay;

pub(super) trait Answer {
    type Value;
    fn success(value: Self::Value) -> ApiResponse;
    fn respond(returned: Result<Self::Value, ApiResponse>) -> ApiResponse {
        returned.map_or_else(|error| error, Self::success)
    }
    #[cfg(test)]
    const SPEC: AnswerSpec;
}

impl<T: Serialize + Wire> Answer for T {
    type Value = T;
    fn success(value: T) -> ApiResponse {
        ApiResponse::ok(value)
    }
    #[cfg(test)]
    const SPEC: AnswerSpec = AnswerSpec::Json(T::ident);
}

impl Answer for Binary {
    type Value = BinaryAnswer;
    fn success(answer: BinaryAnswer) -> ApiResponse {
        answer.0
    }
    #[cfg(test)]
    const SPEC: AnswerSpec = AnswerSpec::Binary;
}

impl Answer for Relay {
    type Value = Value;
    fn success(value: Value) -> ApiResponse {
        ApiResponse::untyped(value)
    }
    #[cfg(test)]
    const SPEC: AnswerSpec = AnswerSpec::Relay;
}

#[cfg(test)]
pub(super) trait RequestBody {
    const SPEC: BodySpec;
}

#[cfg(test)]
impl<T: DeserializeOwned + Wire> RequestBody for T {
    const SPEC: BodySpec = BodySpec::Json(T::ident);
}

#[cfg(test)]
impl RequestBody for Bytes {
    const SPEC: BodySpec = BodySpec::Bytes;
}

#[cfg(test)]
pub(super) type WireRef = fn(&ts_rs::Config) -> String;

#[cfg(test)]
pub(super) enum BodySpec {
    None,
    Json(WireRef),
    Bytes,
}

#[cfg(test)]
pub(super) enum AnswerSpec {
    Json(WireRef),
    Binary,
    Relay,
}

macro_rules! routes {
    (
        @build ($($services:ident)?) $request:ident, $kind:ident;
        $(
            $method:ident [$($segment:tt),*] $(($body:ident: $body_type:ty))?
                -> $answer:ty = $call:expr
        ),* $(,)?
    ) => {
        fn serve(
            $($services: &std::sync::Arc<$crate::app::services::Services>,)?
            segments: &[&str],
            $request: &$crate::shell::cef::api::ApiRequest,
        ) -> Option<$crate::shell::cef::api::ApiResponse> {
            use $crate::shell::cef::api::routes;
            match segments {
                $(
                    [$($segment),*] if $request.is(routes::method!($method)) => {
                        $(let $body = routes::Body::<$body_type>::new($request);)?
                        Some(<$answer as routes::Answer>::respond($call))
                    }
                )*
                _ => None,
            }
        }

        pub(super) const ROUTES: $crate::shell::cef::api::routes::Routes = {
            use $crate::shell::cef::api::routes;
            routes::Routes {
                serve: routes::Serve::$kind(serve),
                table: &[$(
                    routes::Route {
                        method: routes::method!($method),
                        path: &[$(routes::segment!($segment)),*],
                        #[cfg(test)]
                        body: routes::body_spec!($($body_type)?),
                        #[cfg(test)]
                        answer: <$answer as routes::Answer>::SPEC,
                    }
                ),*],
                #[cfg(test)]
                module: module_path!(),
            }
        };
    };
    (|$services:ident, $request:ident| $($routes:tt)*) => {
        $crate::shell::cef::api::routes::routes!(@build ($services) $request, WithServices; $($routes)*);
    };
    (|$request:ident| $($routes:tt)*) => {
        $crate::shell::cef::api::routes::routes!(@build () $request, WithoutServices; $($routes)*);
    };
}

macro_rules! method {
    (GET) => {
        "GET"
    };
    (POST) => {
        "POST"
    };
    (PUT) => {
        "PUT"
    };
    (PATCH) => {
        "PATCH"
    };
    (DELETE) => {
        "DELETE"
    };
    ($other:ident) => {
        compile_error!(concat!(
            "`",
            stringify!($other),
            "` is not an API method: declare GET, POST, PUT, PATCH or DELETE"
        ))
    };
}

macro_rules! segment {
    ($literal:literal) => {
        $crate::shell::cef::api::routes::Segment::Literal($literal)
    };
    ($param:ident) => {
        $crate::shell::cef::api::routes::Segment::Param {
            #[cfg(test)]
            name: stringify!($param),
        }
    };
}

#[cfg(test)]
macro_rules! body_spec {
    () => {
        $crate::shell::cef::api::routes::BodySpec::None
    };
    ($body_type:ty) => {
        <$body_type as $crate::shell::cef::api::routes::RequestBody>::SPEC
    };
}

#[cfg(test)]
pub(super) use body_spec;
pub(super) use method;
pub(super) use routes;
pub(super) use segment;

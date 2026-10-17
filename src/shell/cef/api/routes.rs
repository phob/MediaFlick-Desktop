//! Each API module declares its routes once with [`routes!`]: the method, the
//! path, the request body and the answer, next to the call that serves it.
//! The macro turns that list into the module's dispatcher and its row of the
//! route table, the 405 answers come from the same list, and
//! `generated_typescript_is_current` renders it into the UI's route map.
//!
//! The compiler holds every handler to its declaration: it returns
//! `Result<_, ApiResponse>` whose success side is the declared wire type, a
//! [`Relay`]'s JSON value or a [`Binary`] route's [`BinaryAnswer`], and only
//! this module turns that success into a response. A handler reads the
//! request body only through the [`Body`] its route declares, because only
//! this module can see a [`RawBody`]'s bytes. What visibility cannot keep
//! inside this module, `only_the_router_answers_with_success` does: it fails
//! when code elsewhere builds a success `ApiResponse`, sets a status, names
//! `Body::new` or `Answer`, or answers an error with a 2xx status.

use std::marker::PhantomData;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{ApiRequest, ApiResponse, NO_STORE, Wire};
use crate::app::services::Services;

/// One module's routes: the dispatcher [`routes!`] generates and the
/// declarations it was generated from.
pub(in crate::shell::cef) struct Routes {
    pub(super) serve: fn(&Arc<Services>, &[&str], &ApiRequest) -> Option<ApiResponse>,
    pub(super) table: &'static [Route],
    #[cfg(test)]
    pub(super) module: &'static str,
}

/// One declaration: the method and path it takes, and, for the generated
/// TypeScript, what it reads and answers.
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
    /// Any one segment, which the call receives under this name.
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

/// The bytes of a request body. Only this module reads them, through
/// [`Body`], so a handler sees the body its route declares and nothing else.
#[derive(Debug, Clone)]
pub struct RawBody(Vec<u8>);

impl RawBody {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

/// The request body a route declares. [`routes!`] makes one for a route
/// that declares a body, and the guard keeps everyone else from calling
/// `Body::new`, so a handler cannot decode a type other than the one its
/// route names.
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
    /// Decodes the JSON body. A request sent without a body decodes like
    /// `{}`, so it is accepted exactly when every field of `T` is optional.
    /// A body that is not JSON, or does not match `T`, is rejected rather
    /// than read as defaults: a garbled "mark as unwatched" must not mark the
    /// item watched. Handlers decode where they used to, after the checks
    /// that answer first.
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

/// A body the UI sends as raw bytes, such as an uploaded image.
pub(super) struct Bytes;

impl<'a> Body<'a, Bytes> {
    pub(super) fn bytes(self) -> &'a [u8] {
        &self.request.body.0
    }
}

/// An answer the UI does not read as JSON: an image, artwork, or a trailer
/// stream. Its handler answers a [`BinaryAnswer`].
pub(super) struct Binary;

/// What a [`Binary`] route answers on success. Only the byte builders below
/// make one, so a binary route cannot answer JSON.
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

    /// Part of a stream, with the status and range headers its source sent.
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

/// Companion JSON the app passes through without a Rust type. Its handler
/// answers the JSON value, and the UI describes it by hand.
pub(super) struct Relay;

/// What a route answers with on success. Its handler returns
/// `Result<Self::Value, ApiResponse>`, and [`Answer::respond`] makes the
/// response: the success through [`Answer::success`], an error as it is.
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

/// A wire type's name in the generated TypeScript.
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

/// Declares a module's routes as `METHOD [path segments] (body: Type) ->
/// Answer = call`. `METHOD` is `GET`, `POST`, `PUT`, `PATCH` or `DELETE`;
/// anything else does not compile. A quoted segment is literal and a name
/// binds that segment for the call; the body is optional. The call sees the
/// names given in `|services, request|` and must return
/// `Result<Answer::Value, ApiResponse>`: the wire type itself, a JSON
/// [`Value`] for [`Relay`], a [`BinaryAnswer`] for [`Binary`]. Routes are
/// tried in the order written.
macro_rules! routes {
    (
        |$services:ident, $request:ident|
        $(
            $method:ident [$($segment:tt),*] $(($body:ident: $body_type:ty))?
                -> $answer:ty = $call:expr
        ),* $(,)?
    ) => {
        fn serve(
            $services: &std::sync::Arc<$crate::app::services::Services>,
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
                serve,
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

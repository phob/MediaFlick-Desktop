//! Each API module declares its routes once with [`routes!`]: the method, the
//! path, the request body and the answer, next to the call that serves it.
//! The macro turns that list into the module's dispatcher and its row of the
//! route table, so the compiler holds every handler to the types its route
//! declares, the 405 answers come from the same list, and
//! `generated_typescript_is_current` renders it into the UI's route map.

use std::marker::PhantomData;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{ApiRequest, ApiResponse, Handled, Wire};
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

/// The request body a route declares. A handler can read it only through
/// this, so it cannot decode a type other than the one its route names.
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
        let body: &[u8] = if self.request.body.iter().all(u8::is_ascii_whitespace) {
            b"{}"
        } else {
            &self.request.body
        };
        serde_json::from_slice(body)
            .map_err(|error| ApiResponse::error(400, format!("invalid request body: {error}")))
    }
}

/// A body the UI sends as raw bytes, such as an uploaded image.
pub(super) struct Bytes;

impl<'a> Body<'a, Bytes> {
    pub(super) fn bytes(self) -> &'a [u8] {
        &self.request.body
    }
}

/// An answer the UI does not read as JSON: an image, artwork, or a trailer
/// stream, built by the handler with its own content type and headers.
pub(super) struct Binary;

/// Companion JSON the app passes through without a Rust type, answered with
/// `ApiResponse::untyped`. The UI describes it by hand.
pub(super) struct Relay;

/// What a route answers with on success, and so what its handler returns.
pub(super) trait Answer {
    type Returned;
    fn respond(returned: Self::Returned) -> ApiResponse;
    #[cfg(test)]
    const SPEC: AnswerSpec;
}

impl<T: Serialize + Wire> Answer for T {
    type Returned = Result<T, ApiResponse>;
    fn respond(returned: Self::Returned) -> ApiResponse {
        returned.map_or_else(|error| error, ApiResponse::ok)
    }
    #[cfg(test)]
    const SPEC: AnswerSpec = AnswerSpec::Json(T::ident);
}

impl Answer for Binary {
    type Returned = Handled;
    fn respond(returned: Handled) -> ApiResponse {
        returned.unwrap_or_else(|error| error)
    }
    #[cfg(test)]
    const SPEC: AnswerSpec = AnswerSpec::Binary;
}

impl Answer for Relay {
    type Returned = Handled;
    fn respond(returned: Handled) -> ApiResponse {
        returned.unwrap_or_else(|error| error)
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
/// Answer = call`. A quoted segment is literal and a name binds that
/// segment for the call; the body is optional. The call sees the names given
/// in `|services, request|` and must return what `Answer` asks for:
/// `Result<Answer, ApiResponse>` for a wire type, `Handled` for [`Binary`]
/// and [`Relay`]. Routes are tried in the order written.
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
                    [$($segment),*] if $request.is(stringify!($method)) => {
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
                        method: stringify!($method),
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
pub(super) use routes;
pub(super) use segment;

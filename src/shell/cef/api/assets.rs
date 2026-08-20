use super::*;

// -------------------------------------------------------------------- assets

// The bundle is built by `build.rs` (Vite, in `ui/`) and staged into `OUT_DIR`.
// It is emitted with fixed names — no content hashing and no code splitting,
// because the assets never cross a network: they are embedded here and served
// from memory.
pub(super) fn static_asset(path: &str) -> Option<ApiResponse> {
    match path {
        "" | "/" | "/index.html" => Some(index_html()),
        "/app.js" => Some(ApiResponse::asset(
            "text/javascript; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/app.js")),
            NO_STORE,
        )),
        "/app.css" => Some(ApiResponse::asset(
            "text/css; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/app.css")),
            NO_STORE,
        )),
        _ => None,
    }
}

pub(super) fn index_html() -> ApiResponse {
    ApiResponse::asset(
        "text/html; charset=utf-8",
        include_bytes!(concat!(env!("OUT_DIR"), "/index.html")),
        NO_STORE,
    )
}

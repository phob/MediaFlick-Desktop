use super::*;

// The bundle is built by `build.rs` (Vite, in `ui/`) and staged into `OUT_DIR`.
// It is emitted with fixed names — no content hashing and no code splitting,
// because the assets never cross a network: they are embedded here and served
// from memory.
pub(super) fn static_asset(path: &str) -> Option<ApiResponse> {
    match path {
        "" | "/" | "/index.html" => Some(index_html()),
        "/app.js" => Some(asset(
            "text/javascript; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/app.js")),
            NO_STORE,
        )),
        "/app.css" => Some(asset(
            "text/css; charset=utf-8",
            include_bytes!(concat!(env!("OUT_DIR"), "/app.css")),
            NO_STORE,
        )),
        _ if path.starts_with("/reader/") => reader_asset(path),
        _ => None,
    }
}

const READER_ASSETS: &[(&str, &str, &[u8])] =
    include!(concat!(env!("OUT_DIR"), "/reader_assets.rs"));

/// libarchive's worker compiles its WebAssembly decoder, so its script alone
/// is served with a policy that allows that. A dedicated worker takes the
/// policy of its own script response, so the page keeps refusing WebAssembly.
const LIBARCHIVE_WORKER: &str = "/reader/libarchive/worker-bundle.js";
const LIBARCHIVE_WORKER_POLICY: &str =
    "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'";

fn reader_asset(path: &str) -> Option<ApiResponse> {
    let Ok(index) = READER_ASSETS.binary_search_by(|(asset, _, _)| asset.cmp(&path)) else {
        return Some(ApiResponse::error(
            404,
            format!("no reader asset at {path}"),
        ));
    };
    let (_, content_type, body) = READER_ASSETS[index];
    let mut response = asset(content_type, body, NO_STORE);
    if path == LIBARCHIVE_WORKER {
        response.headers.push((
            "Content-Security-Policy".to_string(),
            LIBARCHIVE_WORKER_POLICY.to_string(),
        ));
    }
    Some(response)
}

pub(super) fn index_html() -> ApiResponse {
    asset(
        "text/html; charset=utf-8",
        include_bytes!(concat!(env!("OUT_DIR"), "/index.html")),
        NO_STORE,
    )
}

fn asset(content_type: &str, body: &'static [u8], cache_control: &'static str) -> ApiResponse {
    ApiResponse {
        status: 200,
        content_type: content_type.to_string(),
        body: body.to_vec(),
        cache_control,
        headers: Vec::new(),
    }
}

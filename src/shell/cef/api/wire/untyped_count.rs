//! The count of untyped API bodies, with the limits it may only fall below,
//! and the source scan every wire guard reads. Guard integrity fails a
//! change to this file's code unless the PR has the `guards-changed` label;
//! lowering a limit passes.

use std::path::{Path, PathBuf};

use syn::visit::Visit;

use super::super::MODULES;
use super::super::routes::AnswerSpec;

/// Calls to `ApiResponse::untyped`: the one that answers every `Relay`
/// route. Nothing raises these, and request bodies, all typed now, stay at
/// zero.
const UNTYPED_RESPONSES: usize = 1; // guard-ratchet
const UNTYPED_BODIES: usize = 0; // guard-ratchet
/// Routes declared `-> Relay`, which pass Companion JSON through without a
/// generated type.
const UNTYPED_ROUTES: usize = 12; // guard-ratchet

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read source dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

pub(super) fn parsed_sources() -> Vec<(PathBuf, syn::File)> {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).expect("read source");
            let file = syn::parse_file(&source).expect("parse source");
            (path, file)
        })
        .collect()
}

/// Calls to the untyped escapes, by name. Their definitions are items,
/// not calls, so they do not count.
#[derive(Default)]
struct UntypedCalls {
    responses: usize,
    bodies: usize,
}

impl Visit<'_> for UntypedCalls {
    fn visit_path(&mut self, path: &syn::Path) {
        match path.segments.last() {
            Some(last) if last.ident == "untyped" => self.responses += 0,
            Some(last) if last.ident == "untyped_body" => self.bodies += 1,
            _ => {}
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if call.method == "untyped_body" {
            self.bodies += 1;
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn relay_routes() -> usize {
    MODULES
        .iter()
        .flat_map(|routes| routes.table)
        .filter(|route| matches!(route.answer, AnswerSpec::Relay))
        .count()
}

#[test]
fn untyped_responses_only_shrink() {
    let mut calls = UntypedCalls::default();
    for (_, file) in parsed_sources() {
        calls.visit_file(&file);
    }
    let UntypedCalls { responses, bodies } = calls;
    let routes = relay_routes();
    let raised =
        responses > UNTYPED_RESPONSES || bodies > UNTYPED_BODIES || routes > UNTYPED_ROUTES;
    assert!(
        !raised,
        "new untyped API bodies ({responses} responses, {bodies} request bodies, \
         {routes} `-> Relay` routes; allowed {UNTYPED_RESPONSES}, {UNTYPED_BODIES} and \
         {UNTYPED_ROUTES}). Declare the route's answer and body as wire types in `routes!` \
         and decode the body with `Body::decode`."
    );
    assert!(
        responses == UNTYPED_RESPONSES && bodies == UNTYPED_BODIES && routes == UNTYPED_ROUTES,
        "untyped API bodies went down to {responses} responses, {bodies} request bodies and \
         {routes} `-> Relay` routes. Lower UNTYPED_RESPONSES, UNTYPED_BODIES and \
         UNTYPED_ROUTES in src/shell/cef/api/wire/untyped_count.rs to match."
    );
}

//! The untyped-call count behind `UNTYPED_RESPONSES` and `UNTYPED_BODIES`,
//! and the source scan every wire guard reads. Guard integrity fails a
//! change to this file's code unless the PR has the `guards-changed` label.

use std::path::{Path, PathBuf};

use syn::visit::Visit;

use super::{UNTYPED_BODIES, UNTYPED_RESPONSES};

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
            Some(last) if last.ident == "untyped" => self.responses += 1,
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

#[test]
fn untyped_responses_only_shrink() {
    let mut calls = UntypedCalls::default();
    for (_, file) in parsed_sources() {
        calls.visit_file(&file);
    }
    let UntypedCalls { responses, bodies } = calls;
    let raised = responses > UNTYPED_RESPONSES || bodies > UNTYPED_BODIES;
    assert!(
        !raised,
        "new untyped API bodies ({responses} responses, {bodies} request bodies; \
         allowed {UNTYPED_RESPONSES} and {UNTYPED_BODIES}). Declare the route's answer \
         and body as wire types in `routes!` and decode the body with `Body::decode`."
    );
    assert!(
        responses == UNTYPED_RESPONSES && bodies == UNTYPED_BODIES,
        "untyped API bodies went down to {responses} responses and {bodies} request bodies. \
         Lower UNTYPED_RESPONSES and UNTYPED_BODIES in src/shell/cef/api/wire.rs to match."
    );
}

//! The JSON contract with the UI. Every body the API answers through
//! `ApiResponse::ok` or decodes through `ApiRequest::body` is a Rust type
//! listed in [`wire_types!`]. `ui/src/lib/api/wire.gen.ts` is rendered from
//! that list by `generated_typescript_is_current` and is never edited by hand.

use serde::Serialize;
use ts_rs::TS;

use crate::companion::CompanionStatus;
use crate::library::browse::FilterOptions;
use crate::library::model::{DetailView, ItemSummary};
use crate::playback::PlayerSnapshot;

use super::AppStatus;
use super::playback::PlayerCommandBody;
use super::settings::SettingsView;

/// A type the UI reads or sends by name. Only [`wire_types!`] implements it,
/// so a handler cannot answer with `json!` or with a type the generated
/// TypeScript does not know.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a wire type, so the UI has no generated TypeScript for it",
    label = "API bodies must be named types, not `json!` values",
    note = "derive `Serialize` (or `Deserialize`) and `ts_rs::TS` on a named type, add it to `wire_types!` in src/shell/cef/api/wire.rs, then run `just wire-types`"
)]
pub(super) trait Wire: TS + 'static {}

macro_rules! wire_types {
    ($($ty:ty),* $(,)?) => {
        $(impl Wire for $ty {})*

        #[cfg(test)]
        fn visit_roots(visitor: &mut impl ts_rs::TypeVisitor) {
            $(visitor.visit::<$ty>();)*
        }
    };
}

wire_types! {
    AppStatus,
    CompanionStatus,
    DetailView,
    ErrorBody,
    FilterOptions,
    ItemSummary,
    PlayerCommandBody,
    PlayerSnapshot,
    SettingsView,
}

/// The body of every non-2xx JSON response.
#[derive(Serialize, TS)]
pub(super) struct ErrorBody {
    pub error: String,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expired: Option<bool>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use syn::visit::Visit;
    use ts_rs::{Config, TS, TypeVisitor};

    const GENERATED: &str = "ui/src/lib/api/wire.gen.ts";

    /// Handlers that predate this module answer through `ApiResponse::untyped`
    /// and decode through `ApiRequest::untyped_body`. Moving one to a wire
    /// type lowers these; nothing raises them.
    const UNTYPED_RESPONSES: usize = 91; // guard-ratchet
    const UNTYPED_BODIES: usize = 32; // guard-ratchet

    struct Collect<'a> {
        cfg: &'a Config,
        decls: BTreeMap<String, String>,
    }

    impl TypeVisitor for Collect<'_> {
        fn visit<T: TS + 'static + ?Sized>(&mut self) {
            if T::output_path().is_none() || self.decls.contains_key(&T::ident(self.cfg)) {
                return;
            }
            self.decls
                .insert(T::ident(self.cfg), format_decl(&T::decl(self.cfg)));
            T::visit_dependencies(self);
        }
    }

    /// Calls `visit(index, byte, depth)` for each byte of `text` outside
    /// string literals and comments, so a `,` or `|` in a doc comment or a
    /// string never splits a declaration. `depth` counts the brackets that
    /// enclose the byte.
    fn scan(text: &str, mut visit: impl FnMut(usize, u8, usize)) {
        let bytes = text.as_bytes();
        let (mut depth, mut index) = (0usize, 0);
        while index < bytes.len() {
            match bytes[index] {
                b'"' => {
                    index += 1;
                    while index < bytes.len() && bytes[index] != b'"' {
                        index += if bytes[index] == b'\\' { 2 } else { 1 };
                    }
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    index += text[index..].find("*/").map_or(bytes.len(), |end| end + 1);
                }
                byte => {
                    if matches!(byte, b'}' | b')' | b'>' | b']') {
                        depth = depth.saturating_sub(1);
                    }
                    visit(index, byte, depth);
                    if matches!(byte, b'{' | b'(' | b'<' | b'[') {
                        depth += 1;
                    }
                }
            }
            index += 1;
        }
    }

    fn split_top_level(text: &str, separator: u8) -> Vec<&str> {
        let mut cuts = Vec::new();
        scan(text, |index, byte, depth| {
            if depth == 0 && byte == separator {
                cuts.push(index);
            }
        });
        let mut parts = Vec::new();
        let mut start = 0;
        for cut in cuts {
            parts.push(&text[start..cut]);
            start = cut + 1;
        }
        parts.push(&text[start..]);
        parts
    }

    /// ts-rs prints each declaration on one line, with doc comments inline.
    /// One field or variant per line keeps a review diff to the part that
    /// changed.
    fn format_decl(decl: &str) -> String {
        let Some((head, rhs)) = decl.split_once(" = ") else {
            return decl.to_string();
        };
        let rhs = rhs.strip_suffix(';').unwrap_or(rhs).trim();
        let variants = split_top_level(rhs, b'|');
        if variants.len() > 1 {
            let lines = variants
                .iter()
                .map(|variant| format!("\n  | {}", tidy_inline(variant.trim())))
                .collect::<String>();
            return format!("{head} ={lines};");
        }
        let mut object_end = None;
        scan(rhs, |index, byte, depth| {
            if depth == 0 && byte == b'}' && object_end.is_none() {
                object_end = Some(index);
            }
        });
        if rhs.starts_with('{') && object_end == Some(rhs.len() - 1) {
            let fields = split_top_level(&rhs[1..rhs.len() - 1], b',')
                .into_iter()
                .map(str::trim)
                .filter(|field| !field.is_empty())
                .map(format_field)
                .collect::<String>();
            return format!("{head} = {{\n{fields}}};");
        }
        format!("{head} = {};", tidy_inline(rhs))
    }

    fn format_field(field: &str) -> String {
        let Some((doc, field)) = field
            .strip_prefix("/**")
            .and_then(|rest| rest.split_once("*/"))
        else {
            return format!("  {field},\n");
        };
        let lines = doc
            .lines()
            .map(|line| line.trim().trim_start_matches('*').trim())
            .filter(|line| !line.is_empty())
            .map(|line| format!("   * {line}\n"))
            .collect::<String>();
        format!("  /**\n{lines}   */\n  {},\n", field.trim())
    }

    /// Drops the comma ts-rs leaves before an inline object's closing brace:
    /// `{ "command": "seek", positionMs: number, }`.
    fn tidy_inline(text: &str) -> String {
        let mut dangling = Vec::new();
        scan(text, |index, byte, _| {
            if byte == b',' && text[index + 1..].trim_start().starts_with('}') {
                dangling.push(index);
            }
        });
        let mut out = text.to_string();
        for index in dangling.into_iter().rev() {
            out.remove(index);
        }
        out
    }

    fn render() -> String {
        let cfg = Config::new().with_large_int("number");
        let mut collect = Collect {
            cfg: &cfg,
            decls: BTreeMap::new(),
        };
        super::visit_roots(&mut collect);
        let mut out = String::from(
            "// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.\n",
        );
        for decl in collect.decls.values() {
            out.push_str("\nexport ");
            out.push_str(decl);
            out.push('\n');
        }
        out
    }

    #[test]
    fn generated_typescript_is_current() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(GENERATED);
        let rendered = render();
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        if rendered == committed {
            return;
        }
        if std::env::var_os("MEDIAFLICK_WRITE_WIRE_TYPES").is_some() {
            std::fs::write(&path, rendered).expect("write wire.gen.ts");
            return;
        }
        let first = rendered
            .lines()
            .chain(std::iter::repeat("<missing>"))
            .zip(committed.lines().chain(std::iter::repeat("<missing>")))
            .take(rendered.lines().count().max(committed.lines().count()))
            .find(|(new, old)| new != old)
            .map_or_else(String::new, |(new, old)| {
                format!("\n  committed: {old}\n  rust now:  {new}")
            });
        panic!(
            "{GENERATED} does not match the Rust wire types. Run `just wire-types` and commit the result.{first}"
        );
    }

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

    fn parsed_sources() -> Vec<(PathBuf, syn::File)> {
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
            if path
                .segments
                .last()
                .is_some_and(|last| last.ident == "untyped")
            {
                self.responses += 1;
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
        assert!(
            responses <= UNTYPED_RESPONSES && bodies <= UNTYPED_BODIES,
            "new untyped API bodies ({responses} responses, {bodies} request bodies; \
             allowed {UNTYPED_RESPONSES} and {UNTYPED_BODIES}). Answer with \
             `ApiResponse::ok(<wire type>)` and decode with `request.body::<wire type>()`."
        );
        assert!(
            responses == UNTYPED_RESPONSES && bodies == UNTYPED_BODIES,
            "untyped API bodies went down to {responses} responses and {bodies} request bodies. \
             Lower UNTYPED_RESPONSES and UNTYPED_BODIES in src/shell/cef/api/wire.rs to match."
        );
    }

    fn list_tokens(attr: &syn::Attribute, name: &str) -> Option<String> {
        match &attr.meta {
            syn::Meta::List(list) if list.path.is_ident(name) => Some(list.tokens.to_string()),
            _ => None,
        }
    }

    fn derives_ts(attrs: &[syn::Attribute]) -> bool {
        attrs
            .iter()
            .filter_map(|attr| list_tokens(attr, "derive"))
            .any(|derives| {
                derives.split(',').any(|path| {
                    path.rsplit("::")
                        .next()
                        .is_some_and(|name| name.trim() == "TS")
                })
            })
    }

    /// serde leaves out a `skip_serializing_if` field, but ts-rs types it as a
    /// present key unless the field also says `#[ts(optional)]` or carries
    /// `#[serde(default)]`. The UI would then read `undefined` as `T | null`.
    fn omitted_but_required(field: &syn::Field) -> bool {
        let serde = field
            .attrs
            .iter()
            .filter_map(|attr| list_tokens(attr, "serde"))
            .collect::<Vec<_>>();
        let omitted = serde
            .iter()
            .any(|tokens| tokens.contains("skip_serializing"));
        let defaulted = serde.iter().any(|tokens| {
            tokens
                .split(',')
                .any(|part| part.trim().starts_with("default"))
        });
        let optional = field
            .attrs
            .iter()
            .filter_map(|attr| list_tokens(attr, "ts"))
            .any(|tokens| tokens.contains("optional"));
        omitted && !defaulted && !optional
    }

    struct OmittedFields<'a> {
        file: &'a Path,
        found: Vec<String>,
    }

    impl OmittedFields<'_> {
        fn check(&mut self, owner: &syn::Ident, fields: &syn::Fields) {
            for field in fields.iter().filter(|field| omitted_but_required(field)) {
                let name = field
                    .ident
                    .as_ref()
                    .map_or_else(|| "<tuple field>".to_string(), ToString::to_string);
                self.found
                    .push(format!("{}: {owner}.{name}", self.file.display()));
            }
        }
    }

    impl Visit<'_> for OmittedFields<'_> {
        fn visit_item_struct(&mut self, item: &syn::ItemStruct) {
            if derives_ts(&item.attrs) {
                self.check(&item.ident, &item.fields);
            }
        }

        fn visit_item_enum(&mut self, item: &syn::ItemEnum) {
            if derives_ts(&item.attrs) {
                for variant in &item.variants {
                    self.check(&variant.ident, &variant.fields);
                }
            }
        }
    }

    #[test]
    fn omitted_fields_are_optional_in_typescript() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        for (path, file) in parsed_sources() {
            let mut visitor = OmittedFields {
                file: path.strip_prefix(root).unwrap_or(&path),
                found: Vec::new(),
            };
            visitor.visit_file(&file);
            found.append(&mut visitor.found);
        }
        assert!(
            found.is_empty(),
            "these wire fields can be left out of the JSON but are typed as always present. \
             Add `#[ts(optional)]` (or `#[ts(optional = nullable)]` for a non-`Option` field):\n  {}",
            found.join("\n  ")
        );
    }
}

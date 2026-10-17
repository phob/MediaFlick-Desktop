//! The JSON contract with the UI. `ApiResponse::ok` and `ApiRequest::body`
//! take only types listed in [`wire_types!`], and `ui/src/lib/api/wire.gen.ts`
//! is rendered from that list by `generated_typescript_is_current`. The JSON
//! encoder and decoder they share are private to this module; the handlers
//! that predate it use `ApiResponse::untyped` and `ApiRequest::untyped_body`,
//! which `untyped_responses_only_shrink` counts.

use serde::Serialize;
use serde::de::DeserializeOwned;
use ts_rs::TS;

use crate::jellyfin::api::ApiError;

use crate::companion::CompanionStatus;
use crate::library::browse::FilterOptions;
use crate::library::model::{DetailView, ItemSummary};
use crate::playback::PlayerSnapshot;

use super::playback::PlayerCommandBody;
use super::settings::SettingsView;
use super::{ApiRequest, ApiResponse, AppStatus, NO_STORE};

/// A type the UI reads or sends by name. Only [`wire_types!`] implements it,
/// so a handler cannot answer with `json!` or with a type the generated
/// TypeScript does not know.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a wire type, so the UI has no generated TypeScript for it",
    label = "API bodies must be named types, not `json!` values",
    note = "derive `Serialize` (or `Deserialize`) and `ts_rs::TS` on a named type, add it to `wire_types!` in src/shell/cef/api/wire.rs, then run `just wire-types`"
)]
pub(super) trait Wire: TS + sealed::Sealed + 'static {}

mod sealed {
    /// Only [`super::wire_types!`] implements this, so no other module can
    /// make a type [`super::Wire`] without listing it.
    pub trait Sealed {}
}

macro_rules! wire_types {
    ($($ty:ty),* $(,)?) => {
        $(impl sealed::Sealed for $ty {})*
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
    FilterOptions,
    ItemSummary,
    PlayerCommandBody,
    PlayerSnapshot,
    SettingsView,
}

/// The body of every non-2xx JSON response. It is generated for the UI but
/// is not a [`Wire`] type, so `ApiResponse::ok` cannot send it with a 200.
#[derive(Serialize, TS)]
struct ErrorBody {
    error: String,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    expired: Option<bool>,
}

impl ApiRequest {
    /// Decodes the JSON body into the endpoint's request type. A request sent
    /// without a body decodes like `{}`, so it is accepted exactly when every
    /// field of `T` is optional. A body that is not JSON, or does not match
    /// `T`, is rejected rather than read as defaults: a garbled "mark as
    /// unwatched" must not mark the item watched.
    pub(super) fn body<T: DeserializeOwned + Wire>(&self) -> Result<T, ApiResponse> {
        self.decode()
    }

    /// [`Self::body`] for a request type the generated TypeScript does not
    /// know. `untyped_responses_only_shrink` keeps it from spreading.
    pub(super) fn untyped_body<T: DeserializeOwned>(&self) -> Result<T, ApiResponse> {
        self.decode()
    }

    fn decode<T: DeserializeOwned>(&self) -> Result<T, ApiResponse> {
        let body: &[u8] = if self.body.iter().all(u8::is_ascii_whitespace) {
            b"{}"
        } else {
            &self.body
        };
        serde_json::from_slice(body)
            .map_err(|error| ApiResponse::error(400, format!("invalid request body: {error}")))
    }
}

impl ApiResponse {
    fn json(status: u16, value: impl Serialize) -> Self {
        let body = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
        drop(value);
        Self {
            status,
            content_type: "application/json; charset=utf-8".to_string(),
            body,
            cache_control: NO_STORE,
            headers: Vec::new(),
        }
    }

    pub(super) fn ok<T: Serialize + Wire>(value: T) -> Self {
        Self::json(200, value)
    }

    /// A response the UI reads without a generated type. Answer with a
    /// [`Wire`] type through [`Self::ok`] instead;
    /// `untyped_responses_only_shrink` keeps this from spreading.
    pub(super) fn untyped(value: impl Serialize) -> Self {
        Self::json(200, value)
    }

    pub(super) fn error(status: u16, message: impl Into<String>) -> Self {
        Self::json(
            status,
            ErrorBody {
                error: message.into(),
                expired: None,
            },
        )
    }

    pub(super) fn from_api_error(error: &ApiError) -> Self {
        Self::json(
            error.client_status(),
            ErrorBody {
                error: error.to_string(),
                expired: Some(*error == ApiError::Unauthorized),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::collections::{BTreeMap, HashSet};
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
        visited: HashSet<TypeId>,
        decls: BTreeMap<String, String>,
    }

    impl TypeVisitor for Collect<'_> {
        fn visit<T: TS + 'static + ?Sized>(&mut self) {
            if T::output_path().is_none() || !self.visited.insert(TypeId::of::<T>()) {
                return;
            }
            let name = T::ident(self.cfg);
            let decl = format_decl(&T::decl(self.cfg));
            // Each instantiation of a generic type renders the same
            // declaration; a different declaration is a second type.
            if let Some(seen) = self.decls.get(&name) {
                assert!(
                    *seen == decl,
                    "two wire types are both named `{name}`; rename one with `#[ts(rename = \"...\")]`"
                );
            } else {
                self.decls.insert(name, decl);
            }
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
            return format!("  {},\n", tidy_inline(field));
        };
        let lines = doc
            .trim()
            .lines()
            .map(|line| {
                let line = line.trim();
                match line.strip_prefix('*') {
                    Some(text) => format!("   *{}\n", text.trim_end()),
                    None if line.is_empty() => "   *\n".to_string(),
                    None => format!("   * {line}\n"),
                }
            })
            .collect::<String>();
        format!("  /**\n{lines}   */\n  {},\n", tidy_inline(field.trim()))
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
            visited: HashSet::new(),
            decls: BTreeMap::new(),
        };
        super::visit_roots(&mut collect);
        collect.visit::<super::ErrorBody>();
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
        // A Windows checkout may hold the file with CRLF line endings.
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        if rendered == committed {
            return;
        }
        if std::env::var("MEDIAFLICK_WRITE_WIRE_TYPES").as_deref() == Ok("1") {
            std::fs::write(&path, rendered).expect("write wire.gen.ts");
            println!("rewrote {GENERATED}");
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

    struct AttrKey {
        name: String,
        /// The value when it is a `true` or `false` literal.
        flag: Option<bool>,
        /// Written as `key(...)` rather than `key` or `key = value`.
        nested: bool,
    }

    fn attr_keys(attr: &syn::Attribute) -> syn::Result<Vec<AttrKey>> {
        let mut keys = Vec::new();
        attr.parse_nested_meta(|meta| {
            let name = meta
                .path
                .get_ident()
                .map_or_else(String::new, ToString::to_string);
            let (mut flag, mut nested) = (None, false);
            if meta.input.peek(syn::Token![=]) {
                let value: syn::Expr = meta.value()?.parse()?;
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Bool(value),
                    ..
                }) = value
                {
                    flag = Some(value.value);
                }
            } else if meta.input.peek(syn::token::Paren) {
                nested = true;
                meta.parse_nested_meta(|inner| {
                    if inner.input.peek(syn::Token![=]) {
                        inner.value()?.parse::<syn::Expr>()?;
                    }
                    Ok(())
                })?;
            }
            keys.push(AttrKey { name, flag, nested });
            Ok(())
        })?;
        Ok(keys)
    }

    fn has_key(keys: &[AttrKey], name: &str) -> bool {
        keys.iter().any(|key| key.name == name)
    }

    /// The serde keys ts-rs 12 reads for each kind of item
    /// (`ts-rs-macros/src/attr`). It skips any other key with a warning that
    /// no build fails on, and it drops the whole `#[serde(...)]` attribute
    /// when a key it knows is written as `key(...)`.
    const STRUCT_SERDE: &[&str] = &[
        "rename",
        "rename_all",
        "tag",
        "bound",
        "default",
        "deny_unknown_fields",
        "crate",
    ];
    const ENUM_SERDE: &[&str] = &[
        "rename",
        "rename_all",
        "rename_all_fields",
        "tag",
        "content",
        "untagged",
        "bound",
        "crate",
    ];
    const VARIANT_SERDE: &[&str] = &["rename", "rename_all", "skip", "untagged", "borrow"];
    const FIELD_SERDE: &[&str] = &[
        "rename",
        "skip",
        "skip_serializing_if",
        "skip_serializing",
        "flatten",
        "default",
        "borrow",
    ];
    /// Keys that only change how input is matched, so ts-rs skipping them
    /// leaves the TypeScript right.
    const DECODE_ONLY_SERDE: &[&str] = &["alias", "deny_unknown_fields"];

    struct SerdeFidelity<'a> {
        file: &'a Path,
        found: Vec<String>,
    }

    /// The attributes of one item as ts-rs will read them.
    struct Seen {
        ts: Vec<AttrKey>,
        /// The serde keys ts-rs reads for this kind of item.
        serde: Vec<AttrKey>,
        /// `#[ts(type = ...)]` or `#[ts(as = ...)]` states the TypeScript.
        overridden: bool,
    }

    impl SerdeFidelity<'_> {
        fn report(&mut self, at: &str, problem: &str) {
            self.found
                .push(format!("{}: {at} {problem}", self.file.display()));
        }

        fn keys(&mut self, at: &str, attrs: &[syn::Attribute], name: &str) -> Vec<Vec<AttrKey>> {
            let mut all = Vec::new();
            for attr in attrs.iter().filter(|attr| attr.path().is_ident(name)) {
                match attr_keys(attr) {
                    Ok(keys) => all.push(keys),
                    Err(error) => self.report(
                        at,
                        &format!("has a `#[{name}]` attribute this check cannot read: {error}"),
                    ),
                }
            }
            all
        }

        fn seen(&mut self, at: &str, attrs: &[syn::Attribute], accepted: &[&str]) -> Seen {
            let ts = self
                .keys(at, attrs, "ts")
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            let overridden = has_key(&ts, "type") || has_key(&ts, "as");
            if has_key(&ts, "optional_fields") {
                self.report(
                    at,
                    "uses `#[ts(optional_fields)]`; mark each field `#[ts(optional)]` instead",
                );
            }
            let mut serde = Vec::new();
            for keys in self.keys(at, attrs, "serde") {
                if let Some(key) = keys
                    .iter()
                    .find(|key| key.nested && accepted.contains(&key.name.as_str()))
                {
                    if !overridden {
                        self.report(at, &format!("has `#[serde({}(...))]`, which ts-rs cannot read, so it ignores that whole attribute; use `{} = \"...\"` or describe the JSON with `#[ts(type = ...)]`", key.name, key.name));
                    }
                    continue;
                }
                for key in keys {
                    if accepted.contains(&key.name.as_str()) {
                        serde.push(key);
                    } else if !overridden && !DECODE_ONLY_SERDE.contains(&key.name.as_str()) {
                        self.report(at, &format!("has `#[serde({})]`, which ts-rs ignores; describe the JSON with `#[ts(type = ...)]` or `#[ts(as = ...)]`", key.name));
                    }
                }
            }
            Seen {
                ts,
                serde,
                overridden,
            }
        }

        /// serde leaves out a `skip_serializing_if` field, but ts-rs types it
        /// as a present key unless the field says `#[ts(optional)]` or carries
        /// `#[serde(default)]`. The last `#[ts(optional ...)]` wins over both,
        /// as in ts-rs, and a `#[ts(type = ...)]` changes the value type only.
        /// The UI would otherwise read `undefined` as `T | null`.
        fn fields(&mut self, owner: &str, fields: &syn::Fields) {
            for (index, field) in fields.iter().enumerate() {
                let name = field
                    .ident
                    .as_ref()
                    .map_or_else(|| index.to_string(), ToString::to_string);
                let at = format!("{owner}.{name}");
                let Seen { ts, serde, .. } = self.seen(&at, &field.attrs, FIELD_SERDE);
                let optional = ts
                    .iter()
                    .rfind(|key| key.name == "optional")
                    .map_or_else(|| has_key(&serde, "default"), |key| key.flag != Some(false));
                let omitted =
                    has_key(&serde, "skip_serializing_if") || has_key(&serde, "skip_serializing");
                if omitted && !optional {
                    self.report(&at, "can be left out of the JSON but is typed as always present; add `#[ts(optional)]` (`#[ts(optional = nullable)]` for a non-`Option` field)");
                }
            }
        }
    }

    impl Visit<'_> for SerdeFidelity<'_> {
        fn visit_item_struct(&mut self, item: &syn::ItemStruct) {
            if derives_ts(&item.attrs) {
                let owner = item.ident.to_string();
                if !self.seen(&owner, &item.attrs, STRUCT_SERDE).overridden {
                    self.fields(&owner, &item.fields);
                }
            }
        }

        fn visit_item_enum(&mut self, item: &syn::ItemEnum) {
            if derives_ts(&item.attrs) {
                let owner = item.ident.to_string();
                if self.seen(&owner, &item.attrs, ENUM_SERDE).overridden {
                    return;
                }
                for variant in &item.variants {
                    let owner = format!("{owner}::{}", variant.ident);
                    if !self.seen(&owner, &variant.attrs, VARIANT_SERDE).overridden {
                        self.fields(&owner, &variant.fields);
                    }
                }
            }
        }
    }

    #[test]
    fn generated_typescript_follows_serde() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        for (path, file) in parsed_sources() {
            let mut visitor = SerdeFidelity {
                file: path.strip_prefix(root).unwrap_or(&path),
                found: Vec::new(),
            };
            visitor.visit_file(&file);
            found.append(&mut visitor.found);
        }
        assert!(
            found.is_empty(),
            "the generated TypeScript would not match what serde sends:\n  {}",
            found.join("\n  ")
        );
    }
}

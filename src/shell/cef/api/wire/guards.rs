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
    let raised = responses > UNTYPED_RESPONSES || bodies > UNTYPED_BODIES;
    assert!(
        !raised,
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

fn derives(attrs: &[syn::Attribute], derive: &str) -> bool {
    attrs
        .iter()
        .filter_map(|attr| list_tokens(attr, "derive"))
        .any(|derives| {
            derives.split(',').any(|path| {
                path.rsplit("::")
                    .next()
                    .is_some_and(|name| name.trim() == derive)
            })
        })
}

fn is_option(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Path(path) if path.qself.is_none()
        && path.path.segments.last().is_some_and(|last| last.ident == "Option"))
}

struct AttrKey {
    name: String,
    /// The value when it is a `true` or `false` literal.
    flag: Option<bool>,
    /// The value when it is a string literal or a single word.
    text: Option<String>,
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
        let (mut flag, mut text, mut nested) = (None, None, false);
        if meta.input.peek(syn::Token![=]) {
            match meta.value()?.parse()? {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Bool(value),
                    ..
                }) => flag = Some(value.value),
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(value),
                    ..
                }) => text = Some(value.value()),
                syn::Expr::Path(path) => text = path.path.get_ident().map(ToString::to_string),
                _ => {}
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
        keys.push(AttrKey {
            name,
            flag,
            text,
            nested,
        });
        Ok(())
    })?;
    Ok(keys)
}

fn has_key(keys: &[AttrKey], name: &str) -> bool {
    keys.iter().any(|key| key.name == name)
}

fn last_key<'a>(keys: &'a [AttrKey], name: &str) -> Option<&'a AttrKey> {
    keys.iter().rfind(|key| key.name == name)
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
    /// The keys of `#[serde(...)]` attributes ts-rs drops whole. serde
    /// still obeys them.
    dropped: Vec<AttrKey>,
    /// `#[ts(type = ...)]` or `#[ts(as = ...)]` states the TypeScript.
    overridden: bool,
}

/// What a type's own attributes mean for each of its fields.
#[derive(Clone, Copy)]
struct Container {
    /// `#[ts(optional_fields)]` makes each `Option` field `?: T`, and
    /// `#[ts(optional_fields = nullable)]` makes it `?: T | null`.
    optional_fields: bool,
    /// Only decoded, never sent, so a key serde does not require must be
    /// optional in the TypeScript.
    decoded_only: bool,
    /// `#[serde(default)]` on the type makes every field optional to serde.
    default: bool,
}

fn optional_fields(seen: &Seen) -> Option<bool> {
    last_key(&seen.ts, "optional_fields").map(|key| key.flag != Some(false))
}

impl Container {
    /// serde sends `null` for a `None` field it does not skip, so only a type
    /// that is never sent may drop `null` from its `Option` fields; serde
    /// decodes a missing key and `null` the same way.
    fn new(
        fidelity: &mut SerdeFidelity<'_>,
        at: &str,
        attrs: &[syn::Attribute],
        seen: &Seen,
    ) -> Self {
        let container = Self {
            optional_fields: optional_fields(seen).unwrap_or(false),
            decoded_only: derives(attrs, "Deserialize") && !derives(attrs, "Serialize"),
            default: has_key(&seen.serde, "default") || has_key(&seen.dropped, "default"),
        };
        container.check(fidelity, at, seen);
        container
    }

    fn check(self, fidelity: &mut SerdeFidelity<'_>, at: &str, seen: &Seen) {
        let drops_null = last_key(&seen.ts, "optional_fields")
            .is_some_and(|key| key.flag != Some(false) && key.text.as_deref() != Some("nullable"));
        if drops_null && !self.decoded_only {
            fidelity.report(at, "uses `#[ts(optional_fields)]`, which types the `null` serde sends for `None` as absent; write `#[ts(optional_fields = nullable)]`");
        }
    }

    /// A variant takes the enum's derives, and its `optional_fields` unless
    /// it sets its own.
    fn variant(self, fidelity: &mut SerdeFidelity<'_>, at: &str, seen: &Seen) -> Self {
        let variant = Self {
            optional_fields: optional_fields(seen).unwrap_or(self.optional_fields),
            ..self
        };
        variant.check(fidelity, at, seen);
        variant
    }
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
        // A field's override replaces only its value type; ts-rs still
        // takes the key and the `?` from the serde attributes it reads.
        let reports_nested = !overridden || accepted == FIELD_SERDE;
        let (mut serde, mut dropped) = (Vec::new(), Vec::new());
        for keys in self.keys(at, attrs, "serde") {
            if let Some(key) = keys
                .iter()
                .find(|key| key.nested && accepted.contains(&key.name.as_str()))
            {
                if reports_nested {
                    self.report(at, &format!("has `#[serde({}(...))]`, which ts-rs cannot read, so it ignores that whole attribute; use `{} = \"...\"`, or describe the whole type with `#[ts(type = ...)]`", key.name, key.name));
                }
                dropped.extend(keys);
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
            dropped,
            overridden,
        }
    }

    /// serde leaves out a `skip_serializing_if` field, but ts-rs types it
    /// as a present key unless the field says `#[ts(optional)]` or carries
    /// `#[serde(default)]`. The last `#[ts(optional ...)]` wins over both,
    /// as in ts-rs, and a `#[ts(type = ...)]` changes the value type only.
    /// A skip in an attribute ts-rs drops still omits the key. The UI
    /// would otherwise read `undefined` as `T | null`.
    ///
    /// A type that is only decoded is a request body: serde accepts it
    /// without an `Option` field or a field with a default, so the
    /// TypeScript must let the UI leave that key out.
    fn fields(&mut self, owner: &str, fields: &syn::Fields, container: Container) {
        for (index, field) in fields.iter().enumerate() {
            let name = field
                .ident
                .as_ref()
                .map_or_else(|| index.to_string(), ToString::to_string);
            let at = format!("{owner}.{name}");
            let Seen {
                ts, serde, dropped, ..
            } = self.seen(&at, &field.attrs, FIELD_SERDE);
            let omitted = [&serde, &dropped].into_iter().any(|keys| {
                has_key(keys, "skip_serializing_if") || has_key(keys, "skip_serializing")
            });
            // ts-rs reads `Option` from the `#[ts(as = ...)]` type, and a
            // `#[ts(type = ...)]` field does not inherit `optional_fields`.
            let ts_type = last_key(&ts, "as")
                .and_then(|key| key.text.as_deref())
                .and_then(|text| syn::parse_str::<syn::Type>(text).ok())
                .unwrap_or_else(|| field.ty.clone());
            let optional = match last_key(&ts, "optional") {
                Some(key) => key.flag != Some(false),
                None if container.optional_fields && !has_key(&ts, "type") => is_option(&ts_type),
                None => omitted && has_key(&serde, "default"),
            };
            if omitted && !optional {
                self.report(&at, "can be left out of the JSON but is typed as always present; add `#[ts(optional)]` (`#[ts(optional = nullable)]` for a non-`Option` field)");
            }
            let skipped = [&serde, &dropped]
                .into_iter()
                .any(|keys| has_key(keys, "skip") || has_key(keys, "flatten"));
            let unrequired = container.default
                || is_option(&field.ty)
                || [&serde, &dropped]
                    .into_iter()
                    .any(|keys| has_key(keys, "default"));
            if container.decoded_only && unrequired && !skipped && !optional {
                self.report(&at, "can be left out of the request but is typed as required; add `#[ts(optional = nullable)]`, or `#[ts(optional_fields)]` on the type for its `Option` fields");
            }
        }
    }
}

impl Visit<'_> for SerdeFidelity<'_> {
    fn visit_item_struct(&mut self, item: &syn::ItemStruct) {
        if derives(&item.attrs, "TS") {
            let owner = item.ident.to_string();
            let seen = self.seen(&owner, &item.attrs, STRUCT_SERDE);
            if !seen.overridden {
                let container = Container::new(self, &owner, &item.attrs, &seen);
                self.fields(&owner, &item.fields, container);
            }
        }
    }

    fn visit_item_enum(&mut self, item: &syn::ItemEnum) {
        if derives(&item.attrs, "TS") {
            let owner = item.ident.to_string();
            let seen = self.seen(&owner, &item.attrs, ENUM_SERDE);
            if seen.overridden {
                return;
            }
            let container = Container::new(self, &owner, &item.attrs, &seen);
            for variant in &item.variants {
                let owner = format!("{owner}::{}", variant.ident);
                let seen = self.seen(&owner, &variant.attrs, VARIANT_SERDE);
                if !seen.overridden {
                    let variant_container = container.variant(self, &owner, &seen);
                    self.fields(&owner, &variant.fields, variant_container);
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
        "the generated TypeScript would not match what serde sends or accepts:\n  {}",
        found.join("\n  ")
    );
}

use std::collections::BTreeSet;
use std::path::Path;

use syn::visit::Visit;

use super::super::MODULES;
use super::super::routes::{Route, Segment};
use super::untyped_count::parsed_sources;

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
    /// Decoded by a derived `Deserialize`, so every key the TypeScript lets
    /// the UI leave out must be one serde can do without.
    decoded: bool,
    /// Only decoded, never sent, so a `None` may be typed as an absent key.
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
            decoded: derives(attrs, "Deserialize"),
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
    /// A decoded type must not let the UI leave out a key serde requires,
    /// or the request is rejected. The reverse is safe: the TypeScript may
    /// require a key serde could do without.
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
            let required = !container.default
                && !is_option(&field.ty)
                && ![&serde, &dropped]
                    .into_iter()
                    .any(|keys| has_key(keys, "default"));
            if container.decoded && required && !skipped && optional {
                self.report(&at, "is required by serde but optional in the TypeScript, so a request that leaves it out is rejected; drop the `#[ts(optional)]` or `#[ts(optional_fields)]` that makes it optional, or give the field `#[serde(default)]`");
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

/// Why a settings patch field may name an enum while taking a string.
const FROM_ID: &str = "`from_id` decodes the enum by its serde name, which the TypeScript names";

/// What `generated_typescript_follows_serde` cannot check, each with why
/// the TypeScript still matches serde: a `TS` type whose `Serialize` or
/// `Deserialize` is written by hand, and a `#[ts(type = ...)]` or
/// `#[ts(as = ...)]` on a type the app decodes, on its variants or on its
/// fields. `unchecked_claims_are_listed` fails on one missing here and on
/// an entry nothing claims any more.
const UNCHECKED_CLAIMS: &[(&str, &str)] = &[
    ("AppearanceSettingsPatch.accent", FROM_ID),
    ("AppearanceSettingsPatch.density", FROM_ID),
    ("ApplicationSettingsPatch.close_behavior", FROM_ID),
    (
        "CollectionSource::TmdbDiscover.parameters",
        "a `serde_json::Value` is any JSON value, which `JsonValue` types",
    ),
    (
        "FilterKey",
        "`Serialize` writes `FilterKey::id`, each variant's name in camel case, as `rename_all` says",
    ),
    (
        "HomeElement",
        "`Deserialize` reads the `kind`-tagged `id` with `enabled` that the derived `Serialize` writes",
    ),
    (
        "LibraryId",
        "a newtype over `String` decoded with `try_from = \"String\"`, so a string both ways",
    ),
    ("PlaybackSettingsPatch.skip_commercial", FROM_ID),
    ("PlaybackSettingsPatch.skip_credits", FROM_ID),
    ("PlaybackSettingsPatch.skip_intro", FROM_ID),
    ("PlaybackSettingsPatch.skip_recap", FROM_ID),
    ("PlaybackSettingsPatch.streaming_quality", FROM_ID),
    ("PlayerSettingsPatch.default_fullscreen", FROM_ID),
    (
        "PlayerSettingsPatch.mark_watched_next",
        "`NullablePatch` takes `T` or `null`, which `T | null` types; `null` clears and a missing key keeps",
    ),
    (
        "PlayerSettingsPatch.mpv_path",
        "`NullablePatch` takes `T` or `null`, which `T | null` types; `null` clears and a missing key keeps",
    ),
    ("PlayerSettingsPatch.player_backend", FROM_ID),
    (
        "SortKey",
        "`Serialize` writes `SortKey::id`, which each variant's `#[ts(rename)]` repeats",
    ),
    (
        "ViewingSettingsWrite",
        "`ViewingWrite` reads the `ViewingView` keys, every one defaulted, and an optional `StartupView`",
    ),
];

/// Hand-written serde impls by type name, then every claim as a name in
/// [`UNCHECKED_CLAIMS`].
#[derive(Default)]
struct Claims {
    hand_serialize: BTreeSet<String>,
    hand_deserialize: BTreeSet<String>,
    found: BTreeSet<String>,
}

fn impl_target(item: &syn::ItemImpl) -> Option<(String, String)> {
    let (_, trait_path, _) = item.trait_.as_ref()?;
    let trait_name = trait_path.segments.last()?.ident.to_string();
    let syn::Type::Path(self_ty) = item.self_ty.as_ref() else {
        return None;
    };
    let type_name = self_ty.path.segments.last()?.ident.to_string();
    Some((trait_name, type_name))
}

fn overrides_ts(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("ts"))
        .filter_map(|attr| attr_keys(attr).ok())
        .any(|keys| has_key(&keys, "type") || has_key(&keys, "as"))
}

impl Claims {
    fn fields(&mut self, owner: &str, fields: &syn::Fields) {
        for (index, field) in fields.iter().enumerate() {
            if overrides_ts(&field.attrs) {
                let name = field
                    .ident
                    .as_ref()
                    .map_or_else(|| index.to_string(), ToString::to_string);
                self.found.insert(format!("{owner}.{name}"));
            }
        }
    }

    /// Records the claims of one `TS` type and reports whether it is
    /// decoded, so the caller can look at its fields.
    fn item(&mut self, name: &str, attrs: &[syn::Attribute]) -> bool {
        if self.hand_serialize.contains(name) || self.hand_deserialize.contains(name) {
            self.found.insert(name.to_string());
        }
        let decoded = derives(attrs, "Deserialize") || self.hand_deserialize.contains(name);
        if decoded && overrides_ts(attrs) {
            self.found.insert(name.to_string());
        }
        decoded
    }
}

struct HandImpls<'a>(&'a mut Claims);

impl Visit<'_> for HandImpls<'_> {
    fn visit_item_impl(&mut self, item: &syn::ItemImpl) {
        match impl_target(item) {
            Some((trait_name, type_name)) if trait_name == "Serialize" => {
                self.0.hand_serialize.insert(type_name);
            }
            Some((trait_name, type_name)) if trait_name == "Deserialize" => {
                self.0.hand_deserialize.insert(type_name);
            }
            _ => {}
        }
        syn::visit::visit_item_impl(self, item);
    }
}

impl Visit<'_> for Claims {
    fn visit_item_struct(&mut self, item: &syn::ItemStruct) {
        let name = item.ident.to_string();
        if derives(&item.attrs, "TS") && self.item(&name, &item.attrs) {
            self.fields(&name, &item.fields);
        }
    }

    fn visit_item_enum(&mut self, item: &syn::ItemEnum) {
        let name = item.ident.to_string();
        if derives(&item.attrs, "TS") && self.item(&name, &item.attrs) {
            for variant in &item.variants {
                let owner = format!("{name}::{}", variant.ident);
                if overrides_ts(&variant.attrs) {
                    self.found.insert(owner.clone());
                }
                self.fields(&owner, &variant.fields);
            }
        }
    }
}

#[test]
fn unchecked_claims_are_listed() {
    let sources = parsed_sources();
    let mut claims = Claims::default();
    for (_, file) in &sources {
        HandImpls(&mut claims).visit_file(file);
    }
    for (_, file) in &sources {
        claims.visit_file(file);
    }
    let listed = UNCHECKED_CLAIMS
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect::<BTreeSet<_>>();
    let unlisted = claims.found.difference(&listed).collect::<Vec<_>>();
    let stale = listed.difference(&claims.found).collect::<Vec<_>>();
    assert!(
        unlisted.is_empty(),
        "the guards cannot check these claims about the JSON; check each by hand and \
         add it to UNCHECKED_CLAIMS in src/shell/cef/api/wire/guards.rs with why it holds: \
         {unlisted:?}"
    );
    assert!(
        stale.is_empty(),
        "UNCHECKED_CLAIMS lists claims nothing makes any more; remove them: {stale:?}"
    );
}

fn describe(route: &Route) -> String {
    let path = route
        .path
        .iter()
        .map(|segment| match segment {
            Segment::Literal(literal) => *literal,
            Segment::Param { name } => name,
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("{} /api/{path}", route.method)
}

/// `route` tries the declarations in order and answers with the first that
/// takes the method and path, so one that an earlier declaration covers
/// never runs. Its path with every parameter set to a value no literal
/// segment has reaches it unless such a declaration comes first.
#[test]
fn every_declared_route_is_reachable() {
    let routes = MODULES
        .iter()
        .flat_map(|routes| routes.table)
        .collect::<Vec<_>>();
    let mut shadowed = Vec::new();
    for (index, route) in routes.iter().enumerate() {
        let path = route
            .path
            .iter()
            .map(|segment| match segment {
                Segment::Literal(literal) => *literal,
                Segment::Param { .. } => "\0",
            })
            .collect::<Vec<_>>();
        if let Some(earlier) = routes[..index]
            .iter()
            .find(|earlier| earlier.method == route.method && earlier.matches(&path))
        {
            shadowed.push(format!("{} after {}", describe(route), describe(earlier)));
        }
    }
    assert!(
        shadowed.is_empty(),
        "these route declarations never run, because an earlier one takes every request \
         they would; remove them or declare them first:\n  {}",
        shadowed.join("\n  ")
    );
}

/// The files that may build a success `ApiResponse` or reach past a route's
/// declaration, and why. Everywhere else a handler answers through its
/// route's declared type and reads only the body its route declares.
const ROUTER_FILES: &[(&str, &str)] = &[
    (
        "src/shell/cef/api/routes.rs",
        "turns each route's declared answer into its response and hands the handler its \
         declared body",
    ),
    (
        "src/shell/cef/api/wire.rs",
        "the JSON encoder behind `ok`, `untyped` and the errors",
    ),
    (
        "src/shell/cef/api/assets.rs",
        "builds the responses for the UI bundle's files, which no route declares",
    ),
];

/// Paths only [`ROUTER_FILES`] may name: the success builders, the body a
/// route declares, and the trait that turns a declared answer into a
/// response, by name, through the answers that are not wire types, or
/// through a glob of `routes`.
const ROUTER_ONLY: &[[&str; 2]] = &[
    ["ApiResponse", "ok"],
    ["ApiResponse", "untyped"],
    ["Body", "new"],
    ["routes", "Answer"],
    ["routes", "*"],
    ["Answer", "respond"],
    ["Answer", "success"],
    ["Relay", "respond"],
    ["Relay", "success"],
    ["Binary", "respond"],
    ["Binary", "success"],
];

/// What one file does that only [`ROUTER_FILES`] may.
struct RouterOnly {
    api: bool,
    found: Vec<String>,
}

impl RouterOnly {
    fn check(&mut self, path: &[String]) {
        if path
            .windows(2)
            .any(|pair| ROUTER_ONLY.iter().any(|only| pair == only))
        {
            self.found.push(format!("names `{}`", path.join("::")));
        }
    }
}

fn is_api_response(path: &syn::Path) -> bool {
    path.segments
        .last()
        .is_some_and(|last| last.ident == "ApiResponse")
}

fn names(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

fn use_paths(tree: &syn::UseTree, prefix: &mut Vec<String>, paths: &mut Vec<Vec<String>>) {
    let leaf = |prefix: &[String], name: String| [prefix, &[name]].concat();
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            use_paths(&path.tree, prefix, paths);
            prefix.pop();
        }
        syn::UseTree::Name(name) => paths.push(leaf(prefix, name.ident.to_string())),
        syn::UseTree::Rename(rename) => paths.push(leaf(prefix, rename.ident.to_string())),
        syn::UseTree::Glob(_) => paths.push(leaf(prefix, "*".to_string())),
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                use_paths(tree, prefix, paths);
            }
        }
    }
}

/// The `a::b::c` paths written in a macro's tokens, which syn leaves
/// unparsed, such as a `vec![..]`'s elements. A turbofish is skipped, so
/// `a::<T>::b` reads as `a::b`.
fn macro_paths(mut cursor: syn::buffer::Cursor<'_>, paths: &mut Vec<Vec<String>>) {
    let mut current = Vec::new();
    let mut colons = 0;
    while !cursor.eof() {
        if let Some((ident, next)) = cursor.ident() {
            if colons != 2 && !current.is_empty() {
                paths.push(std::mem::take(&mut current));
            }
            current.push(ident.to_string());
            colons = 0;
            cursor = next;
            continue;
        }
        if let Some((punct, next)) = cursor.punct() {
            if punct.as_char() == ':' {
                colons += 1;
                cursor = next;
                continue;
            }
            if punct.as_char() == '<' && colons == 2 {
                colons = 0;
                cursor = past_generics(next);
                continue;
            }
        }
        if !current.is_empty() {
            paths.push(std::mem::take(&mut current));
        }
        colons = 0;
        if let Some((inner, _, _, next)) = cursor.any_group() {
            macro_paths(inner, paths);
            cursor = next;
        } else if let Some((_, next)) = cursor.token_tree() {
            cursor = next;
        }
    }
    if !current.is_empty() {
        paths.push(current);
    }
}

/// The cursor past the `>` that closes a turbofish's `<`.
fn past_generics(mut cursor: syn::buffer::Cursor<'_>) -> syn::buffer::Cursor<'_> {
    let (mut depth, mut arrow) = (1_usize, false);
    while depth > 0 && !cursor.eof() {
        if let Some((punct, next)) = cursor.punct() {
            match punct.as_char() {
                '<' => depth += 1,
                '>' if !arrow => depth -= 1,
                _ => {}
            }
            arrow = punct.as_char() == '-';
            cursor = next;
        } else if let Some((_, next)) = cursor.token_tree() {
            arrow = false;
            cursor = next;
        }
    }
    cursor
}

/// A `routes!` table's request bodies, answers and calls, so the checks on
/// ordinary code run on them too.
struct RouteTable(Vec<(Option<syn::Type>, syn::Type, syn::Expr)>);

impl syn::parse::Parse for RouteTable {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        input.parse::<syn::Token![|]>()?;
        input.parse::<syn::Ident>()?;
        input.parse::<syn::Token![,]>()?;
        input.parse::<syn::Ident>()?;
        input.parse::<syn::Token![|]>()?;
        let mut rows = Vec::new();
        while !input.is_empty() {
            input.parse::<syn::Ident>()?;
            input.parse::<syn::ExprArray>()?;
            let body = if input.peek(syn::token::Paren) {
                let body;
                syn::parenthesized!(body in input);
                body.parse::<syn::Ident>()?;
                body.parse::<syn::Token![:]>()?;
                Some(body.parse()?)
            } else {
                None
            };
            input.parse::<syn::Token![->]>()?;
            let answer = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            rows.push((body, answer, input.parse()?));
            if !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
            }
        }
        Ok(Self(rows))
    }
}

impl Visit<'_> for RouterOnly {
    fn visit_path(&mut self, path: &syn::Path) {
        self.check(&names(path));
        syn::visit::visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &syn::ItemUse) {
        let mut paths = Vec::new();
        use_paths(&item.tree, &mut Vec::new(), &mut paths);
        for path in paths {
            self.check(&path);
        }
    }

    fn visit_macro(&mut self, mac: &syn::Macro) {
        if mac
            .path
            .segments
            .last()
            .is_some_and(|last| last.ident == "routes")
        {
            match mac.parse_body::<RouteTable>() {
                Ok(RouteTable(rows)) => {
                    for (body, answer, call) in &rows {
                        if let Some(body) = body {
                            self.visit_type(body);
                        }
                        self.visit_type(answer);
                        self.visit_expr(call);
                    }
                }
                Err(error) => self.found.push(format!(
                    "has a `routes!` table this check cannot read: {error}"
                )),
            }
        } else {
            let buffer = syn::buffer::TokenBuffer::new2(mac.tokens.clone());
            let mut paths = Vec::new();
            macro_paths(buffer.begin(), &mut paths);
            for path in paths {
                self.check(&path);
            }
        }
        syn::visit::visit_macro(self, mac);
    }

    fn visit_expr_struct(&mut self, expr: &syn::ExprStruct) {
        if is_api_response(&expr.path) {
            self.found
                .push("builds an `ApiResponse { .. }`".to_string());
        }
        syn::visit::visit_expr_struct(self, expr);
    }

    fn visit_item_impl(&mut self, item: &syn::ItemImpl) {
        if matches!(&*item.self_ty, syn::Type::Path(ty) if is_api_response(&ty.path)) {
            self.found.push("implements on `ApiResponse`".to_string());
        }
        syn::visit::visit_item_impl(self, item);
    }

    fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
        if self.api
            && matches!(&*assign.left, syn::Expr::Field(field)
                if matches!(&field.member, syn::Member::Named(name) if name == "status"))
        {
            self.found.push("sets a `.status`".to_string());
        }
        syn::visit::visit_expr_assign(self, assign);
    }

    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if let syn::Expr::Path(func) = &*call.func
            && names(&func.path).ends_with(&["ApiResponse".to_string(), "error".to_string()])
            && let Some(syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(status),
                ..
            })) = call.args.first()
            && !status
                .base10_parse::<u16>()
                .is_ok_and(|status| (400..600).contains(&status))
        {
            self.found
                .push(format!("answers an error with status {status}"));
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// A handler's success answer is the type its route declares, and nothing
/// else: `Answer::respond` turns it into the response in `routes.rs`, and a
/// handler's error side must not smuggle a 2xx past it.
#[test]
fn only_the_router_answers_with_success() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut outside = Vec::new();
    let mut used = BTreeSet::new();
    for (path, file) in parsed_sources() {
        let relative = path
            .strip_prefix(root)
            .expect("source under the crate")
            .to_string_lossy()
            .replace('\\', "/");
        let mut visitor = RouterOnly {
            api: relative.starts_with("src/shell/cef/api"),
            found: Vec::new(),
        };
        visitor.visit_file(&file);
        if visitor.found.is_empty() {
            continue;
        }
        if ROUTER_FILES.iter().any(|(listed, _)| *listed == relative) {
            used.insert(relative);
        } else {
            outside.extend(
                visitor
                    .found
                    .into_iter()
                    .map(|found| format!("{relative} {found}")),
            );
        }
    }
    assert!(
        outside.is_empty(),
        "only the router builds a success response or reads past a route's declaration. \
         Return the route's declared answer (a wire type, a `Relay`'s JSON value or a \
         `BinaryAnswer`) and an `ApiResponse::error` with a 4xx or 5xx status, and read the \
         body through the `Body` the route declares:\n  {}",
        outside.join("\n  ")
    );
    let stale = ROUTER_FILES
        .iter()
        .filter(|(listed, _)| !used.contains(*listed))
        .collect::<Vec<_>>();
    assert!(
        stale.is_empty(),
        "ROUTER_FILES lists files that no longer need it; remove them: {stale:?}"
    );
}

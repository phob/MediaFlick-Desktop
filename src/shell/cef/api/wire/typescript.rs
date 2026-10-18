use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use ts_rs::{Config, TS, TypeVisitor};

use super::super::MODULES;
use super::super::routes::{AnswerSpec, BodySpec, Route, Segment, WireRef};

const GENERATED: &str = "ui/src/lib/api/wire.gen.ts";
const GENERATED_DIR: &str = "ui/src/lib/api/wire";
const HEADER: &str =
    "// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.\n";

struct Decl {
    module: String,
    text: String,
    /// The wire types the declaration names.
    deps: Vec<String>,
}

struct Collect<'a> {
    cfg: &'a Config,
    visited: HashSet<TypeId>,
    decls: BTreeMap<String, Decl>,
}

/// The defining module without the crate, as the generated file's name:
/// `mediaflick_desktop::shell::cef::api::wire::ErrorBody` is `api.wire`.
fn module_of<T: ?Sized>() -> String {
    let path = std::any::type_name::<T>();
    let path = path.split_once('<').map_or(path, |(path, _)| path);
    module_name(path.rsplit_once("::").map_or("", |(module, _)| module))
}

/// A module path without the crate: `mediaflick_desktop::shell::cef::api`
/// is `api`.
fn module_name(path: &str) -> String {
    let module = path.split_once("::").map_or("", |(_, module)| module);
    let module = module.strip_prefix("shell::cef::").unwrap_or(module);
    module.replace("::", ".")
}

impl TypeVisitor for Collect<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if T::output_path().is_none() || !self.visited.insert(TypeId::of::<T>()) {
            return;
        }
        let name = T::ident(self.cfg);
        let decl = Decl {
            module: module_of::<T>(),
            text: format_decl(&T::decl(self.cfg)),
            deps: T::dependencies(self.cfg)
                .into_iter()
                .map(|dep| dep.ts_name)
                .collect(),
        };
        // Each instantiation of a generic type renders the same
        // declaration; a different declaration is a second type. Names
        // stay unique across files because the index re-exports them all.
        if let Some(seen) = self.decls.get(&name) {
            assert!(
                seen.text == decl.text,
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
    // A mapped type, `{ [key in string]: T }`, is one member and takes no
    // comma.
    let object = rhs
        .strip_prefix('{')
        .filter(|body| !body.trim_start().starts_with('['));
    if let Some(body) = object
        && object_end == Some(rhs.len() - 1)
    {
        let fields = split_top_level(&body[..body.len() - 1], b',')
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

/// The name of a module's route map: `api.collections` is
/// `ApiCollectionsRoutes`.
fn route_map_name(module: &str) -> String {
    let words = module
        .split(['.', '_'])
        .map(capitalized)
        .collect::<String>();
    format!("{words}Routes")
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_ascii_uppercase().to_string() + chars.as_str()
    })
}

fn camel_case(name: &str) -> String {
    let mut words = name.split('_');
    let first = words.next().unwrap_or_default().to_string();
    first + &words.map(capitalized).collect::<String>()
}

/// One route as a member of its module's route map, keyed by its method
/// and path. `kind` says how the UI reads the answer: as the generated
/// `response` type (`json`), as Companion JSON it describes by hand
/// (`relay`), or not at all, because it only links to the bytes
/// (`binary`).
fn route_member(route: &Route, mut name: impl FnMut(&WireRef) -> String) -> String {
    let mut path = String::from("/api");
    let mut params = Vec::new();
    for segment in route.path {
        match segment {
            Segment::Literal(literal) => path.push_str(&format!("/{literal}")),
            Segment::Param { name } => {
                let name = camel_case(name);
                path.push_str(&format!("/{{{name}}}"));
                params.push(format!("{name}: string | number"));
            }
        }
    }
    let params = if params.is_empty() {
        "null".to_string()
    } else {
        format!("{{ {} }}", params.join(", "))
    };
    let body = match &route.body {
        BodySpec::None => "null".to_string(),
        BodySpec::Json(wire) => name(wire),
        BodySpec::Bytes => "ArrayBuffer".to_string(),
    };
    let answer = match &route.answer {
        AnswerSpec::Json(wire) => format!("kind: \"json\", response: {}", name(wire)),
        AnswerSpec::Relay => "kind: \"relay\"".to_string(),
        AnswerSpec::Binary => "kind: \"binary\"".to_string(),
    };
    format!(
        "  \"{} {path}\": {{ params: {params}, body: {body}, {answer} }},\n",
        route.method
    )
}

/// Every generated file by its path from the repository root: one per
/// Rust module, importing what it names from the others, and the index
/// that re-exports them and joins the modules' route maps into `Routes`.
fn render() -> BTreeMap<String, String> {
    let cfg = Config::new().with_large_int("number");
    let mut collect = Collect {
        cfg: &cfg,
        visited: HashSet::new(),
        decls: BTreeMap::new(),
    };
    super::visit_roots(&mut collect);
    collect.visit::<super::ErrorBody>();
    let decls = collect.decls;
    let mut modules = BTreeMap::<String, (BTreeMap<String, BTreeSet<String>>, String)>::new();
    for decl in decls.values() {
        let (imports, body) = modules.entry(decl.module.clone()).or_default();
        for dep in &decl.deps {
            let from = &decls[dep].module;
            if *from != decl.module {
                imports.entry(from.clone()).or_default().insert(dep.clone());
            }
        }
        body.push_str("\nexport ");
        body.push_str(&decl.text);
        body.push('\n');
    }
    let mut route_maps = Vec::new();
    for routes in MODULES {
        let module = module_name(routes.module);
        let (imports, body) = modules.entry(module.clone()).or_default();
        let mut name = |wire: &WireRef| {
            let name = wire(&cfg);
            let from = &decls
                .get(&name)
                .unwrap_or_else(|| panic!("`{name}` is not in wire_types!"))
                .module;
            if *from != module {
                imports
                    .entry(from.clone())
                    .or_default()
                    .insert(name.clone());
            }
            name
        };
        let members = routes
            .table
            .iter()
            .map(|route| route_member(route, &mut name))
            .collect::<String>();
        let map = route_map_name(&module);
        body.push_str(&format!("\nexport type {map} = {{\n{members}}};\n"));
        route_maps.push((module, map));
    }
    let mut files = BTreeMap::new();
    let mut index = format!("{HEADER}\n");
    for (module, (imports, body)) in modules {
        let mut out = HEADER.to_string();
        if !imports.is_empty() {
            out.push('\n');
        }
        for (from, names) in imports {
            let names = names.into_iter().collect::<Vec<_>>().join(", ");
            out.push_str(&format!(
                "import type {{ {names} }} from \"./{from}.gen.ts\"\n"
            ));
        }
        out.push_str(&body);
        index.push_str(&format!("export type * from \"./wire/{module}.gen.ts\"\n"));
        files.insert(format!("{GENERATED_DIR}/{module}.gen.ts"), out);
    }
    index.push('\n');
    for (module, map) in &route_maps {
        index.push_str(&format!(
            "import type {{ {map} }} from \"./wire/{module}.gen.ts\"\n"
        ));
    }
    let maps = route_maps
        .iter()
        .map(|(_, map)| format!("\n  & {map}"))
        .collect::<String>();
    index.push_str("\n/** Every route the native app serves, by method and path. */\n");
    index.push_str(&format!("export type Routes ={maps};\n"));
    files.insert(GENERATED.to_string(), index);
    files
}

/// The generated files on disk, with the line endings a Windows checkout
/// may give them normalized.
fn committed(root: &Path) -> BTreeMap<String, String> {
    let names = std::fs::read_dir(root.join(GENERATED_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with(".gen.ts"))
        .map(|name| format!("{GENERATED_DIR}/{name}"));
    std::iter::once(GENERATED.to_string())
        .chain(names)
        .filter_map(|path| {
            let text = std::fs::read_to_string(root.join(&path)).ok()?;
            Some((path, text.replace("\r\n", "\n")))
        })
        .collect()
}

fn first_difference(rendered: &str, committed: &str) -> String {
    rendered
        .lines()
        .chain(std::iter::repeat("<missing>"))
        .zip(committed.lines().chain(std::iter::repeat("<missing>")))
        .take(rendered.lines().count().max(committed.lines().count()))
        .find(|(new, old)| new != old)
        .map_or_else(String::new, |(new, old)| {
            format!("\n  committed: {old}\n  rust now:  {new}")
        })
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "regenerates committed source files on request; git holds the durable copy"
)]
fn generated_typescript_is_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rendered = render();
    let committed = committed(root);
    if rendered == committed {
        return;
    }
    if std::env::var("MEDIAFLICK_WRITE_WIRE_TYPES").as_deref() == Ok("1") {
        std::fs::create_dir_all(root.join(GENERATED_DIR)).expect("create wire dir");
        for (path, text) in &rendered {
            if committed.get(path) != Some(text) {
                std::fs::write(root.join(path), text).expect("write generated TypeScript");
                println!("rewrote {path}");
            }
        }
        for path in committed
            .keys()
            .filter(|path| !rendered.contains_key(*path))
        {
            std::fs::remove_file(root.join(path)).expect("remove stale TypeScript");
            println!("removed {path}");
        }
        return;
    }
    let problem = rendered
        .iter()
        .find_map(|(path, new)| match committed.get(path) {
            None => Some(format!("{path} is missing")),
            Some(old) if old != new => Some(format!(
                "{path} does not match{}",
                first_difference(new, old)
            )),
            Some(_) => None,
        })
        .or_else(|| {
            committed
                .keys()
                .find(|path| !rendered.contains_key(*path))
                .map(|path| format!("{path} is no longer generated"))
        })
        .unwrap_or_default();
    panic!(
        "the generated TypeScript does not match the Rust wire types. Run `just wire-types` and commit the result.\n  {problem}"
    );
}

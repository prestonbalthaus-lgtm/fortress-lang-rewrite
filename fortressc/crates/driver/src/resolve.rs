//! API resolution: finding the `.fsi` files a component imports, and putting
//! their declarations in scope before anything is checked.
//!
//! THREE FACTS SHAPE ALL OF IT, and none of them is a preference.
//!
//! API-FIRST IS ARCHITECTURAL. Over the `.fsi` files the dependency graph is
//! acyclic; over the `.fss` files the same computation strands File,
//! FlatString, FortressLibrary, Reader, Stream, String and Writer in ONE
//! seven-file mutual cycle. `Specification/basic/components/overview.tex:26-33`
//! says why: components never refer to other components directly, and every
//! external reference is to an api. A resolver that starts from `.fss`
//! deadlocks immediately.
//!
//! SOURCE-PATH ORDER IS LOAD-BEARING INPUT. Ten api names exist in more than
//! one directory -- Executable, File, FileSupport, FlatString, FortressLibrary,
//! List, Map, Pairs, Set, System -- and they are DIFFERENT LIBRARIES, not
//! copies. The legacy answer is on disk and is reused verbatim.
//!
//! THE PHASE ORDER IS INHERITED FROM M3d. `registry.concrete` and every type
//! tag freeze in `Checker::new`, and `mono::expand` runs to a fixpoint before
//! it, so seeded names have to be in the component BEFORE `check` is called.
//! Merging into the AST rather than into the registry is what gets that for
//! free: `check` sees one component and neither the checker nor codegen learns
//! that an import exists.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fortress_ast::{Component, Decl, ImportDecl};

/// `default_repository/configuration:44`:
/// `fortress.source.path=;.;${_fr}/LibraryBuiltin;${FORTRESS_AUTOHOME}/Library;${_fr}/test_library`
/// with `_fr = ${FORTRESS_AUTOHOME}/ProjectFortress` (:23). `.` is the
/// importing file's own directory, and it is FIRST -- which is what lets a test
/// directory shadow a library api.
const REPOSITORY_PATH: [&str; 3] = [
    "ProjectFortress/LibraryBuiltin",
    "Library",
    "ProjectFortress/test_library",
];

/// Loads the api a component EXPORTS. Imports and exports read the same source
/// path and the same files; what differs is the obligation -- an import puts
/// names in scope, an export is a promise the component has to keep.
pub fn find_api(name: &str, source: &Path) -> Option<Component> {
    let file = find(name, &source_path(source))?;
    let text = std::fs::read_to_string(file).ok()?;
    let tokens = fortress_lexer::lex(&text).ok()?;
    fortress_parser::parse(&tokens).ok()
}

/// What resolution found, for the diagnostic the driver prints.
pub struct Resolution {
    pub component: Component,
    /// Api names that were found and merged, in load order.
    pub loaded: Vec<String>,
    /// Api names no file on the source path provides.
    pub missing: Vec<String>,
    /// Api names whose file was found and did not parse. An api this compiler
    /// cannot read yet is not the importing component's fault, so it is
    /// reported and skipped rather than being made fatal.
    pub unreadable: Vec<String>,
}

/// The repository root: the nearest ancestor of `source` that holds both
/// `Library` and `ProjectFortress`. `FORTRESS_AUTOHOME` overrides it, which is
/// the same variable the legacy configuration reads.
fn autohome(source: &Path) -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("FORTRESS_AUTOHOME") {
        return Some(PathBuf::from(home));
    }
    let mut dir = source.parent()?.to_path_buf();
    loop {
        if dir.join("Library").is_dir() && dir.join("ProjectFortress").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn source_path(source: &Path) -> Vec<PathBuf> {
    let mut path: Vec<PathBuf> = Vec::new();
    if let Some(own) = source.parent() {
        path.push(own.to_path_buf());
    }
    if let Some(home) = autohome(source) {
        path.extend(REPOSITORY_PATH.iter().map(|d| home.join(d)));
    }
    path
}

/// `import Compiled5.a.{...}` names an api whose file is `Compiled5.a.fsi`: the
/// corpus writes a dotted api name as a dotted FILE name, not as directories.
/// `import FlatString.FlatString` is the api `FlatString` and one name in it,
/// and only the file system can say which -- so both readings are tried, api
/// name first.
fn candidates(import: &ImportDecl) -> Vec<String> {
    let mut names = vec![import.api_name.clone()];
    if let Some((head, _)) = import.api_name.rsplit_once('.') {
        names.push(head.to_owned());
    }
    names.retain(|n| !n.is_empty());
    names
}

fn find(name: &str, path: &[PathBuf]) -> Option<PathBuf> {
    let file = format!("{name}.fsi");
    path.iter().map(|dir| dir.join(&file)).find(|p| p.is_file())
}

fn decl_name(decl: &Decl) -> &str {
    match decl {
        Decl::Function(f) => &f.name,
        Decl::Trait(t) => &t.name,
        Decl::Object(o) => &o.name,
    }
}

/// Resolves `component`'s imports transitively and returns it with the api
/// declarations prepended.
///
/// BEST EFFORT BY DESIGN, and the reason is measurable rather than tasteful: of
/// the 68 top-level `.fsi` files in the library set, 41 do not yet parse. Making
/// an unreadable api fatal would take every importing component with it and
/// measure the parser rather than the resolver. What is reported instead is
/// which apis were skipped and why, and the driver prints it.
pub fn resolve(component: &Component, source: &Path) -> Resolution {
    let path = source_path(source);
    let mut loaded = Vec::new();
    let mut missing = Vec::new();
    let mut unreadable = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // Declarations the component itself makes always win: an api gives a
    // SIGNATURE and the component gives the definition, and `source-code.tex`
    // makes satisfying the api the component's obligation rather than the
    // resolver's.
    let mut taken: HashSet<String> = component
        .decls
        .iter()
        .map(|d| decl_name(d).to_owned())
        .collect();
    let mut merged: Vec<Decl> = Vec::new();

    let mut queue: Vec<ImportDecl> = component.imports.clone();
    let mut sources: HashMap<String, PathBuf> = HashMap::new();
    while let Some(import) = queue.pop() {
        let Some((name, file)) = candidates(&import)
            .into_iter()
            .find_map(|n| find(&n, &path).map(|f| (n, f)))
        else {
            let named = import.api_name.clone();
            if !named.is_empty() && !missing.contains(&named) {
                missing.push(named);
            }
            continue;
        };
        if !seen.insert(name.clone()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            unreadable.push(name);
            continue;
        };
        let parsed = fortress_lexer::lex(&text)
            .ok()
            .and_then(|tokens| fortress_parser::parse(&tokens).ok());
        let Some(api) = parsed else {
            unreadable.push(name);
            continue;
        };
        // An api's own imports are resolved too: `FortressLibrary.fsi` imports
        // `NativeArray` and `NatReflect`, which live under `LibraryBuiltin` and
        // are not in the census set at all.
        queue.extend(api.imports.iter().cloned());
        sources.insert(name.clone(), file);
        // ONLY THE TYPES. An api's FUNCTION declarations are signatures the
        // importing component must SATISFY -- `source-code.tex:313-320` makes
        // that the component's obligation and it is step 5, not this step --
        // and merging them into a `.fss` makes the checker demand a body for
        // every one. Its TRAITS and OBJECTS are what a use site refers to by
        // name, and they are what `unknown type` is asking for.
        for decl in api.decls {
            if matches!(decl, Decl::Function(_)) {
                continue;
            }
            if taken.insert(decl_name(&decl).to_owned()) {
                merged.push(decl);
            }
        }
        loaded.push(name);
    }

    let mut component = component.clone();
    merged.append(&mut component.decls);
    component.decls = merged;
    Resolution {
        component,
        loaded,
        missing,
        unreadable,
    }
}

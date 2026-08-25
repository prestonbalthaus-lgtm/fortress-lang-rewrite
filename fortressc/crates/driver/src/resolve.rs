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

use fortress_ast::{Component, Decl, ImportDecl, ImportItems, Member, Span, StaticParam, TypeRef};
use fortress_types::BUILTIN_TYPE_NAMES;

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

/// `Specification-1.0-frozen/library/structure.tex:16-18`: the default
/// libraries "are automatically imported by every Fortress component and API".
/// `CompilerBuiltin` is the one this compiler's own bootstrap needs -- it is
/// what declares `RR32`, `RR64` and the numeric traits `FortressLibrary.fsi`
/// stops on at :362 without ever writing an import for them.
const IMPLICITLY_IMPORTED: [&str; 2] = ["CompilerBuiltin", "FortressLibrary"];

/// BOTH HALVES. THIS RAN FOR apis ONLY UNTIL LINK 5 (`bd76d11e3`, 2026-08-23)
/// AND IT HAS RUN FOR COMPONENTS EVER SINCE. The comment here kept saying "the
/// component half is ARCHITECTURALLY OUT" for two days after that, above an
/// early return that had been dead since the same commit, and the false claim
/// propagated out of this file into `02-stack.md`, `04-state.md` and
/// `ROADMAP.md`, where a milestone that was already built stayed on the queue
/// as "the largest remaining lever". Corrected 2026-08-25.
///
/// THE OLD REASON WAS TRUE AND WAS NOT THE BLOCKER, which is why it held for so
/// long. A merged OBJECT does take a 32-bit type tag and a merged SINGLETON is
/// constructed in `main` -- and a trait-only variant built to dodge exactly
/// that lost the same 402 files. What actually stopped it was a NAME COLLISION
/// WITH THE BUILTIN SCALARS. The four rules that made it land are in
/// `bd76d11e3`'s message and each is measured against what happens without it:
/// a merged declaration LOSES to a builtin of its own name; a merged trait's
/// supertype edge to a builtin is DROPPED; a merged functional method is NOT
/// lifted into a component; and a merged object is lowered ONLY ON DEMAND,
/// never if it is a singleton, never without a buildable layout.
///
/// WHAT HOLDS IT: `fortressc/tests/implicitbuiltin.fss` is COMPILED AND RUN by
/// `apply-gate.sh`, and it carries a literal, a `String` and a `||` precisely
/// because the whole risk of the component half is a merged declaration
/// shadowing a builtin of the same name.
///
/// NOT INTO THE BUILTIN ITSELF, and not into anything it reaches: only the
/// TOP-LEVEL file gets the implicit import, because an api pulled in through
/// the queue is resolved with the imports it WRITES. `CompilerBuiltin` imports
/// `AnyType` and `CompilerAlgebra`, and injecting the reverse edge would make
/// the graph the api-first design exists to keep acyclic.
///
/// LAST IN THE QUEUE, WHICH IS `insert(0, ..)`: the loop POPS, so index zero is
/// reached last and an explicitly written import claims a contested name first.
///
/// ONE GUARD, ON ONE LINE, CARRYING NO VERTICAL BAR: a mutation row splits on
/// `IFS='|'`, so `||` cannot appear in a line a table has to reach. The second
/// guard was the api-only early return; Link 5 neutered it to `if false` rather
/// than deleting it, and the dead block came out on 2026-08-25 with the comment
/// above. Nothing targets it -- `apply-gate.sh`'s row for this function matches
/// `if component.name == name {`.
fn implicit_import(component: &Component, queue: &mut Vec<ImportDecl>) {
    for name in IMPLICITLY_IMPORTED {
        // `break` AND NOT `continue`, AND THAT ONE WORD IS THE LAYERING. The
        // core apis are ordered here, and a core api implicitly imports the
        // ones BELOW it and no more: `CompilerBuiltin` is the root and takes
        // nothing, `FortressLibrary` takes the builtin (which is how `RR32`
        // resolves in it without a written import), and every other file takes
        // both. `continue` would hand the builtin an implicit import of
        // `FortressLibrary` -- the REVERSE EDGE the api-first design exists to
        // keep out, and apply-gate's "the builtin does not implicitly import
        // itself" row caught it the first time it was written that way.
        if component.name == name {
            break;
        }
        queue.insert(
            0,
            ImportDecl {
                api_name: name.to_owned(),
                is_api: true,
                items: ImportItems::OnDemand,
                except: Vec::new(),
                span: Span::new(0, 0),
            },
        );
    }
}

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
    /// How many declarations were PREPENDED. `component.decls[..merged]` came
    /// out of another file and their spans index THAT file's text, so anything
    /// reporting a position has to know where the component's own start.
    pub merged: usize,
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

/// Every type name a declaration mentions: its topology clauses and the types
/// of its members. A merged declaration has to be WELL FORMED, and a trait
/// whose supertype was left behind is `unknown type` at the use site -- so a
/// named import brings what it named PLUS what those declarations need from the
/// same api, to a fixpoint.
fn references(decl: &Decl, out: &mut Vec<String>) {
    fn walk(t: &TypeRef, out: &mut Vec<String>) {
        match t {
            TypeRef::Named { name, args, .. } => {
                out.push(name.clone());
                for a in args {
                    walk(a, out);
                }
            }
            TypeRef::Tuple { elems, .. } => elems.iter().for_each(|e| walk(e, out)),
            TypeRef::Arrow { from, to, .. } => {
                walk(from, out);
                walk(to, out);
            }
            // The ELEMENT names a type an import must bring; an extent is a
            // static argument and names none.
            TypeRef::Shaped { base, .. } => walk(base, out),
            TypeRef::Unit { .. } | TypeRef::Static { .. } => {}
        }
    }
    // SUPERTYPES ONLY, and the narrowing is measured rather than tasteful. A
    // trait's supertype is part of its IDENTITY -- subtyping cannot be decided
    // without it, so merging a trait and leaving its supertype behind merges
    // something ill formed. A METHOD'S parameter type is not: it is needed when
    // that method is CALLED, and if it is called the name is demanded and
    // reported honestly.
    //
    // FOLLOWING MEMBER TYPES TOO WAS TRIED AND IS WIDER THAN THE DEFECT IT
    // FIXES: `import FortressLibrary.{println, String}` reaches `Reduction`
    // through String's members and blows MAX_INSTANTIATIONS again -- a
    // different name from the `Indexed` the old merge-everything hit, and the
    // same failure. Neither `comprises` nor `excludes` is followed either:
    // both name types that are BELOW or BESIDE this one, never above it.
    let topology = match decl {
        Decl::Trait(t) => &t.extends,
        Decl::Object(o) => &o.extends,
        // Neither declares a supertype, so there is no topology to walk.
        Decl::Function(_) | Decl::Value(_) => return,
    };
    for t in topology {
        walk(t, out);
    }
}

/// The named set closed over `references`, restricted to what THIS api
/// declares. A name the api does not declare is somebody else's to provide and
/// is left alone.
fn closure(decls: &[Decl], seed: Vec<String>) -> HashSet<String> {
    let here: HashMap<&str, &Decl> = decls.iter().map(|d| (decl_name(d), d)).collect();
    let mut want: HashSet<String> = seed.into_iter().collect();
    let mut work: Vec<String> = want.iter().cloned().collect();
    while let Some(name) = work.pop() {
        let Some(decl) = here.get(name.as_str()) else {
            continue;
        };
        let mut found = Vec::new();
        references(decl, &mut found);
        for r in found {
            if here.contains_key(r.as_str()) && want.insert(r.clone()) {
                work.push(r);
            }
        }
    }
    want
}

/// Everything the resolver merges is marked, api-side and component-side
/// alike. Nothing reads it on the api side yet -- an api is never lowered --
/// but the fact is the same fact and recording it in one place is what keeps
/// the two from drifting.
fn mark_merged(decl: &mut Decl) {
    match decl {
        Decl::Trait(t) => t.merged = true,
        Decl::Object(o) => o.merged = true,
        Decl::Function(_) | Decl::Value(_) => {}
    }
}

/// A merged declaration's `extends` edge to a BUILTIN type name, dropped.
///
/// Component-side, a builtin-named declaration is skipped -- `trait String` in
/// `CompilerBuiltin.fsi` IS `Type::String`, and merging it would shadow the
/// builtin so that every literal in the importing component stopped typing.
/// `CompilerBuiltin.fsi:51` then writes `trait JavaString extends String`, and
/// the edge points at a name that is no longer a trait.
///
/// THE EDGE IS DROPPED RATHER THAN THE TRAIT REFUSED, because the edge could
/// never have been honoured here anyway: a scalar has no trait representation
/// in this backend -- that is the boxing decision, not a gap -- so nothing was
/// ever going to be a subtype of `String` by inheritance. Dropping it narrows
/// what typechecks and cannot make anything type that should not.
fn drop_builtin_supertypes(decl: &mut Decl) {
    let extends = match decl {
        Decl::Trait(t) => &mut t.extends,
        Decl::Object(o) => &mut o.extends,
        Decl::Function(_) | Decl::Value(_) => return,
    };
    extends.retain(|t| !matches!(t, TypeRef::Named { name, .. } if BUILTIN_TYPE_NAMES.contains(&name.as_str())));
}

/// How many static parameters a declaration takes, for a declaration that can
/// take any. A function or a value has none to compare, and `None` says so
/// rather than pretending it has zero.
fn static_arity(decl: &Decl) -> Option<usize> {
    match decl {
        Decl::Trait(t) => Some(t.static_params.len()),
        Decl::Object(o) => Some(o.static_params.len()),
        Decl::Function(_) | Decl::Value(_) => None,
    }
}

/// The private name a merged declaration keeps its identity under when it loses
/// a collision it cannot legitimately lose. UNWRITABLE, the same way
/// `SELF_TYPE_PLACEHOLDER` is: `$` lexes as an operator character and never as
/// part of an identifier, so no source file can name one of these and no
/// source file can be shadowed by one. LEADING, because `mangle_static` builds
/// `Name$Arg$e` -- a name that starts with `$` is one this function made.
fn scoped_name(api: &str, name: &str) -> String {
    format!("${api}${name}")
}

fn set_decl_name(decl: &mut Decl, to: String) {
    match decl {
        Decl::Trait(t) => t.name = to,
        Decl::Object(o) => o.name = to,
        Decl::Function(f) => f.name = to,
        Decl::Value(v) => v.name = to,
    }
}

/// A STATIC PARAMETER'S NAME SHADOWS A TYPE OF THAT NAME, and the rewrite has to
/// honour it or it CAPTURES. `rename_in_static_params` rewrites a parameter's
/// BOUNDS and never the parameter's own name, so without `bound` the two come
/// apart: `[\Comprehension\]` keeps its name while every use of it in a
/// signature becomes `$FortressLibrary$Comprehension`, `mono`'s `Subst` is keyed
/// by `param.name`, and the reference stops binding to the static argument and
/// silently resolves to the renamed trait instead.
///
/// THIS PROJECT HAS BEEN BURNED BY THIS EXACT SHAPE TWICE. `comprises.rs`
/// carries `Row::statics` and `is_own_static` because
/// `Library/CompilerAlgebra.fsi:24` writes `trait Equality[\T\] comprises T`
/// while `test_library/Compiled3.f.fsi` declares a real `trait T`; and
/// `SELF_TYPE_PLACEHOLDER` became `$Self` for the mirror image. Measured at ZERO
/// live instances -- the contested names are `ReductionWithZeroes`,
/// `BigOperator`, `BigReduction`, `Comprehension` and `List`, and the static
/// parameters in those files are `R`, `L`, `I`, `O`, `E` and `F` -- so this is a
/// guard against a shape, not a fix for a symptom.
fn rename_in_type(t: &mut TypeRef, map: &HashMap<String, String>, bound: &HashSet<String>) {
    match t {
        TypeRef::Named { name, args, .. } => {
            if !bound.contains(name.as_str()) {
                if let Some(to) = map.get(name.as_str()) {
                    name.clone_from(to);
                }
            }
            for a in args {
                rename_in_type(a, map, bound);
            }
        }
        TypeRef::Tuple { elems, .. } => {
            elems.iter_mut().for_each(|e| rename_in_type(e, map, bound));
        }
        TypeRef::Arrow { from, to, .. } => {
            rename_in_type(from, map, bound);
            rename_in_type(to, map, bound);
        }
        // THE ELEMENT ONLY, AND THE EXTENT DELIBERATELY NOT, which is the same
        // cut `references` makes ten lines up and for the same reason: an
        // extent is a static ARGUMENT and names no type. A name written there
        // is a `nat` parameter or a literal, and rewriting one would point a
        // VALUE at a trait.
        TypeRef::Shaped { base, .. } => rename_in_type(base, map, bound),
        TypeRef::Unit { .. } | TypeRef::Static { .. } => {}
    }
}

/// The parameters' own names are already in `bound` when this is called: an
/// F-bound (`[\T extends Comparable[\T\]\]`) names the parameter inside its
/// own bound, so the bounds are read in the scope the parameters create.
fn rename_in_static_params(
    params: &mut [StaticParam],
    map: &HashMap<String, String>,
    bound: &HashSet<String>,
) {
    for p in params {
        for b in &mut p.bounds {
            rename_in_type(b, map, bound);
        }
    }
}

fn static_param_names(params: &[StaticParam]) -> impl Iterator<Item = String> + '_ {
    params.iter().map(|p| p.name.clone())
}

fn rename_in_members(
    members: &mut [Member],
    map: &HashMap<String, String>,
    bound: &HashSet<String>,
) {
    for m in members {
        match m {
            Member::Field(f) => rename_in_type(&mut f.ty, map, bound),
            Member::Method(md) => {
                // A METHOD OPENS ITS OWN SCOPE. `map[\G3\](f: E2->G3)` binds
                // `G3` for the whole signature and for nothing outside it.
                let mut inner = bound.clone();
                inner.extend(static_param_names(&md.static_params));
                rename_in_static_params(&mut md.static_params, map, &inner);
                for p in &mut md.params {
                    rename_in_type(&mut p.ty, map, &inner);
                }
                if let Some(r) = &mut md.return_type {
                    rename_in_type(r, map, &inner);
                }
            }
            Member::Coercion { from, .. } => {
                from.iter_mut().for_each(|t| rename_in_type(t, map, bound));
            }
        }
    }
}

/// Every TYPE POSITION a merged trait or object has, rewritten.
///
/// BODIES ARE NOT WALKED, AND `scopeable` IS WHAT EARNS THAT. Nine `Expr`
/// variants can carry a `TypeRef` -- `ObjectExpr`, `Comprehension`, `Try`,
/// `Instantiate`, `Lambda`, `TypeCaseArm`, `Binding` and friends -- so walking
/// type positions only is complete EXACTLY WHEN there are no bodies, and that
/// is now a checked precondition rather than a property of `.fsi` files that
/// nothing enforces. Every type position IS here.
fn rename_types(decl: &mut Decl, map: &HashMap<String, String>) {
    let (static_params, extends, comprises, excludes, members) = match decl {
        Decl::Trait(t) => (
            &mut t.static_params,
            &mut t.extends,
            &mut t.comprises,
            &mut t.excludes,
            &mut t.members,
        ),
        Decl::Object(o) => (
            &mut o.static_params,
            &mut o.extends,
            &mut o.comprises,
            &mut o.excludes,
            &mut o.members,
        ),
        Decl::Function(_) | Decl::Value(_) => return,
    };
    let bound: HashSet<String> = static_param_names(static_params).collect();
    rename_in_static_params(static_params, map, &bound);
    for t in extends.iter_mut().chain(comprises).chain(excludes) {
        rename_in_type(t, map, &bound);
    }
    rename_in_members(members, map, &bound);
    // AN OBJECT'S VALUE PARAMETERS ARE ITS FIELDS, so they are read in the
    // scope its static parameters create, exactly like its members.
    if let Decl::Object(o) = decl {
        for p in o.params.iter_mut().flatten() {
            rename_in_type(&mut p.ty, map, &bound);
        }
    }
}

/// Does this declaration carry a MEMBER BODY or a field INITIALIZER?
///
/// `rename_types` walks type positions and not bodies, and the reason it is
/// allowed to is that an api DECLARES. THAT PREMISE IS NOT ENFORCED BY THE
/// PARSER: `member()` takes no signature-only flag -- that reaches `fn_decl`
/// and `opr_decl`, top-level functions -- so a member method reads a body and a
/// field reads an initializer identically in an `api` and a `component`. The
/// only enforcement is checker-side (`error.rs`, "an `api` is a set of
/// declarations"), and AN IMPORTED api IS PARSED AND MERGED AND NEVER CHECKED.
/// `ProjectFortress/parser_tests/XXXDefinitions.fsi:19` is the existence proof:
/// `api XXXDefinitions` with `m(): () = ()` inside a trait.
///
/// So the precondition is CHECKED rather than assumed, and it is checked over
/// the WHOLE api rather than the one declaration: a rename rewrites every
/// declaration that api contributed, so one body anywhere in it would make the
/// rewrite partial. An api with a body scopes nothing and keeps today's drop.
fn has_member_body(decl: &Decl) -> bool {
    let members = match decl {
        Decl::Trait(t) => &t.members,
        Decl::Object(o) => &o.members,
        Decl::Function(_) | Decl::Value(_) => return false,
    };
    members.iter().any(|m| match m {
        Member::Field(f) => f.init.is_some(),
        Member::Method(md) => md.body.is_some(),
        Member::Coercion { .. } => false,
    })
}

fn decl_name(decl: &Decl) -> &str {
    match decl {
        Decl::Function(f) => &f.name,
        Decl::Trait(t) => &t.name,
        Decl::Object(o) => &o.name,
        Decl::Value(v) => &v.name,
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
    // KEYED BY THE api NAME AND WHAT THE IMPORT ASKED FOR, not by the name
    // alone. `ComparisonLibrary.fsi` writes `import CompilerAlgebra.{Equality,
    // opr =}` and `CompilerBuiltin` writes `import CompilerAlgebra.{Equality,
    // StandardTotalOrder}`; with the name alone as the key the FIRST of those
    // decided, the second was silently dropped, and a merged builtin trait was
    // left extending a `StandardTotalOrder` nothing had brought in. Two
    // requests for the same api at different name sets are two requests, and
    // `taken` is what keeps the second from merging anything twice.
    //
    // IT STILL TERMINATES on a cycle: `RecA` importing `RecB` importing `RecA`
    // is the SAME pair both times.
    let mut seen: Vec<(String, ImportItems)> = Vec::new();
    // Declarations the component itself makes always win: an api gives a
    // SIGNATURE and the component gives the definition, and `source-code.tex`
    // makes satisfying the api the component's obligation rather than the
    // resolver's.
    let mut taken: HashSet<String> = component
        .decls
        .iter()
        .map(|d| decl_name(d).to_owned())
        .collect();
    // WHAT EACH TAKEN NAME IS SHAPED LIKE, which is what separates a collision
    // between two COPIES of one declaration from a collision between two
    // genuinely different types. See `scoped` below.
    let mut shapes: HashMap<String, Option<usize>> = component
        .decls
        .iter()
        .map(|d| (decl_name(d).to_owned(), static_arity(d)))
        .collect();
    let mut merged: Vec<Decl> = Vec::new();
    // Which api each merged declaration came out of, so a rename can follow
    // that api's own references and nobody else's.
    let mut origins: Vec<String> = Vec::new();
    // Per api, the names that api's declarations must now spell differently.
    let mut renames: HashMap<String, HashMap<String, String>> = HashMap::new();

    // WHAT EACH IMPORT ASKED FOR, carried alongside it. `ImportItems` is on the
    // declaration and was read by nothing -- so `import FortressLibrary.{
    // println, String}` pulled in EVERY trait and object the library declares.
    // That is not a tidiness point: it is what put `Indexed` into the
    // instantiation budget of a component that never named it, and
    // MAX_INSTANTIATIONS is what that component died on.
    let mut queue: Vec<ImportDecl> = component.imports.clone();
    implicit_import(component, &mut queue);
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
        let key = (name.clone(), import.items.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
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
        // WHAT THE IMPORT NAMED, or everything for an on-demand one.
        // `intro.tex:38-63` calls `.{...}` and `import api Foo` imports ON
        // DEMAND -- every name -- and a NAMED list is a request for those names
        // and no others. 841 corpus imports are on-demand and 142 are named, so
        // this narrows one import in seven and leaves the rest exactly as they
        // were.
        let from_api = api.is_api;
        // See `has_member_body`. Computed over the WHOLE api and before the
        // loop below moves `api.decls`.
        let scopeable = from_api && !api.decls.iter().any(has_member_body);
        let wanted: Option<HashSet<String>> = match &import.items {
            ImportItems::OnDemand => None,
            ImportItems::Named(names) => Some(closure(
                &api.decls,
                names.iter().map(|n| n.name.clone()).collect(),
            )),
        };
        // ONLY THE TYPES. An api's FUNCTION and VALUE declarations are
        // signatures the importing component must SATISFY --
        // `source-code.tex:313-320` makes that the component's obligation and
        // it is step 5, not this step -- and merging them into a `.fss` makes
        // the checker demand a body for every one. Its TRAITS and OBJECTS are
        // what a use site refers to by name, and they are what `unknown type`
        // is asking for.
        //
        // A VALUE IS ON THIS LIST BECAUSE IT USED TO BE A FUNCTION. It was
        // parsed as a nullary `FnDecl` and skipped by that arm; the moment it
        // became `Decl::Value` it started being merged, and
        // `Library/CompilerSystem.fsi:15` -- `args : StringVector`, with no
        // `StringVector` anywhere -- took SEVEN importing components down with
        // it, each reporting a name it never wrote.
        for decl in api.decls {
            if matches!(decl, Decl::Function(_) | Decl::Value(_)) {
                continue;
            }
            let mut decl = decl;
            // ONLY AN api's DECLARATIONS ARE MARKED. An api DECLARES and a
            // component DEFINES: `ProjectFortress/compiler_regressions/
            // object_from_diff_component.fss` imports a `.fss` and CONSTRUCTS
            // the object it finds there, and that works precisely because the
            // definition came with it. Marking that one would take its
            // constructor away and hand codegen `unknown function `O$new``.
            if from_api {
                mark_merged(&mut decl);
            }
            if from_api && !component.is_api {
                if BUILTIN_TYPE_NAMES.contains(&decl_name(&decl)) {
                    continue;
                }
                drop_builtin_supertypes(&mut decl);
            }
            if let Some(wanted) = wanted.as_ref() {
                if !wanted.contains(decl_name(&decl)) {
                    continue;
                }
            }
            let written = decl_name(&decl).to_owned();
            if taken.insert(written.clone()) {
                shapes.insert(written, static_arity(&decl));
                origins.push(name.clone());
                merged.push(decl);
                continue;
            }
            // A LOST COLLISION IS NORMALLY THE RIGHT ANSWER AND STAYS ONE. The
            // shipped libraries are LAYERED COPIES -- `CompilerBuiltin` and
            // `FortressLibrary` declare 25615 of the corpus's 25639 colliding
            // names at the SAME arity, and identifying them is what the
            // layering is for. What follows fires on the other 24.
            //
            // TWO DECLARATIONS TAKING A DIFFERENT NUMBER OF STATIC PARAMETERS
            // ARE NOT COPIES. No substitution makes `[\R\]` and `[\R,L\]`
            // one declaration, so the loser is a DIFFERENT TYPE that happens to
            // share a name, and dropping it silently re-points its own api's
            // references at something that cannot accept them:
            // `Library/GeneratorLibrary.fsi` declares `ReductionWithZeroes
            // [\R\]`, `FortressLibrary.fsi:1871` declares `[\R,L\]`, and
            // six of FortressLibrary's own objects that DID merge then read
            // `takes 1 static argument(s), found 2`.
            //
            // So it keeps its identity under an unwritable name, and every
            // reference from ITS OWN api follows it. Nobody else's does: an
            // importer's own declaration still wins for the importer's own
            // references, which is what makes this a narrowing rather than a
            // change of who wins.
            //
            // api-SIDE ONLY AND BODY-FREE, and that pair is what makes
            // `rename_types` complete rather than partial: every type reference
            // in a declaration with no bodies is in a TYPE POSITION, and every
            // type position is walked. `scopeable` is where that is CHECKED.
            let winner = shapes.get(&written).copied().flatten();
            let (Some(mine), Some(theirs)) = (static_arity(&decl), winner) else {
                continue;
            };
            // TWO `if`s AND NOT ONE, because a mutation row splits on
            // `IFS='|'` and cannot carry a `||`. The arity comparison is the
            // one a table has to be able to reach.
            if !scopeable {
                continue;
            }
            if mine == theirs {
                continue;
            }
            let private = scoped_name(&name, &written);
            // THE SECOND REQUEST FOR ONE api MERGES NOTHING TWICE. `seen` is
            // keyed by the api name AND what the import asked for, so an api
            // reached both on demand and by a named list is popped twice --
            // and the second pass recomputes the SAME private name. Without
            // this the list takes a second declaration under it, which is two
            // templates of one name in `mono`'s generics map.
            if !taken.insert(private.clone()) {
                continue;
            }
            set_decl_name(&mut decl, private.clone());
            renames
                .entry(name.clone())
                .or_default()
                .insert(written, private.clone());
            shapes.insert(private, Some(mine));
            origins.push(name.clone());
            merged.push(decl);
        }
        if !loaded.contains(&name) {
            loaded.push(name);
        }
    }

    // A POST-PASS, BECAUSE THE COLLISION IS FOUND AFTER THE REFERENCES ARE
    // MERGED. An api's declarations arrive in source order, and
    // `FortressLibrary.fsi` writes the six objects that read
    // `ReductionWithZeroes[\_,_\]` at :78 and :2021 -- both sides of the
    // declaration itself at :1871.
    for (decl, origin) in merged.iter_mut().zip(&origins) {
        if let Some(map) = renames.get(origin) {
            rename_types(decl, map);
        }
    }

    let mut component = component.clone();
    let count = merged.len();
    merged.append(&mut component.decls);
    component.decls = merged;
    Resolution {
        component,
        loaded,
        missing,
        unreadable,
        merged: count,
    }
}

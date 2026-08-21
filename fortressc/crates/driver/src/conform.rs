//! Component-satisfies-api: `source-code.tex:313-320`.
//!
//! > A component must satisfy every top-level declaration in any api it
//! > exports.
//!
//! Until this existed the driver took ONE file, `Component::exports` had no
//! readers, and nothing had ever compared a `.fss` to a `.fsi`. `export
//! Executable` was a token the parser stored and no one asked about, which is
//! why a component could export an api it did not implement -- and why
//! `XXXcom.sun.test8.fss`, whose whole point is a component name that does not
//! match, compiles.
//!
//! THE COMPARISON IS AT `TypeRef` LEVEL, deliberately, and that is a stated
//! limitation rather than an oversight. Two names are the same declaration when
//! they are spelled the same and shaped the same; whether `List.List` and
//! `List` denote one type is a RESOLUTION question, and resolution across
//! qualified names is the rest of phase 3. So `f(x: ZZ32)` in the api and
//! `f(x: ZZ32)` in the component match, and nothing here pretends to know that
//! `f(x: List)` and `f(x: PureList.List)` might.

use fortress_ast::{Component, Decl, Modifiers, Param, TypeRef};

/// One way a component fails its api.
pub struct Violation {
    pub api: String,
    pub what: String,
}

impl core::fmt::Display for Violation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "the exported api `{}` requires {}", self.api, self.what)
    }
}

/// Spans differ between two files by construction, and `TypeRef` derives
/// `PartialEq` over them, so structural equality has to be written out.
fn same_type(a: &TypeRef, b: &TypeRef) -> bool {
    match (a, b) {
        (
            TypeRef::Named {
                name: an, args: aa, ..
            },
            TypeRef::Named {
                name: bn, args: ba, ..
            },
        ) => an == bn && aa.len() == ba.len() && aa.iter().zip(ba).all(|(x, y)| same_type(x, y)),
        (TypeRef::Unit { .. }, TypeRef::Unit { .. }) => true,
        (TypeRef::Tuple { elems: ae, .. }, TypeRef::Tuple { elems: be, .. }) => {
            ae.len() == be.len() && ae.iter().zip(be).all(|(x, y)| same_type(x, y))
        }
        (
            TypeRef::Arrow {
                from: af, to: at, ..
            },
            TypeRef::Arrow {
                from: bf, to: bt, ..
            },
        ) => same_type(af, bf) && same_type(at, bt),
        _ => false,
    }
}

/// The varargs flag is part of the shape: `run()` and `run(args: String...)`
/// are different declarations, and conflating them is how a conformance check
/// passes something that will not link.
fn same_params(a: &[Param], b: &[Param]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.varargs == y.varargs && same_type(&x.ty, &y.ty))
}

fn same_return(a: Option<&TypeRef>, b: Option<&TypeRef>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => same_type(x, y),
        (None, None) => true,
        // An api always writes the return type and an inferred one in the
        // component is not a mismatch to report from here -- the checker has
        // already resolved it, and this pass reads the AST.
        _ => true,
    }
}

fn describe(params: &[Param]) -> String {
    let mut out = String::from("(");
    for (index, p) in params.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&p.name);
        if p.varargs {
            out.push_str("...");
        }
    }
    out.push(')');
    out
}

/// `FnSig = SimpleName w ":" w NoNewlineType` (`Function.rats:18`). An api may
/// declare a function as a NAME OF ARROW TYPE -- `foo: String -> ()` -- and it
/// means the same thing as `foo(x: String): ()`. Fifteen corpus apis write it,
/// and without this the shapes never match and every one of them reports a
/// violation that is not there.
///
/// A tuple on the left is the parameter list: `f: (ZZ32, String) -> ()` is two
/// parameters, not one tuple, because this subset has no tuple values.
fn as_signature(decl: &fortress_ast::FnDecl) -> Option<(Vec<TypeRef>, Option<TypeRef>)> {
    if !decl.value_binding {
        return Some((
            decl.params.iter().map(|p| p.ty.clone()).collect(),
            decl.return_type.clone(),
        ));
    }
    match decl.return_type.as_ref()? {
        TypeRef::Arrow { from, to, .. } => {
            let params = match from.as_ref() {
                TypeRef::Tuple { elems, .. } => elems.clone(),
                TypeRef::Unit { .. } => Vec::new(),
                other => vec![other.clone()],
            };
            Some((params, Some(to.as_ref().clone())))
        }
        // A genuine value declaration, not a function in disguise.
        _ => None,
    }
}

fn same_shape(have: &fortress_ast::FnDecl, want: &fortress_ast::FnDecl) -> bool {
    let (Some((hp, hr)), Some((wp, wr))) = (as_signature(have), as_signature(want)) else {
        return false;
    };
    hp.len() == wp.len()
        && hp.iter().zip(&wp).all(|(x, y)| same_type(x, y))
        && same_return(hr.as_ref(), wr.as_ref())
        // The varargs flag is only ever set on a written parameter list, so it
        // is compared where both sides have one.
        && (have.value_binding || want.value_binding || same_params(&have.params, &want.params))
}

/// `private` declarations do not participate (`source-code.tex:313-320`).
const fn participates(m: Modifiers) -> bool {
    !m.private
}

/// Every top-level declaration of `api` that `component` does not satisfy.
pub fn violations(component: &Component, api: &Component, api_name: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    let say = |what: String| Violation {
        api: api_name.to_owned(),
        what,
    };

    for decl in &api.decls {
        match decl {
            Decl::Function(want) => {
                if !participates(want.modifiers) {
                    continue;
                }
                let found: Vec<&_> = component
                    .decls
                    .iter()
                    .filter_map(|d| match d {
                        Decl::Function(f) if f.name == want.name => Some(f),
                        _ => None,
                    })
                    .collect();
                if found.is_empty() {
                    out.push(say(format!("`{}`, which is not declared", want.name)));
                    continue;
                }
                // An overload set satisfies the api if ONE of its members has
                // the declared shape. `overloading.tex` lets a component
                // declare more than the api asks for.
                if !found.iter().any(|f| same_shape(f, want)) {
                    out.push(say(format!(
                        "`{}{}`, and no declaration of that name has those parameters",
                        want.name,
                        as_signature(want).map_or_else(
                            || describe(&want.params),
                            |(params, _)| format!("/{}", params.len())
                        )
                    )));
                }
            }
            Decl::Trait(want) => {
                let found = component.decls.iter().find_map(|d| match d {
                    Decl::Trait(t) if t.name == want.name => Some(t),
                    _ => None,
                });
                let Some(have) = found else {
                    out.push(say(format!("trait `{}`, which is not declared", want.name)));
                    continue;
                };
                // "IDENTICAL", not "compatible": `source-code.tex:290-299`
                // makes a trait's exported hierarchy a fact about the api, so a
                // component may not widen or narrow it. ALL THREE TOPOLOGY
                // CLAUSES are the hierarchy, not just `extends` -- an api that
                // declares `trait T excludes S` and a component that declares
                // `trait T` describe different lattices, and the legacy says so
                // by name ("due to different excludes clauses for traits").
                for (clause, mine, theirs) in [
                    ("extend", &have.extends, &want.extends),
                    ("comprise", &have.comprises, &want.comprises),
                    ("exclude", &have.excludes, &want.excludes),
                ] {
                    if mine.len() != theirs.len()
                        || !mine.iter().zip(theirs).all(|(x, y)| same_type(x, y))
                    {
                        out.push(say(format!(
                            "trait `{}` to {clause} exactly what the api declares",
                            want.name
                        )));
                    }
                }
                // The open marker is part of the clause and not decoration: an
                // api's `comprises { ... }` says the set is open, and a
                // component that closes it has narrowed the exported hierarchy.
                if have.comprises_open != want.comprises_open {
                    out.push(say(format!(
                        "trait `{}` to declare the same OPEN (`...`) `comprises` clause \
                         as the api",
                        want.name
                    )));
                }
            }
            Decl::Object(want) => {
                if !component.decls.iter().any(|d| match d {
                    Decl::Object(o) => o.name == want.name,
                    _ => false,
                }) {
                    out.push(say(format!(
                        "object `{}`, which is not declared",
                        want.name
                    )));
                }
            }
        }
    }
    out
}

/// `source-code.tex:326-330`: a component exporting several apis may not
/// satisfy declarations in more than one of them with a SINGLE definition. Two
/// apis that both declare `T` are two different types, and one `trait T end`
/// cannot be both.
///
/// THE RULE IS ABOUT NAMES ACROSS APIS, not about any one api, which is why it
/// cannot live in `violations` -- that function sees one api at a time and the
/// offence is invisible from inside either of them.
pub fn shared_definitions(component: &Component, apis: &[(&String, Component)]) -> Vec<Violation> {
    let mut out = Vec::new();
    if apis.len() < 2 {
        return out;
    }
    let declared = |c: &Component, name: &str| {
        c.decls.iter().any(|d| match d {
            Decl::Function(f) => f.name == name && participates(f.modifiers),
            Decl::Trait(t) => t.name == name,
            Decl::Object(o) => o.name == name,
        })
    };
    for (index, (first, api)) in apis.iter().enumerate() {
        for decl in &api.decls {
            let name = match decl {
                Decl::Function(f) if participates(f.modifiers) => &f.name,
                Decl::Trait(t) => &t.name,
                Decl::Object(o) => &o.name,
                Decl::Function(_) => continue,
            };
            if !declared(component, name) {
                continue;
            }
            for (second, other) in apis.iter().skip(index + 1) {
                if declared(other, name) {
                    out.push(Violation {
                        api: (*first).clone(),
                        what: format!(
                            "`{name}` to be a different definition from the `{name}` \
                             that `{second}` declares -- one definition may not satisfy \
                             two exported apis"
                        ),
                    });
                }
            }
        }
    }
    out
}

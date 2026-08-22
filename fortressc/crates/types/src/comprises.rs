//! `comprises` well-formedness, which was recorded and read by nothing.
//!
//! `Specification/basic/traits.tex` states three rules and this file is all
//! three. They were worth writing down because the clause had been PARSED AND
//! DROPPED since M3c: `trait T comprises { NoSuchTrait } end` compiled while
//! the identical name in `extends` was refused, and the open marker `...` was
//! discarded outright -- so an open set and an unwritten one were the same
//! empty list.
//!
//! THE OWN/MERGED SPLIT IS NOT TIDINESS. Import resolution PREPENDS an api's
//! traits and objects to the component's declarations, and those declarations
//! carry spans into ANOTHER FILE. Rendering one against this file's text puts
//! the caret in the copyright header -- measured, not feared: the first draft
//! reported Compiled10.p.fss at 10:82, which is inside the comment block. So
//! the rule reads BOTH sets and reports only spans it knows belong here.
//!
//! `open_allowed` is the caller's, not this module's, because a `.fsi` with no
//! `api` header is still an api -- 26 of the corpus's 229 write it that way --
//! and the parser's `is_api` reads the header.

use std::collections::HashMap;

use fortress_ast::{Decl, Span, StaticParam, TypeRef};

use crate::error::TypeError;

/// What one declaration contributes: what it explicitly extends, what it
/// comprises, whether that clause was open, and whether we may point at it.
struct Row<'a> {
    span: Span,
    extends: &'a [TypeRef],
    comprises: &'a [TypeRef],
    /// THE DECLARATION'S OWN STATIC PARAMETERS, and this field is not
    /// bookkeeping. `Library/CompilerAlgebra.fsi:24` writes
    /// `trait Equality[\T\] comprises T`, where `T` IS THE STATIC PARAMETER --
    /// so the name in the clause is not a type at all, and looking it up among
    /// the declarations means finding whatever unrelated `T` the file happens
    /// to declare. `ProjectFortress/test_library/Compiled3.f.fsi` declares
    /// `trait T`, and with `Equality` merged in that combination reported
    /// `T is listed in the comprises clause of Equality but does not
    /// explicitly extend Equality` about two things that have nothing to do
    /// with each other.
    statics: &'a [StaticParam],
    open: bool,
    is_trait: bool,
    own: bool,
}

impl Row<'_> {
    fn is_own_static(&self, name: &str) -> bool {
        self.statics.iter().any(|p| p.name == name)
    }

    /// Whether a `comprises` clause on this row is one THIS FILE wrote. Named
    /// rather than spelled `!r.own` at the site so the mutation table has a
    /// UNIQUE line to reach: the open-comprises rule below tests `own` too, and
    /// a `from` pattern matching twice is a row that silently does nothing.
    const fn clause_is_ours(&self) -> bool {
        self.own
    }
}

impl Row<'_> {
    /// A span to report against: this row's if it is ours, else the caller's
    /// fallback. Never another file's offsets.
    const fn at(&self, fallback: Span) -> Span {
        if self.own {
            self.span
        } else {
            fallback
        }
    }
}

fn head(ty: &TypeRef) -> Option<&str> {
    match ty {
        TypeRef::Named { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

fn row(decl: &Decl, own: bool) -> Option<(&str, Row<'_>)> {
    match decl {
        // A value has no `extends` and no `comprises`; there is no row.
        Decl::Value(_) => None,
        Decl::Trait(t) => Some((
            t.name.as_str(),
            Row {
                span: t.span,
                extends: &t.extends,
                comprises: &t.comprises,
                statics: &t.static_params,
                open: t.comprises_open,
                is_trait: true,
                own,
            },
        )),
        Decl::Object(o) => Some((
            o.name.as_str(),
            Row {
                span: o.span,
                extends: &o.extends,
                comprises: &o.comprises,
                statics: &o.static_params,
                open: o.comprises_open,
                is_trait: false,
                own,
            },
        )),
        Decl::Function(_) => None,
    }
}

/// Checks one declaration set. `merged` is what resolution prepended and is
/// read but never pointed at; `open_allowed` says whether this file is an api.
pub fn check(
    own: &[Decl],
    merged: &[Decl],
    open_allowed: bool,
    fallback: Span,
) -> Result<(), TypeError> {
    // DECLARATION ORDER IS CARRIED ALONGSIDE THE MAP, and it is not tidiness.
    // Every rule below reports the FIRST violation it meets; iterating a
    // `HashMap` meant the first was whichever the hasher happened to yield, so
    // the SAME BINARY reported `XXXComprisesHidden.fss` against `T` on one run
    // and `S` on the next. Both are correct refusals of the same file, which is
    // why it went unnoticed -- and this project asserts MESSAGES, so a
    // nondeterministic one is a flaky gate waiting to happen.
    let mut rows: HashMap<&str, Row> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for (decl, is_own) in merged
        .iter()
        .map(|d| (d, false))
        .chain(own.iter().map(|d| (d, true)))
    {
        let Some((name, r)) = row(decl, is_own) else {
            continue;
        };
        // An own declaration REPLACES a merged one of the same name and keeps
        // the merged one's position, so the order is a pure function of the
        // two inputs either way.
        if rows.insert(name, r).is_none() {
            order.push(name);
        }
    }

    // traits.tex:161-162 -- "In an API (but not a component), a `comprises`
    // clause may include `...`". A rule about WHERE the marker may be written,
    // so it reads only what this file wrote: an open clause arriving through a
    // resolved api is that api's business and is legal there.
    if !open_allowed {
        for name in &order {
            let Some(r) = rows.get(name) else { continue };
            if r.open && r.own {
                return Err(TypeError::OpenComprisesInComponent {
                    span: r.span,
                    name: (*name).to_owned(),
                });
            }
        }
    }

    // traits.tex:232-235 -- the traits listed "are exactly the traits that
    // immediately extend T and they must explicitly extend T (i.e., list T in
    // their `extends` clause)". A name that is not declared in either set is
    // NOT reported: an api this compiler cannot read yet is skipped by the
    // resolver, so demanding the declaration would measure the parser.
    for name in &order {
        let Some(r) = rows.get(name) else { continue };
        // THE CLAUSE MUST BE ONE THIS FILE WROTE, which is the same guard the
        // open-comprises rule below already carries and for the same reason.
        // A merged api's `comprises` clause names ITS OWN declarations, and the
        // resolver deliberately lets this file's declarations WIN a contested
        // name -- so the name in the clause and the row it finds here can be
        // two unrelated types. `ProjectFortress/BirdyLib/Comparison.fsi`
        // declares `object LessThan extends Comparison`, the merged builtin
        // declares `trait TotalComparison comprises { LessThan, ... }`, and
        // together they reported that BirdyLib's `LessThan` fails to extend a
        // trait BirdyLib has never heard of. The api is refused on its own when
        // it is compiled, which is where that error belongs.
        if !r.clause_is_ours() {
            continue;
        }
        for listed in r.comprises {
            let Some(sub) = head(listed) else { continue };
            // A STATIC PARAMETER IS NOT A TYPE NAME. See `Row::statics`.
            if r.is_own_static(sub) {
                continue;
            }
            let Some(sub_row) = rows.get(sub) else {
                continue;
            };
            if !sub_row.extends.iter().any(|e| head(e) == Some(*name)) {
                return Err(TypeError::ComprisesNameDoesNotExtend {
                    span: if sub_row.own {
                        sub_row.span
                    } else {
                        r.at(fallback)
                    },
                    trait_name: (*name).to_owned(),
                    listed: sub.to_owned(),
                });
            }
        }
    }

    // traits.tex:236-241 -- when a `comprises` clause includes `...`, a
    // component exporting the api may extend the trait, "but these traits may
    // not be declared or imported by the API". The offence is a declaration in
    // THE SAME SET extending an open-comprises trait, so this loop reads every
    // row's `extends` against every open row rather than one row against
    // itself, and it only fires where the open clause is legal in the first
    // place -- in a component the previous rule has already refused.
    let open: Vec<&str> = order
        .iter()
        .filter(|n| rows.get(*n).is_some_and(|r| r.open && r.is_trait))
        .copied()
        .collect();
    if !open.is_empty() {
        for name in &order {
            let Some(r) = rows.get(name) else { continue };
            // THE EXTENDER MUST BE ONE THIS FILE WROTE. A component that
            // imports an api gets that api's traits merged into it, and
            // reporting the api's own ill-formedness against every importer is
            // the foreign-span problem one level up: the offence is not in this
            // file, nobody editing this file can fix it, and the api is refused
            // on its own when it is compiled.
            //
            // MEASURED, not feared: making `Library/FortressLibrary.fsi` parse
            // in full took `SpecData/examples/advanced/Overloading.fss` from
            // compiling to refused, pointing its caret at the component header,
            // because `trait AnyIntegral extends { QQ }` arrived through
            // resolution. That number grows with every api the parser learns to
            // read.
            if !r.own {
                continue;
            }
            for sup in r.extends {
                let Some(sup_name) = head(sup) else { continue };
                if open.contains(&sup_name) {
                    return Err(TypeError::ExtendsOpenComprises {
                        span: r.span,
                        trait_name: sup_name.to_owned(),
                        extender: (*name).to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}

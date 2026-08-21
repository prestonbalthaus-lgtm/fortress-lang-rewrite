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

use fortress_ast::{Decl, Span, TypeRef};

use crate::error::TypeError;

/// What one declaration contributes: what it explicitly extends, what it
/// comprises, whether that clause was open, and whether we may point at it.
struct Row<'a> {
    span: Span,
    extends: &'a [TypeRef],
    comprises: &'a [TypeRef],
    open: bool,
    is_trait: bool,
    own: bool,
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
        Decl::Trait(t) => Some((
            t.name.as_str(),
            Row {
                span: t.span,
                extends: &t.extends,
                comprises: &t.comprises,
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
    let rows: HashMap<&str, Row> = merged
        .iter()
        .filter_map(|d| row(d, false))
        .chain(own.iter().filter_map(|d| row(d, true)))
        .collect();

    // traits.tex:161-162 -- "In an API (but not a component), a `comprises`
    // clause may include `...`". A rule about WHERE the marker may be written,
    // so it reads only what this file wrote: an open clause arriving through a
    // resolved api is that api's business and is legal there.
    if !open_allowed {
        for (name, r) in &rows {
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
    for (name, r) in &rows {
        for listed in r.comprises {
            let Some(sub) = head(listed) else { continue };
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
    let open: Vec<&str> = rows
        .iter()
        .filter(|(_, r)| r.open && r.is_trait)
        .map(|(n, _)| *n)
        .collect();
    if !open.is_empty() {
        for (name, r) in &rows {
            for sup in r.extends {
                let Some(sup_name) = head(sup) else { continue };
                if open.contains(&sup_name) {
                    return Err(TypeError::ExtendsOpenComprises {
                        span: r.at(fallback),
                        trait_name: sup_name.to_owned(),
                        extender: (*name).to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}

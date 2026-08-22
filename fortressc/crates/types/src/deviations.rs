//! NAMED DEVIATIONS: things 1.0 has, that this compiler will not implement, and
//! that must be refused BY NAME rather than left to fail as something else.
//!
//! The rule this file exists for is `D7 §3.2`, and its own wording is the
//! specification: the diagnostic "must ... name the mechanism ... and not a
//! generic type error, because the failure will otherwise surface as an
//! unrelated mismatch deep inside `ChunkedSparseArray`".

use fortress_ast::{Decl, Span, TypeRef};

use crate::error::TypeError;

/// `NatReflect.reflect(z:ZZ32):NatParam` turns a RUN-TIME integer into a static
/// `nat` parameter. A monomorphizing compiler cannot stamp a specialisation for
/// a value it does not know, so the whole path is out of v1.
///
/// THE CARRIER IS THE TRAIT, not the call. `NatParam` is what the runtime value
/// travels in and what `N[\n\]` is matched against, so refusing the trait
/// catches the api that declares it, the component that defines it, and any
/// component that imports either -- one rule, three positions. `reflect` is
/// refused on its own too, because a component could declare it without the
/// trait being in scope.
///
/// COST, MEASURED: SIX corpus files mention `NatParam` and FIVE mention
/// `reflect(`. NOT ONE OF THEM COMPILES TODAY, so this refuses nothing that
/// works and bookmarks exactly the thing D7 asked to be bookmarked.
const CARRIER: &str = "NatParam";
const CONVERTER: &str = "reflect";

fn returns_carrier(ty: Option<&TypeRef>) -> bool {
    matches!(ty, Some(TypeRef::Named { name, .. }) if name == CARRIER)
}

/// `merged` is what import resolution prepended: it is READ and never pointed
/// at, because its spans index another file's text. Same split, same reason, as
/// [`crate::comprises`].
pub fn check(own: &[Decl], merged: &[Decl], fallback: Span) -> Result<(), TypeError> {
    for (decl, here) in own
        .iter()
        .map(|d| (d, true))
        .chain(merged.iter().map(|d| (d, false)))
    {
        let hit = match decl {
            Decl::Trait(t) => (t.name == CARRIER).then_some(t.span),
            Decl::Object(o) => (o.name == CARRIER).then_some(o.span),
            Decl::Function(f) => {
                (f.name == CONVERTER && returns_carrier(f.return_type.as_ref())).then_some(f.span)
            }
            // The carrier is a TRAIT and the converter a FUNCTION; a value can
            // be neither, so it can never be the deviation this looks for.
            Decl::Value(_) => None,
        };
        if let Some(span) = hit {
            return Err(TypeError::NatReflectRuntimeArgument {
                span: if here { span } else { fallback },
            });
        }
    }
    Ok(())
}

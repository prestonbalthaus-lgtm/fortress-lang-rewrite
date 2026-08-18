//! The trait hierarchy, closed transitively, and the concrete types beneath it.
//!
//! Everything here is decided at compile time. Traits have no run-time
//! representation: membership is a fact about a concrete object's tag, and this
//! is where that fact is computed.

use std::collections::{BTreeMap, BTreeSet};

use fortress_ast::{Span, TypeRef};

use crate::error::TypeError;
use crate::types::{Elem, Type, TypedField};

#[derive(Debug)]
pub(crate) struct TraitInfo {
    /// Transitive, not including the trait itself.
    pub(crate) supertraits: BTreeSet<&'static str>,
}

#[derive(Debug)]
pub(crate) struct ObjectInfo {
    pub(crate) tag: u32,
    pub(crate) supertraits: BTreeSet<&'static str>,
    /// Layout order. The first `param_count` are the constructor's parameters.
    pub(crate) fields: Vec<TypedField>,
    pub(crate) param_count: usize,
    pub(crate) singleton: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Registry {
    pub(crate) traits: BTreeMap<&'static str, TraitInfo>,
    pub(crate) objects: BTreeMap<&'static str, ObjectInfo>,
    /// Declaration order. Tags follow it, and so do the arms of every switch,
    /// which is what keeps the emitted module deterministic.
    pub(crate) concrete: Vec<&'static str>,
}

impl Registry {
    pub(crate) fn is_object(&self, name: &str) -> bool {
        self.objects.contains_key(name)
    }

    /// Reflexive subtyping over the closed hierarchy. Scalars are unrelated to
    /// everything but themselves: a scalar implementing a trait would force
    /// boxing, which is M3d.
    pub(crate) fn is_subtype(&self, sub: Type, sup: Type) -> bool {
        if sub == sup {
            return true;
        }
        let Type::Trait(wanted) = sup else {
            return false;
        };
        match sub {
            Type::Object(name) => self
                .objects
                .get(name)
                .is_some_and(|o| o.supertraits.contains(wanted)),
            Type::Trait(name) => self
                .traits
                .get(name)
                .is_some_and(|t| t.supertraits.contains(wanted)),
            _ => false,
        }
    }

    /// Every concrete type a value of this trait can actually be, in
    /// declaration order. Closed world: this is the whole program.
    pub(crate) fn concretes_below(&self, wanted: &'static str) -> Vec<&'static str> {
        self.concrete
            .iter()
            .copied()
            .filter(|name| {
                self.objects
                    .get(name)
                    .is_some_and(|o| o.supertraits.contains(wanted))
            })
            .collect()
    }

    pub(crate) fn tag_of(&self, name: &str) -> Option<u32> {
        self.objects.get(name).map(|o| o.tag)
    }

    pub(crate) fn field(&self, object: &str, field: &str) -> Option<(u32, Type)> {
        let info = self.objects.get(object)?;
        let (index, found) = info
            .fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == field)?;
        u32::try_from(index).ok().map(|i| (i, found.ty))
    }

    /// A written type name to a [`Type`]. The registry's keys are populated
    /// before any type is resolved, so a forward reference to an object
    /// declared later in the file works.
    pub(crate) fn resolve(&self, t: &TypeRef) -> Result<Type, TypeError> {
        if t.name == "Array" {
            let Some(argument) = &t.argument else {
                return Err(TypeError::UnsupportedElementType {
                    span: t.span,
                    name: "Array".to_owned(),
                });
            };
            let inner = self.resolve(argument)?;
            return Elem::of(inner).map(Type::Array).ok_or_else(|| {
                TypeError::UnsupportedElementType {
                    span: argument.span,
                    name: inner.name().to_owned(),
                }
            });
        }
        if t.argument.is_some() {
            return Err(TypeError::UnknownType {
                span: t.span,
                name: t.name.clone(),
            });
        }
        match t.name.as_str() {
            "ZZ32" => Ok(Type::ZZ32),
            "ZZ64" => Ok(Type::ZZ64),
            "RR64" => Ok(Type::RR64),
            "Boolean" => Ok(Type::Boolean),
            "String" => Ok(Type::String),
            name => {
                if let Some((interned, _)) = self.traits.get_key_value(name) {
                    return Ok(Type::Trait(interned));
                }
                if let Some((interned, _)) = self.objects.get_key_value(name) {
                    return Ok(Type::Object(interned));
                }
                Err(TypeError::UnknownType {
                    span: t.span,
                    name: t.name.clone(),
                })
            }
        }
    }
}

/// Transitive closure of `extends`, by repeated expansion with a visited set.
/// A cycle is a diagnostic naming the trait, not a hang.
pub(crate) fn close_traits(
    direct: &BTreeMap<&'static str, Vec<&'static str>>,
    spans: &BTreeMap<&'static str, Span>,
) -> Result<BTreeMap<&'static str, BTreeSet<&'static str>>, TypeError> {
    let mut closed: BTreeMap<&'static str, BTreeSet<&'static str>> = BTreeMap::new();
    for &name in direct.keys() {
        let mut seen: BTreeSet<&'static str> = BTreeSet::new();
        let mut stack: Vec<&'static str> = direct.get(name).cloned().unwrap_or_default();
        while let Some(next) = stack.pop() {
            if next == name {
                return Err(TypeError::TraitCycle {
                    span: spans.get(name).copied().unwrap_or(Span::new(0, 0)),
                    name: name.to_owned(),
                });
            }
            if !seen.insert(next) {
                continue;
            }
            stack.extend(direct.get(next).into_iter().flatten().copied());
        }
        closed.insert(name, seen);
    }
    Ok(closed)
}

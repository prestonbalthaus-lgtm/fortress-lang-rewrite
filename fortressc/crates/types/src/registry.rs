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
            // A TUPLE IS OUTSIDE THE TRAIT HIERARCHY, and saying so here is a
            // decision rather than a default. `types.tex` gives tuples
            // structural subtyping element by element; nothing in this compiler
            // implements it, and a `_` arm would have answered the same `false`
            // without anyone having decided. The day tuple values land, this
            // arm is where element-wise subtyping goes.
            Type::Tuple(_) => false,
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
        self.field_decl(object, field).map(|(i, f)| (i, f.ty))
    }

    pub(crate) fn field_decl(&self, object: &str, field: &str) -> Option<(u32, &TypedField)> {
        let info = self.objects.get(object)?;
        let (index, found) = info
            .fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == field)?;
        u32::try_from(index).ok().map(|i| (i, found))
    }

    /// The first path from `ty` to storage a parallel iteration could write:
    /// an array, or a field declared `var`. Reference fields are followed, so
    /// an object holding an object holding an array is found. A trait is every
    /// concrete type below it, because the value could be any of them.
    ///
    /// This is what replaces "an object cannot be written through", which was
    /// true only while no field store existed in the language.
    pub(crate) fn reaches_mutable(&self, ty: Type) -> Option<String> {
        let mut seen: BTreeSet<&'static str> = BTreeSet::new();
        self.reaches_from(ty, &mut seen)
    }

    fn reaches_from(&self, ty: Type, seen: &mut BTreeSet<&'static str>) -> Option<String> {
        match ty {
            Type::Array(_) => Some(String::new()),
            Type::Object(name) => {
                if !seen.insert(name) {
                    return None;
                }
                let info = self.objects.get(name)?;
                info.fields.iter().find_map(|f| {
                    if f.mutable {
                        return Some(f.name.clone());
                    }
                    let rest = self.reaches_from(f.ty, seen)?;
                    Some(if rest.is_empty() {
                        f.name.clone()
                    } else {
                        format!("{}.{rest}", f.name)
                    })
                })
            }
            Type::Trait(name) => self
                .concretes_below(name)
                .into_iter()
                .find_map(|concrete| self.reaches_from(Type::Object(concrete), seen)),
            // THE ONE CATCH-ALL IN THIS COMPILER A TUPLE WOULD HAVE BEEN
            // SILENTLY WRONG IN. `_ => None` means "reaches no mutable
            // storage", and a tuple holding an array reaches one -- so a
            // parallel loop body could have written through it and M4's race
            // freedom would have rested on a default. It recurses.
            Type::Tuple(elems) => elems.iter().enumerate().find_map(|(index, elem)| {
                let rest = self.reaches_from(*elem, seen)?;
                Some(if rest.is_empty() {
                    index.to_string()
                } else {
                    format!("{index}.{rest}")
                })
            }),
            _ => None,
        }
    }

    /// A written type name to a [`Type`]. The registry's keys are populated
    /// before any type is resolved, so a forward reference to an object
    /// declared later in the file works.
    pub(crate) fn resolve(&self, t: &TypeRef) -> Result<Type, TypeError> {
        let (name, args, span) = match t {
            TypeRef::Named { name, args, span } => (name, args, *span),
            TypeRef::Unit { .. } => return Ok(Type::Void),
            TypeRef::Tuple { span, .. } => {
                return Err(TypeError::TypeNotImplemented {
                    span: *span,
                    form: "a tuple type",
                })
            }
            TypeRef::Arrow { span, .. } => {
                return Err(TypeError::TypeNotImplemented {
                    span: *span,
                    form: "an arrow type",
                })
            }
        };
        if name == "Array" {
            let [argument] = args.as_slice() else {
                return Err(TypeError::UnsupportedElementType {
                    span,
                    name: "Array".to_owned(),
                });
            };
            let inner = self.resolve(argument)?;
            if inner == Type::Void {
                return Err(TypeError::VoidNotStorable {
                    span: argument.span(),
                    position: "an array element",
                });
            }
            return Elem::of(inner).map(Type::Array).ok_or_else(|| {
                TypeError::UnsupportedElementType {
                    span: argument.span(),
                    name: inner.name().to_owned(),
                }
            });
        }
        // Anything else carrying static arguments here means a generic survived
        // expansion, which cannot happen: `check` runs `expand` first.
        if !args.is_empty() {
            return Err(TypeError::UnknownType {
                span,
                name: name.clone(),
            });
        }
        match name.as_str() {
            "ZZ32" => Ok(Type::ZZ32),
            "ZZ64" => Ok(Type::ZZ64),
            "RR64" => Ok(Type::RR64),
            "Boolean" => Ok(Type::Boolean),
            "String" => Ok(Type::String),
            other => {
                if let Some((interned, _)) = self.traits.get_key_value(other) {
                    return Ok(Type::Trait(interned));
                }
                if let Some((interned, _)) = self.objects.get_key_value(other) {
                    return Ok(Type::Object(interned));
                }
                Err(TypeError::UnknownType {
                    span,
                    name: name.clone(),
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

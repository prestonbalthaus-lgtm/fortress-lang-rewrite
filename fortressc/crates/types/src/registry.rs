//! The trait hierarchy, closed transitively, and the concrete types beneath it.
//!
//! Everything here is decided at compile time. Traits have no run-time
//! representation: membership is a fact about a concrete object's tag, and this
//! is where that fact is computed.

use std::collections::{BTreeMap, BTreeSet};

use fortress_ast::{ExtentForm, ExtentRange, ShapeSpelling, Span, TypeRef};

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
    /// The dimension and unit names this component declares. Read at exactly
    /// one place, `resolve_name`, so that a dimension written where a type is
    /// required says which of the two it is instead of `unknown type` -- which
    /// would send the reader looking for a declaration that IS there.
    pub(crate) dimensions: crate::dimensions::Dimensions,
}

impl Registry {
    pub(crate) fn is_object(&self, name: &str) -> bool {
        self.objects.contains_key(name)
    }

    /// An object with no value parameters. It declares a VALUE of its own name
    /// rather than a constructor, which is what separates row 2 of
    /// `Compiled9.c.fss`'s collision matrix from row 3.
    pub(crate) fn is_singleton(&self, name: &str) -> bool {
        self.objects.get(name).is_some_and(|o| o.singleton)
    }

    /// Reflexive subtyping over the closed hierarchy. Scalars are unrelated to
    /// everything but themselves: a scalar implementing a trait would force
    /// boxing, which is M3d.
    pub(crate) fn is_subtype(&self, sub: Type, sup: Type) -> bool {
        if sub == sup {
            return true;
        }
        // A THREAD HANDLE IS COVARIANT TO `Any` AND NOWHERE ELSE, which is the
        // narrowest rule that types the corpus. Every Spawn file declares
        // `Thread[\Any\]` and spawns a body of some other type -- `()` in
        // Spawn1/2/3 -- so without this the declaration and the initializer
        // never agree.
        //
        // COVARIANCE IS SOUND HERE ONLY BECAUSE THE HANDLE IS READ-ONLY:
        // `val()` is the sole reader and it is REFUSED on a `Thread[\Any\]`,
        // because `Any` is a trait, a trait-typed value is a pointer to a
        // tagged object, and a body returning a `ZZ32` produces no tag. So
        // nothing can observe the widened element at all. Widening this to
        // general covariance needs that refusal revisited first.
        //
        // IT SITS ABOVE THE TRAIT GUARD BECAUSE THE SUPERTYPE IS NOT A TRAIT.
        // Written below it the arm is unreachable -- `sup` is a `Thread`, the
        // guard returns false, and the rule silently does nothing. It did
        // exactly that for one build.
        if let (Type::Thread(_), Type::Thread(want)) = (sub, sup) {
            return matches!(*want, Type::Trait("Any"));
        }
        let Type::Trait(wanted) = sup else {
            return false;
        };
        // `types-vals-vars.tex:121-122`: "`Any` is the top of the type
        // hierarchy: all types are subtypes of `Any`." :263 and :374 say it
        // again for tuple and arrow types, and :136 lists the IMMEDIATE
        // subtypes of `Any` as tuple types, arrow types, `()` and `Object` --
        // so a scalar reaches `Any` WITHOUT passing under `Object`, and this
        // arm is deliberately not `Object`'s. `Object` here means "has a 32-bit
        // type tag", which a scalar does not.
        //
        // SUBTYPING IS NOT STORAGE, and that split is the whole reason this is
        // safe to say. A trait slot is a POINTER TO A TAGGED BLOCK and a `ZZ32`
        // is an unboxed `i32`; what stops one reaching the other is
        // `Checker::needs_a_trait_representation`, not this function. The `()`
        // arm below said exactly this for one type and is now general -- it
        // reads `false` because `Any` never gets here any more.
        if wanted == "Any" {
            return true;
        }
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
            // `()` IS AN IMMEDIATE SUBTYPE OF `Any` AND OF NOTHING ELSE.
            // `basic/types-vals-vars.tex:469-471`: "The type `()` is the type
            // of the value `()`. Its only supertype (other than itself) is
            // `Any`, and it excludes every other type." :136 lists the
            // immediate subtypes of `Any` as tuple types, arrow types, `()`
            // and `Object` -- so it does NOT sit under `Object`, and writing it
            // that way would put it below every user trait's root. THE `Any`
            // HALF MOVED UP, where it now speaks for every type; what is left
            // here is the "and of nothing else".
            Type::Void => false,
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
            Type::Array(..) => Some(String::new()),
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
            // A TUPLE TYPE RESOLVES. This is the single construction gate that
            // `Type::Tuple`'s own comment named as refusing, and it no longer
            // does -- SPIKE-COMPOSITE-TYPE landed the variant precisely so this
            // line could become a build.
            //
            // RESOLVING IS NOT THE SAME AS HAVING A REPRESENTATION, and the two
            // are deliberately separated. An `api` never reaches codegen, so a
            // signature may name a tuple freely -- `FortressLibrary.fsi:2347`
            // writes `opr ||(self, b:(Any,Any)):String` and :1730 writes one as
            // a static argument. A DEFINED function is refused by name in the
            // checker instead, at `tuple_free`, until values land.
            TypeRef::Tuple { elems, .. } => {
                let mut built = Vec::with_capacity(elems.len());
                for e in elems {
                    built.push(self.resolve(e)?);
                }
                return Ok(Type::Tuple(crate::types::intern_types(&built)));
            }
            // A VALUE where a TYPE is required. This is reachable from real
            // source -- `Cell[\ 3 \]` where `Cell`'s parameter is a type --
            // and the diagnostic has to say which of the two it got, because
            // "unknown type `3`" would send the reader looking for a
            // declaration.
            TypeRef::Static { expr, span } => {
                return Err(TypeError::StaticValueWhereTypeRequired {
                    span: *span,
                    written: expr.written(),
                })
            }
            TypeRef::Arrow { span, .. } => {
                return Err(TypeError::TypeNotImplemented {
                    span: *span,
                    form: "an arrow type",
                })
            }
            // THE SINGLE GATE for every shape suffix. `Type::Array` carries a
            // RANK now and no extent, so nothing below this line can construct
            // a non-zero origin or a matrix -- which is what makes the
            // invariant checkable by reading one function rather than by
            // trusting every construction site.
            TypeRef::Shaped {
                base,
                spelling,
                extents,
                span,
            } => return self.resolve_shaped(base, *spelling, extents, *span),
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
            // `Array[\\T\\]` written by name is RANK ONE. The higher ranks have
            // no spelling in static-argument position in this subset -- 1.0
            // writes `Array2[\\T, b0, s0, b1, s1\\]` and nothing in the corpus
            // does -- so they arrive only through a shape suffix below.
            return Elem::of(inner).map(|e| Type::Array(e, 1)).ok_or_else(|| {
                TypeError::UnsupportedElementType {
                    span: argument.span(),
                    name: inner.name().to_owned(),
                }
            });
        }
        // `Thread[\T\]` is written like a declared generic and resolved like a
        // builtin, exactly as `Array[\T\]` above is. The argument is the
        // body's RESULT type, and `Any` is what the whole corpus writes there.
        debug_assert!(
            crate::types::BUILTIN_TYPE_CONSTRUCTORS.contains(&"Thread"),
            "a builtin constructor the registry builds must also be one expansion lets through"
        );
        if name == "Thread" {
            let [argument] = args.as_slice() else {
                return Err(TypeError::UnsupportedElementType {
                    span,
                    name: "Thread".to_owned(),
                });
            };
            return Ok(Type::Thread(crate::types::intern_type(
                self.resolve(argument)?,
            )));
        }
        // Anything else carrying static arguments here means a generic survived
        // expansion, which cannot happen: `check` runs `expand` first.
        if !args.is_empty() {
            return Err(TypeError::UnknownType {
                span,
                name: name.clone(),
            });
        }
        self.resolve_name(name, span)
    }

    /// `ZZ32[5]` is `Array[\ZZ32\]` with a size the checker can compare
    /// against a literal, and nothing else in this subset resolves.
    ///
    /// THE SIZE IS VALIDATED HERE AND DROPPED HERE, and both halves are the
    /// decision. Carrying it into `Type` was priced and refused: `Type` is
    /// `Copy` and compared with `==` at `is_subtype`, at overload duplicate
    /// detection and across M3c's whole dispatch domain, so an extent inside
    /// it re-decides what type equality means -- and `array(n)` takes a
    /// RUN-TIME count, which has no extent to supply at all. What survives is
    /// a declaration-site check (`Checker::check_declared_extent`) and a
    /// NAMED DEVIATION with two stated holes: a mismatch arriving through a
    /// call is not caught, and a parameter's declared extent is not checked
    /// against its argument.
    fn resolve_shaped(
        &self,
        base: &TypeRef,
        spelling: ShapeSpelling,
        extents: &[ExtentRange],
        span: Span,
    ) -> Result<Type, TypeError> {
        // `traits.tex:99-101`. `RR^3` and `ZZ32^(2 BY 4)` are 1.0's VECTOR and
        // MATRIX types, which are not `Array1` and do not share its trait, so
        // resolving them to a one dimensional array would be a wrong answer
        // rather than a partial one. All 18 corpus sites are shapes; not one
        // is the dimension exponent that shares the spelling.
        if spelling == ShapeSpelling::Caret {
            return Err(TypeError::TypeNotImplemented {
                span,
                form: "a vector or matrix type",
            });
        }
        // EVERY DIMENSION IS CHECKED, not just the first. The extents were
        // always a `Vec` -- the parser has read `ZZ32[2,3]` since `T[n]`
        // landed -- and this was the single line that refused what it read.
        if extents.is_empty() {
            return Err(TypeError::ArrayDimensions {
                span,
                dimensions: 0,
            });
        }
        let Ok(rank) = u8::try_from(extents.len()) else {
            return Err(TypeError::ArrayDimensions {
                span,
                dimensions: extents.len(),
            });
        };
        for extent in extents {
            let Some(size) = extent.plain_size() else {
                if extent.form == ExtentForm::Size {
                    return Err(TypeError::ArraySizeMissing { span });
                }
                return Err(TypeError::ExtentRangeNotImplemented {
                    span: extent.span,
                    written: extent.written(),
                });
            };
            // A name here survived `mono`'s substitution, which means it
            // resolved to nothing. Saying so beats `unknown type `n``, which
            // sends the reader looking for a declaration that was never meant
            // to exist.
            if !matches!(size, TypeRef::Static { .. }) {
                return Err(TypeError::ArraySizeNotStatic {
                    span: size.span(),
                    written: size.written(),
                });
            }
        }
        let inner = self.resolve(base)?;
        if inner == Type::Void {
            return Err(TypeError::VoidNotStorable {
                span: base.span(),
                position: "an array element",
            });
        }
        Elem::of(inner)
            .map(|e| Type::Array(e, rank))
            .ok_or_else(|| TypeError::UnsupportedElementType {
                span: base.span(),
                name: inner.name().to_owned(),
            })
    }

    /// A DECLARED TRAIT OR OBJECT WINS OVER THE BUILTIN SCALAR OF THE SAME
    /// NAME, and this order is the whole of the scalar-supertrait fix.
    ///
    /// The builtin names were matched FIRST and unconditionally, which made
    /// them a RESERVED NAMESPACE. They are not one. `RR64` and `Boolean` are
    /// ordinary library traits in 1.0 -- `conversions-coercions.tex:850-866`
    /// writes `trait RR64 ... coerce(x: RR32) widens`, and
    /// `Library/FortressLibrary.fsi:335` declares
    /// `trait RR64 extends Number comprises { Float, FloatLiteral, RR32, QQ }`
    /// forty-one lines above the `trait QQ extends { RR64, ... }` that this
    /// compiler refused. The declaration was REGISTERED and then UNREACHABLE:
    /// the trait went into `self.traits` and nothing could ever resolve to it.
    ///
    /// So this is a name resolution defect and NOT the representation question
    /// it was filed as. A trait extending a scalar needs no tag, no boxing and
    /// no injection, because with the declaration reachable `RR64` in supertype
    /// position resolves to `Type::Trait("RR64")` and every pass downstream is
    /// the ordinary trait machinery. `is_subtype` is untouched.
    ///
    /// THE BUILTIN LIST IS A BOOTSTRAP VOCABULARY, which is what
    /// `types::BUILTIN_TYPE_NAMES` already says of `Any` and `Object`: they are
    /// seeded because nothing can import them YET, and they come out on the day
    /// import resolution can supply them. The scalars are the same trajectory,
    /// one step earlier -- a component that declares the name means its own
    /// declaration, exactly as `Compiled6.u.fss` says with
    /// `import CompilerBuiltin.{...} except Boolean` above its own
    /// `trait Boolean`.
    ///
    /// TWO HOLES, both measured at zero corpus cost and neither built for:
    ///   * In a shadowing component the builtin becomes UNNAMEABLE, so a float
    ///     in a BODY there has no type to be. No file reaches it --
    ///     `FortressLibrary.fss` is far behind the `.fsi`.
    ///   * Static arguments resolve during EXPANSION, before this registry
    ///     exists, so `[\RR64\]` in a shadowing component stamps the builtin
    ///     while the same name in a signature resolves to the trait. Nothing in
    ///     the corpus writes one.
    fn resolve_name(&self, name: &str, span: Span) -> Result<Type, TypeError> {
        if let Some((interned, _)) = self.traits.get_key_value(name) {
            return Ok(Type::Trait(interned));
        }
        if let Some((interned, _)) = self.objects.get_key_value(name) {
            return Ok(Type::Object(interned));
        }
        match name {
            "ZZ32" => Ok(Type::ZZ32),
            "ZZ64" => Ok(Type::ZZ64),
            "RR64" => Ok(Type::RR64),
            "Boolean" => Ok(Type::Boolean),
            "String" => Ok(Type::String),
            "Char" => Ok(Type::Char),
            _ => {
                if let Some(kind) = self.dimensions.describes(name) {
                    return Err(TypeError::DimensionIsNotAType {
                        span,
                        name: name.to_owned(),
                        kind,
                    });
                }
                Err(TypeError::UnknownType {
                    span,
                    name: name.to_owned(),
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

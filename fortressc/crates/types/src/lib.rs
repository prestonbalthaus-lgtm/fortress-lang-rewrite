//! The numeric tower and static overload resolution.
//!
//! Two rules that must stay distinct, and the negative tests exist to keep them
//! that way:
//!
//! * Literals are unfixed until context pins them. `1` in a `ZZ64` slot is a
//!   `ZZ64` literal, not a `ZZ32` value being converted.
//! * Values are never implicitly converted. A `ZZ32` variable in a `ZZ64` slot
//!   is an error, and the fix is to write `widen`.
//!
//! Everything leaves here resolved to one concrete [`Target`], so codegen never
//! asks a type question.

mod error;
mod mono;
mod registry;
mod types;

pub use mono::{expand, mangle_static, MAX_INSTANTIATIONS};

pub use error::TypeError;
pub use types::{
    intern, ArithOp, AssignTarget, CompareOp, DispatchFn, DispatchNode, Elem, MpiOp, Target, Type,
    TypedBlockItem, TypedComponent, TypedExpr, TypedExprKind, TypedField, TypedFn, TypedObject,
    TypedParam, ARRAY_ALLOC, ARRAY_LENGTH, ARRAY_SLOT, DISPATCH_FAILED, FIRST_TAG, OBJECT_ALLOC,
};

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use fortress_ast::{
    Assign, BinOp, BlockItem, Component, Decl, Expr, FieldDecl, FnDecl, Member, MethodDecl,
    ObjectDecl, Span, TypeRef, UnOp,
};

use registry::{close_traits, ObjectInfo, Registry, TraitInfo};

/// What a name in scope is: its type, and whether it can be assigned to.
#[derive(Debug, Clone, Copy)]
struct Local {
    ty: Type,
    mutable: bool,
}

type Checked<T> = Result<T, TypeError>;

/// Expansion, then checking, in that order and never interleaved. The phase
/// split is what keeps M3c's dispatch tables correct: `registry.concrete` and
/// every 32-bit tag freeze in `Checker::new`, so the set of concrete types has
/// to be closed before that happens.
pub fn check(component: &Component) -> Checked<TypedComponent> {
    let ground = mono::expand(component)?;
    Checker::new(&ground)?.run(&ground)
}

/// One declaration in an overload set.
#[derive(Debug, Clone)]
struct Signature {
    params: Vec<Type>,
    returns: Type,
    /// False for a bodiless declaration: an abstract method types a call and
    /// names a return, but can never be a dispatch target. Excluding it from
    /// the table is what makes an unimplemented abstract method fail M3c's
    /// exactly-one-winner check instead of needing a rule of its own.
    concrete: bool,
    /// What codegen emits. A set of one keeps its bare Fortress name, so every
    /// symbol that existed before M3c is byte for byte what it was.
    symbol: String,
    /// An over-approximated method stamp whose bound turned out not to hold.
    /// Expansion guessed the receiver, the guess was wrong, and the program is
    /// not at fault -- so the stamp leaves the language entirely rather than
    /// refusing the component or, worse, staying as a dispatch target.
    pruned: bool,
    span: Span,
}

/// The ceiling on |concrete types|^k. A compiler that hangs on user source is
/// barely better than one that panics on it, so reaching this is a diagnostic.
const MAX_DISPATCH_CELLS: usize = 1_000_000;

/// While a dotted method body is checked: the receiver's type, and the fields
/// a bare name may resolve against. A method sees its object's fields.
struct SelfCtx {
    ty: Type,
    fields: Vec<TypedField>,
}

struct Checker {
    registry: Registry,
    functions: HashMap<String, Vec<Signature>>,
    /// Dotted methods, and deliberately NOT `functions`: 1.0 gives `x.f(y)` its
    /// own namespace and its own shadowing rules, so a method never collides
    /// with a top-level `f`. Parameter 0 of every signature is the receiver,
    /// which is the whole trick -- it makes a method call an ordinary tuple for
    /// M3c's symmetric dispatch, so single dispatch needs no new machinery.
    methods: HashMap<String, Vec<Signature>>,
    /// Every name declared `getter` or `setter` anywhere in the component, so a
    /// read of one is reported as an accessor rather than as a missing field.
    accessors: HashSet<String>,
    /// Every functional method that takes static parameters. It is not lifted,
    /// so a call reaches an empty overload set; naming the mechanism is what
    /// keeps the file out of the `unknown name` bucket.
    generic_functional: HashSet<String>,
    /// Which member of which method set each declaration is, keyed by the
    /// owner and the member's index in that owner's member list. The pair is
    /// unique by construction and reads the same in both passes, so it cannot
    /// desynchronise the way a running positional index would.
    ///
    /// The start offset this used to be keyed by is NOT unique: two
    /// instantiations of one generic type clone the same members, so
    /// `Cell[\ZZ32\].get` and `Cell[\String\].get` carry the same span and
    /// the second silently overwrote the first.
    method_slots: HashMap<(&'static str, usize), (String, usize)>,
    /// The same, for functional methods, whose slots index into `functions`
    /// rather than `methods` because that is the set 1.0 lifts them into.
    functional_slots: HashMap<(&'static str, usize), (String, usize)>,
    self_ctx: Option<SelfCtx>,
    /// Which member of which overload set each function declaration is, in
    /// declaration order. Backpatching an inferred return type by name alone
    /// would land it on the wrong overload.
    slots: Vec<(String, usize)>,
    scopes: Vec<HashMap<String, Local>>,
    uses_mpi: bool,
    dispatches: BTreeMap<String, DispatchFn>,
    /// Set while an object's field initializers are checked. They run when the
    /// object is built -- for a singleton, before `run` -- so they may not
    /// reach a singleton, a user function or another constructor. That is what
    /// makes construction order a non-question instead of a null dereference.
    object_init: bool,
}

/// A method lives in its own namespace, so its symbol must not be able to
/// collide with a function's. `mangle` joins with `$`, so `$m$` cannot be
/// produced by it: a Fortress name is never empty.
///
/// One type may declare the same method name at more than one arity or
/// parameter type -- a legitimate overload -- so the parameters go into the
/// symbol exactly as they do for a function. Leaving them out gave both
/// members one symbol, and codegen defined the second against the first's
/// declaration.
fn method_symbol(receiver: &str, name: &str, params: &[Type], overloaded: bool) -> String {
    let base = format!("{receiver}$m${name}");
    if overloaded {
        mangle(&base, params)
    } else {
        base
    }
}

fn mangle(name: &str, params: &[Type]) -> String {
    let mut out = String::from(name);
    out.push('$');
    for (index, ty) in params.iter().enumerate() {
        if index > 0 {
            out.push('_');
        }
        out.push_str(ty.symbol());
    }
    out
}

/// A functional method lifts into the TOP-LEVEL overload set of its name, so
/// its symbol has to be one `mangle` cannot produce: `mangle` joins with a
/// single `$`, so `$f$` is unreachable and a real top-level `f` is safe.
///
/// Always owner qualified, never bare -- a bare one collides with that top
/// level `f` outright.
fn functional_symbol(owner: &str, name: &str, params: &[Type], overloaded: bool) -> String {
    let base = format!("{owner}$f${name}");
    if overloaded {
        mangle(&base, params)
    } else {
        base
    }
}

fn constructor_symbol(name: &str) -> String {
    format!("{name}$new")
}

fn render(types: &[Type]) -> String {
    types
        .iter()
        .map(|t| t.name())
        .collect::<Vec<&str>>()
        .join(", ")
}

/// Pointwise subtyping, strict in at least one position. The whole specificity
/// order is this one function.
fn strictly_below(a: &[Type], b: &[Type], registry: &Registry) -> bool {
    a != b && a.iter().zip(b).all(|(x, y)| registry.is_subtype(*x, *y))
}

fn more_specific(a: &Signature, b: &Signature, registry: &Registry) -> bool {
    strictly_below(&a.params, &b.params, registry)
}

fn members_of(decl: &Decl) -> &[Member] {
    match decl {
        Decl::Trait(t) => &t.members,
        Decl::Object(o) => &o.members,
        Decl::Function(_) => &[],
    }
}

/// A member with a `self` parameter, which is what makes it a *functional*
/// method: 1.0 invokes it `f(x, y)` and never `x.f(y)`. A generic one is not
/// lifted, so it is not one of these.
fn is_functional(m: &MethodDecl) -> bool {
    !m.accessor && m.static_params.is_empty() && m.params.iter().any(|p| p.name == "self")
}

/// How many declarations share each functional name. Top-level declarations
/// and functional methods are ONE overload set, so the count has to span both:
/// counting only the top-level ones gave two members one symbol, and codegen
/// defined the second against the first's declaration.
fn overload_counts(component: &Component) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for decl in &component.decls {
        if let Decl::Function(f) = decl {
            *counts.entry(f.name.clone()).or_default() += 1;
            continue;
        }
        for member in members_of(decl) {
            let Member::Method(m) = member else { continue };
            if is_functional(m) {
                *counts.entry(m.name.clone()).or_default() += 1;
            }
        }
    }
    counts
}

/// `Self` inside a member stands for the type that declares it. For an object
/// owner that is the object; for a trait owner it is the trait, which is what
/// closed-world dispatch supports and is a stated deviation from 1.0, where
/// `Self` is the run-time type of the receiver.
fn substitute_self(t: &TypeRef, owner: &str) -> TypeRef {
    match t {
        TypeRef::Named { name, args, span } if name == "Self" && args.is_empty() => {
            TypeRef::Named {
                name: owner.to_owned(),
                args: Vec::new(),
                span: *span,
            }
        }
        TypeRef::Named { name, args, span } => TypeRef::Named {
            name: name.clone(),
            args: args.iter().map(|a| substitute_self(a, owner)).collect(),
            span: *span,
        },
        TypeRef::Unit { .. } => t.clone(),
        TypeRef::Tuple { elems, span } => TypeRef::Tuple {
            elems: elems.iter().map(|e| substitute_self(e, owner)).collect(),
            span: *span,
        },
        TypeRef::Arrow { from, to, span } => TypeRef::Arrow {
            from: Box::new(substitute_self(from, owner)),
            to: Box::new(substitute_self(to, owner)),
            span: *span,
        },
    }
}

/// A member of an overload set this call could still reach. A withdrawn stamp
/// is not merely off the target list: it must not reach `agreed` either, or its
/// wrongly instantiated parameter types poison the hint a literal takes and the
/// program is blamed for a guess expansion made.
fn live(signature: &Signature, arity: usize) -> bool {
    !signature.pruned && signature.params.len() == arity
}

/// The operator as the source wrote it, for diagnostics.
const fn op_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::Eq => "=",
        BinOp::Ne => "=/=",
        BinOp::And => "AND",
        BinOp::Or => "OR",
    }
}

fn cartesian(domain: &[Vec<Type>]) -> Vec<Vec<Type>> {
    let mut rows: Vec<Vec<Type>> = vec![Vec::new()];
    for column in domain {
        let mut next = Vec::with_capacity(rows.len().saturating_mul(column.len()));
        for prefix in &rows {
            for value in column {
                let mut row = prefix.clone();
                row.push(*value);
                next.push(row);
            }
        }
        rows = next;
    }
    rows
}

impl Checker {
    /// Pass one: every type name, then the hierarchy, then every signature.
    /// Nothing is resolved against a type until all the names are known, so a
    /// forward reference to something declared further down the file works.
    fn new(component: &Component) -> Checked<Self> {
        let mut registry = Registry::default();
        let mut declared: HashMap<&'static str, Span> = HashMap::new();

        for decl in &component.decls {
            let (name, span) = match decl {
                Decl::Trait(t) => (intern(&t.name), t.span),
                Decl::Object(o) => (intern(&o.name), o.span),
                Decl::Function(_) => continue,
            };
            if declared.insert(name, span).is_some() {
                return Err(TypeError::DuplicateDefinition {
                    span,
                    name: name.to_owned(),
                });
            }
            match decl {
                Decl::Trait(_) => {
                    registry.traits.insert(
                        name,
                        TraitInfo {
                            supertraits: BTreeSet::new(),
                        },
                    );
                }
                Decl::Object(o) => {
                    let tag = FIRST_TAG
                        .saturating_add(u32::try_from(registry.concrete.len()).unwrap_or(u32::MAX));
                    registry.concrete.push(name);
                    registry.objects.insert(
                        name,
                        ObjectInfo {
                            tag,
                            supertraits: BTreeSet::new(),
                            fields: Vec::new(),
                            param_count: o.params.as_ref().map_or(0, Vec::len),
                            singleton: o.params.is_none(),
                        },
                    );
                }
                Decl::Function(_) => {}
            }
        }

        let mut checker = Self {
            registry,
            functions: HashMap::new(),
            methods: HashMap::new(),
            accessors: HashSet::new(),
            generic_functional: HashSet::new(),
            method_slots: HashMap::new(),
            functional_slots: HashMap::new(),
            self_ctx: None,
            slots: Vec::new(),
            scopes: Vec::new(),
            uses_mpi: false,
            dispatches: BTreeMap::new(),
            object_init: false,
        };
        let counts = overload_counts(component);
        checker.build_hierarchy(component)?;
        checker.build_signatures(component, &declared, &counts)?;
        checker.build_functional_signatures(component, &counts)?;
        checker.build_method_signatures(component)?;
        Ok(checker)
    }

    /// Resolve a type that has to occupy storage. `basic_type(Void)` is `None`,
    /// so a Void here would build a signature or a layout with a hole in it and
    /// fail as an internal error rather than as a diagnostic.
    fn storable(&self, t: &TypeRef, position: &'static str) -> Checked<Type> {
        let ty = self.registry.resolve(t)?;
        if ty == Type::Void {
            return Err(TypeError::VoidNotStorable {
                span: t.span(),
                position,
            });
        }
        Ok(ty)
    }

    fn supertrait(&self, reference: &TypeRef) -> Checked<&'static str> {
        match self.registry.resolve(reference)? {
            Type::Trait(name) => Ok(name),
            _ => Err(TypeError::NotATrait {
                span: reference.span(),
                name: reference.written(),
            }),
        }
    }

    fn build_hierarchy(&mut self, component: &Component) -> Checked<()> {
        let mut direct: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
        let mut spans: BTreeMap<&'static str, Span> = BTreeMap::new();
        for decl in &component.decls {
            let Decl::Trait(t) = decl else { continue };
            let name = intern(&t.name);
            spans.insert(name, t.span);
            let mut supers = Vec::with_capacity(t.extends.len());
            for reference in &t.extends {
                supers.push(self.supertrait(reference)?);
            }
            direct.insert(name, supers);
        }
        for (name, closed) in close_traits(&direct, &spans)? {
            if let Some(info) = self.registry.traits.get_mut(name) {
                info.supertraits = closed;
            }
        }

        for decl in &component.decls {
            let Decl::Object(o) = decl else { continue };
            let name = intern(&o.name);
            let mut supertraits: BTreeSet<&'static str> = BTreeSet::new();
            for reference in &o.extends {
                let above = self.supertrait(reference)?;
                supertraits.insert(above);
                if let Some(info) = self.registry.traits.get(above) {
                    supertraits.extend(info.supertraits.iter().copied());
                }
            }
            let fields = self.object_fields(o)?;
            if let Some(info) = self.registry.objects.get_mut(name) {
                info.supertraits = supertraits;
                info.fields = fields;
            }
        }
        Ok(())
    }

    fn object_fields(&self, o: &ObjectDecl) -> Checked<Vec<TypedField>> {
        let mut fields: Vec<TypedField> = Vec::new();
        for p in o.params.iter().flatten() {
            fields.push(TypedField {
                name: p.name.clone(),
                ty: self.storable(&p.ty, "a field")?,
            });
        }
        for member in &o.members {
            let Member::Field(f) = member else { continue };
            if f.mutable {
                return Err(TypeError::MutableFieldUnsupported {
                    span: f.span,
                    name: f.name.clone(),
                });
            }
            if f.init.is_none() {
                return Err(TypeError::FieldNeedsInitializer {
                    span: f.span,
                    name: f.name.clone(),
                });
            }
            fields.push(TypedField {
                name: f.name.clone(),
                ty: self.storable(&f.ty, "a field")?,
            });
        }
        for (index, field) in fields.iter().enumerate() {
            if fields
                .iter()
                .take(index)
                .any(|other| other.name == field.name)
            {
                return Err(TypeError::DuplicateDefinition {
                    span: o.span,
                    name: field.name.clone(),
                });
            }
        }
        Ok(fields)
    }

    /// Collect every dotted method into a namespace of its own, receiver first.
    ///
    /// Three kinds of member are deliberately left out, and each omission is
    /// load bearing rather than unfinished:
    ///
    /// * an **accessor** is reached by `o.size`, not `o.size()`, so it is not a
    ///   callee at all;
    /// * a member with a **`self` parameter** is a *functional* method, which
    ///   1.0 lifts into the top-level overload set of its name -- a different
    ///   namespace and a different milestone;
    /// * a **bodiless signature** cannot be a dispatch target. Leaving it out
    ///   is what makes an unimplemented abstract method fail the exactly-one-
    ///   winner check that M3c already runs, instead of needing a rule of its
    ///   own.
    fn build_method_signatures(&mut self, component: &Component) -> Checked<()> {
        let mut counts: HashMap<(&str, &str), usize> = HashMap::new();
        for decl in &component.decls {
            let (owner, members) = match decl {
                Decl::Trait(t) => (t.name.as_str(), &t.members),
                Decl::Object(o) => (o.name.as_str(), &o.members),
                Decl::Function(_) => continue,
            };
            for member in members {
                let Member::Method(m) = member else { continue };
                if m.accessor {
                    self.accessors.insert(m.name.clone());
                }
                if m.accessor
                    || m.params.iter().any(|p| p.name == "self")
                    || !m.static_params.is_empty()
                {
                    continue;
                }
                *counts.entry((owner, m.name.as_str())).or_default() += 1;
            }
        }
        for decl in &component.decls {
            let (owner, members) = match decl {
                Decl::Trait(t) => (intern(&t.name), &t.members),
                Decl::Object(o) => (intern(&o.name), &o.members),
                Decl::Function(_) => continue,
            };
            let receiver = if self.registry.is_object(owner) {
                Type::Object(owner)
            } else {
                Type::Trait(owner)
            };
            for (index, member) in members.iter().enumerate() {
                let Member::Method(m) = member else { continue };
                if m.accessor
                    || m.params.iter().any(|p| p.name == "self")
                    || !m.static_params.is_empty()
                {
                    continue;
                }
                // An abstract declaration on a generic trait can mention that
                // trait's static parameter, which is not a type this pass can
                // resolve. It contributes no dispatch target, so skipping it
                // costs nothing; failing the component over it would refuse a
                // program for a signature nothing calls.
                let abstract_ = m.body.is_none();
                let mut params = vec![receiver];
                let mut unresolved = false;
                for p in &m.params {
                    // `Self` stands for the declaring type here exactly as it
                    // does in a functional method. The two kinds disagreeing
                    // about it would be a difference with no reason behind it.
                    let written = substitute_self(&p.ty, owner);
                    match self.storable(&written, "a parameter") {
                        Ok(ty) => params.push(ty),
                        Err(_) if abstract_ => {
                            unresolved = true;
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
                if unresolved {
                    continue;
                }
                let returns = match &m.return_type {
                    Some(t) => match self.registry.resolve(&substitute_self(t, owner)) {
                        Ok(ty) => ty,
                        Err(_) if abstract_ => continue,
                        Err(e) => return Err(e),
                    },
                    None => Type::Void,
                };
                if self
                    .methods
                    .get(&m.name)
                    .is_some_and(|set| set.iter().any(|other| other.params == params))
                {
                    return Err(TypeError::DuplicateDefinition {
                        span: m.span,
                        name: m.name.clone(),
                    });
                }
                let symbol = method_symbol(
                    owner,
                    &m.name,
                    params.get(1..).unwrap_or_default(),
                    counts.get(&(owner, m.name.as_str())).copied().unwrap_or(0) > 1,
                );
                let set = self.methods.entry(m.name.clone()).or_default();
                let slot = (m.name.clone(), set.len());
                self.method_slots.insert((owner, index), slot);
                set.push(Signature {
                    params,
                    returns,
                    concrete: !abstract_,
                    symbol,
                    pruned: false,
                    span: m.span,
                });
            }
        }
        Ok(())
    }

    /// Lift every functional method into the top-level overload set of its
    /// name. That is the namespace 1.0 puts it in, and it is *not* the dotted
    /// one: `x.f(y)` and `f(x, y)` are different declarations.
    ///
    /// `self` keeps its WRITTEN position. `area(self, k: ZZ32)` lifts to
    /// `(Owner, ZZ32)` and `foo(x: ZZ32, self)` to `(ZZ32, Owner)`; symmetric
    /// dispatch does not care which column holds the interesting type, so
    /// forcing the receiver to position 0 would be code with a chance of being
    /// wrong and no chance of being right in a new way.
    fn build_functional_signatures(
        &mut self,
        component: &Component,
        counts: &HashMap<String, usize>,
    ) -> Checked<()> {
        for decl in &component.decls {
            let owner = match decl {
                Decl::Trait(t) => intern(&t.name),
                Decl::Object(o) => intern(&o.name),
                Decl::Function(_) => continue,
            };
            for (index, member) in members_of(decl).iter().enumerate() {
                let Member::Method(m) = member else { continue };
                if m.accessor || !m.params.iter().any(|p| p.name == "self") {
                    continue;
                }
                if !m.static_params.is_empty() {
                    self.generic_functional.insert(m.name.clone());
                    continue;
                }
                // Same concession as the dotted set: an abstract declaration
                // may mention a type this pass cannot resolve, and it
                // contributes no dispatch target, so it is skipped rather than
                // refusing a program for a signature nothing calls.
                let abstract_ = m.body.is_none();
                let mut params = Vec::with_capacity(m.params.len());
                let mut unresolved = false;
                // The parser gives the `self` parameter the written type
                // `Self`, so it needs no case of its own: one substitution
                // covers the receiver, `x: Self` in another position, and the
                // return type alike.
                for p in &m.params {
                    let written = substitute_self(&p.ty, owner);
                    match self.storable(&written, "a parameter") {
                        Ok(ty) => params.push(ty),
                        Err(_) if abstract_ => {
                            unresolved = true;
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
                if unresolved {
                    continue;
                }
                let returns = match &m.return_type {
                    Some(t) => {
                        let written = substitute_self(t, owner);
                        match self.registry.resolve(&written) {
                            Ok(ty) => ty,
                            Err(_) if abstract_ => continue,
                            Err(e) => return Err(e),
                        }
                    }
                    None => Type::Void,
                };
                if self
                    .functions
                    .get(&m.name)
                    .is_some_and(|set| set.iter().any(|other| other.params == params))
                {
                    return Err(TypeError::DuplicateDefinition {
                        span: m.span,
                        name: m.name.clone(),
                    });
                }
                let symbol = functional_symbol(
                    owner,
                    &m.name,
                    &params,
                    counts.get(&m.name).copied().unwrap_or(0) > 1,
                );
                let set = self.functions.entry(m.name.clone()).or_default();
                self.functional_slots
                    .insert((owner, index), (m.name.clone(), set.len()));
                set.push(Signature {
                    params,
                    returns,
                    concrete: m.body.is_some(),
                    symbol,
                    pruned: false,
                    span: m.span,
                });
            }
        }
        Ok(())
    }

    fn build_signatures(
        &mut self,
        component: &Component,
        declared: &HashMap<&'static str, Span>,
        counts: &HashMap<String, usize>,
    ) -> Checked<()> {
        let mut raw: Vec<(String, Vec<Type>, Type, Span)> = Vec::new();
        for decl in &component.decls {
            let Decl::Function(f) = decl else { continue };
            if declared.contains_key(intern(&f.name)) {
                return Err(TypeError::DuplicateDefinition {
                    span: f.span,
                    name: f.name.clone(),
                });
            }
            let mut params = Vec::with_capacity(f.params.len());
            for p in &f.params {
                params.push(self.storable(&p.ty, "a parameter")?);
            }
            let returns = match &f.return_type {
                Some(t) => self.registry.resolve(t)?,
                // Inferred in pass two; Void until then, and overwritten there.
                None => Type::Void,
            };
            raw.push((f.name.clone(), params, returns, f.span));
        }

        for (name, params, returns, span) in raw {
            let overloaded = counts.get(&name).copied().unwrap_or(0) > 1;
            let symbol = if overloaded {
                mangle(&name, &params)
            } else {
                name.clone()
            };
            let set = self.functions.entry(name.clone()).or_default();
            if set.iter().any(|other| other.params == params) {
                return Err(TypeError::DuplicateDefinition { span, name });
            }
            self.slots.push((name, set.len()));
            set.push(Signature {
                params,
                returns,
                concrete: true,
                symbol,
                pruned: false,
                span,
            });
        }
        Ok(())
    }

    fn run(mut self, component: &Component) -> Checked<TypedComponent> {
        if component.is_api {
            return Err(TypeError::ApiNotExecutable {
                span: component.span,
            });
        }
        for decl in &component.decls {
            let Decl::Function(f) = decl else { continue };
            if f.value_binding {
                return Err(TypeError::ValueBindingUnsupported {
                    span: f.span,
                    name: f.name.clone(),
                });
            }
            if f.name == "run" && !f.params.is_empty() {
                return Err(TypeError::EntryPointTakesArguments {
                    span: f.span,
                    found: f.params.len(),
                });
            }
        }
        self.discharge_bounds(component)?;

        let mut objects = Vec::new();
        for decl in &component.decls {
            if let Decl::Object(o) = decl {
                objects.push(self.object(o)?);
            }
        }

        let mut functions = Vec::new();
        let mut index = 0usize;
        for decl in &component.decls {
            if let Decl::Function(f) = decl {
                functions.push(self.function(f, index)?);
                index += 1;
            }
        }

        // Lifted methods are ordinary functions from here down, which is why
        // codegen needed no change for this milestone.
        for decl in &component.decls {
            let (owner, members) = match decl {
                Decl::Trait(t) => (intern(&t.name), &t.members),
                Decl::Object(o) => (intern(&o.name), &o.members),
                Decl::Function(_) => continue,
            };
            for (index, member) in members.iter().enumerate() {
                let Member::Method(m) = member else { continue };
                if m.accessor
                    || m.params.iter().any(|p| p.name == "self")
                    || !m.static_params.is_empty()
                {
                    continue;
                }
                if m.body.is_none() || !self.method_slots.contains_key(&(owner, index)) {
                    continue;
                }
                if self.pruned_method(owner, index) {
                    continue;
                }
                functions.push(self.method(m, owner, index)?);
            }
        }

        // And functional methods, which are members of the top-level overload
        // set rather than of a namespace of their own. Same lift, same reason
        // codegen needed no change: what comes out is a `TypedFn`.
        for decl in &component.decls {
            let owner = match decl {
                Decl::Trait(t) => intern(&t.name),
                Decl::Object(o) => intern(&o.name),
                Decl::Function(_) => continue,
            };
            for (index, member) in members_of(decl).iter().enumerate() {
                let Member::Method(m) = member else { continue };
                if !is_functional(m) {
                    continue;
                }
                if m.body.is_none() || !self.functional_slots.contains_key(&(owner, index)) {
                    continue;
                }
                functions.push(self.functional_method(m, owner, index)?);
            }
        }

        Ok(TypedComponent {
            name: component.name.clone(),
            exports: component.exports.clone(),
            objects,
            functions,
            dispatches: self.dispatches.into_values().collect(),
            uses_mpi: self.uses_mpi,
        })
    }

    /// The obligations monomorphization recorded. They could not be settled
    /// there -- subtyping needs the registry, and the registry is built from the
    /// component expansion produced -- so they are settled here, before a single
    /// body is checked.
    fn discharge_bounds(&mut self, component: &Component) -> Checked<()> {
        for obligation in &component.bounds {
            let resolved = self
                .registry
                .resolve(&obligation.subject)
                .and_then(|subject| Ok((subject, self.registry.resolve(&obligation.bound)?)));
            let failure = match resolved {
                Ok((subject, bound)) if self.registry.is_subtype(subject, bound) => continue,
                Ok((subject, bound)) => Some(TypeError::BoundNotSatisfied {
                    span: obligation.span,
                    parameter: obligation.parameter.clone(),
                    subject,
                    bound,
                }),
                Err(e) => Some(e),
            };
            // A speculative obligation belongs to a stamp expansion guessed at,
            // and it runs here -- before any body is checked and before any
            // dispatch table is memoised -- precisely so the guess can be
            // withdrawn without refusing the program. A call whose receiver
            // domain includes the pruned type then fails the exactly-one-winner
            // check M3c already runs, which is the closed-world answer.
            if let Some((owner, method)) = &obligation.speculative {
                self.prune_stamp(owner, method);
                continue;
            }
            if let Some(e) = failure {
                return Err(e);
            }
        }
        Ok(())
    }

    fn prune_stamp(&mut self, owner: &str, method: &str) {
        let receiver = if self.registry.is_object(owner) {
            self.registry
                .objects
                .get_key_value(owner)
                .map(|(n, _)| Type::Object(n))
        } else {
            self.registry
                .traits
                .get_key_value(owner)
                .map(|(n, _)| Type::Trait(n))
        };
        let Some(receiver) = receiver else { return };
        let Some(set) = self.methods.get_mut(method) else {
            return;
        };
        for signature in set.iter_mut() {
            if signature.params.first() == Some(&receiver) {
                signature.pruned = true;
                signature.concrete = false;
            }
        }
    }

    fn object(&mut self, o: &ObjectDecl) -> Checked<TypedObject> {
        let name = intern(&o.name);
        let Some(info) = self.registry.objects.get(name) else {
            return Err(TypeError::UnknownType {
                span: o.span,
                name: o.name.clone(),
            });
        };
        let (tag, fields, param_count, singleton) = (
            info.tag,
            info.fields.clone(),
            info.param_count,
            info.singleton,
        );

        let mut scope = HashMap::new();
        for field in fields.iter().take(param_count) {
            scope.insert(
                field.name.clone(),
                Local {
                    ty: field.ty,
                    mutable: false,
                },
            );
        }
        self.scopes.push(scope);
        self.object_init = true;
        let built = self.initializers(o, &fields, param_count);
        self.object_init = false;
        self.scopes.pop();

        Ok(TypedObject {
            name,
            tag,
            symbol: constructor_symbol(name),
            fields,
            param_count,
            initializers: built?,
            singleton,
            span: o.span,
        })
    }

    fn initializers(
        &mut self,
        o: &ObjectDecl,
        fields: &[TypedField],
        param_count: usize,
    ) -> Checked<Vec<TypedExpr>> {
        let body: Vec<&FieldDecl> = o
            .members
            .iter()
            .filter_map(|m| match m {
                Member::Field(f) => Some(f),
                Member::Method(_) => None,
            })
            .collect();
        let mut out = Vec::with_capacity(body.len());
        for (decl, field) in body.iter().zip(fields.iter().skip(param_count)) {
            let Some(init) = &decl.init else {
                return Err(TypeError::FieldNeedsInitializer {
                    span: decl.span,
                    name: decl.name.clone(),
                });
            };
            let value = self.expr(init, Some(field.ty))?;
            self.declare(field.name.clone(), field.ty, false);
            out.push(value);
        }
        Ok(out)
    }

    fn function(&mut self, f: &FnDecl, index: usize) -> Checked<TypedFn> {
        let Some(source) = &f.body else {
            return Err(TypeError::MissingBody {
                span: f.span,
                name: f.name.clone(),
            });
        };
        let Some((set, slot)) = self.slots.get(index).cloned() else {
            return Err(TypeError::UnknownName {
                span: f.span,
                name: f.name.clone(),
            });
        };
        let Some(signature) = self.functions.get(&set).and_then(|v| v.get(slot)) else {
            return Err(TypeError::UnknownName {
                span: f.span,
                name: f.name.clone(),
            });
        };
        let symbol = signature.symbol.clone();
        let declared = f.return_type.is_some().then_some(signature.returns);
        let types = signature.params.clone();

        let mut params = Vec::with_capacity(f.params.len());
        let mut scope = HashMap::new();
        for (p, ty) in f.params.iter().zip(types) {
            scope.insert(p.name.clone(), Local { ty, mutable: false });
            params.push(TypedParam {
                name: p.name.clone(),
                ty,
                span: p.span,
            });
        }

        self.scopes.push(scope);
        let body = self.expr(source, declared);
        self.scopes.pop();
        let body = body?;

        let return_type = declared.unwrap_or(body.ty);
        if let Some(sig) = self.functions.get_mut(&set).and_then(|v| v.get_mut(slot)) {
            sig.returns = return_type;
        }
        Ok(TypedFn {
            name: symbol,
            params,
            return_type,
            body,
            span: f.span,
        })
    }

    /// A stamp withdrawn by `discharge_bounds`. Its body is never checked:
    /// nothing can reach it, and it was written under a substitution the
    /// program never asked for.
    fn pruned_method(&self, owner: &'static str, index: usize) -> bool {
        self.method_slots
            .get(&(owner, index))
            .and_then(|(set, slot)| self.methods.get(set).and_then(|v| v.get(*slot)))
            .is_some_and(|signature| signature.pruned)
    }

    /// One dotted method body, lifted to a `TypedFn` whose first parameter is
    /// the receiver. Codegen needs no new node: a lifted method is a function,
    /// and a method call is a `DispatchFn` over its tuple.
    fn method(&mut self, m: &MethodDecl, owner: &'static str, index: usize) -> Checked<TypedFn> {
        let Some(source) = &m.body else {
            return Err(TypeError::MissingBody {
                span: m.span,
                name: m.name.clone(),
            });
        };
        let Some((set, slot)) = self.method_slots.get(&(owner, index)).cloned() else {
            return Err(TypeError::UnknownName {
                span: m.span,
                name: m.name.clone(),
            });
        };
        let Some(signature) = self.methods.get(&set).and_then(|v| v.get(slot)) else {
            return Err(TypeError::UnknownName {
                span: m.span,
                name: m.name.clone(),
            });
        };
        let symbol = signature.symbol.clone();
        let declared = m.return_type.is_some().then_some(signature.returns);
        let types = signature.params.clone();
        let Some(&receiver) = types.first() else {
            return Err(TypeError::UnknownName {
                span: m.span,
                name: m.name.clone(),
            });
        };

        let mut params = vec![TypedParam {
            name: "self".to_owned(),
            ty: receiver,
            span: m.span,
        }];
        let mut scope = HashMap::new();
        scope.insert(
            "self".to_owned(),
            Local {
                ty: receiver,
                mutable: false,
            },
        );
        for (p, ty) in m.params.iter().zip(types.into_iter().skip(1)) {
            scope.insert(p.name.clone(), Local { ty, mutable: false });
            params.push(TypedParam {
                name: p.name.clone(),
                ty,
                span: p.span,
            });
        }

        // An object's method sees its fields. A trait has none, so a default
        // body there can only reach its parameters and `self`.
        let fields = self
            .registry
            .objects
            .get(owner)
            .map_or_else(Vec::new, |o| o.fields.clone());
        let previous = self.self_ctx.replace(SelfCtx {
            ty: receiver,
            fields,
        });
        self.scopes.push(scope);
        let body = self.expr(source, declared);
        self.scopes.pop();
        self.self_ctx = previous;
        let body = body?;

        let return_type = declared.unwrap_or(body.ty);
        if let Some(sig) = self.methods.get_mut(&set).and_then(|v| v.get_mut(slot)) {
            sig.returns = return_type;
        }
        Ok(TypedFn {
            name: symbol,
            params,
            return_type,
            body,
            span: m.span,
        })
    }

    /// One functional method body, lifted to a `TypedFn` whose parameters are
    /// exactly what was written -- `self` included, in the position the source
    /// put it in.
    fn functional_method(
        &mut self,
        m: &MethodDecl,
        owner: &'static str,
        index: usize,
    ) -> Checked<TypedFn> {
        let Some(source) = &m.body else {
            return Err(TypeError::MissingBody {
                span: m.span,
                name: m.name.clone(),
            });
        };
        let Some((set, slot)) = self.functional_slots.get(&(owner, index)).cloned() else {
            return Err(TypeError::UnknownName {
                span: m.span,
                name: m.name.clone(),
            });
        };
        let Some(signature) = self.functions.get(&set).and_then(|v| v.get(slot)) else {
            return Err(TypeError::UnknownName {
                span: m.span,
                name: m.name.clone(),
            });
        };
        let symbol = signature.symbol.clone();
        let declared = m.return_type.is_some().then_some(signature.returns);
        let types = signature.params.clone();

        let mut receiver = None;
        let mut params = Vec::with_capacity(m.params.len());
        let mut scope = HashMap::new();
        for (p, ty) in m.params.iter().zip(types) {
            if p.name == "self" {
                receiver = Some(ty);
            }
            scope.insert(p.name.clone(), Local { ty, mutable: false });
            params.push(TypedParam {
                name: p.name.clone(),
                ty,
                span: p.span,
            });
        }
        let Some(receiver) = receiver else {
            return Err(TypeError::UnknownName {
                span: m.span,
                name: m.name.clone(),
            });
        };

        // An object's method sees its fields, whichever namespace it lifts
        // into. `f(self): S = x` reading the constructor parameter `x` is
        // ordinary Fortress and it is what `compiler_tests/Compiled17.fss`
        // writes.
        let fields = self
            .registry
            .objects
            .get(owner)
            .map_or_else(Vec::new, |o| o.fields.clone());
        let previous = self.self_ctx.replace(SelfCtx {
            ty: receiver,
            fields,
        });
        self.scopes.push(scope);
        let body = self.expr(source, declared);
        self.scopes.pop();
        self.self_ctx = previous;
        let body = body?;

        let return_type = declared.unwrap_or(body.ty);
        if let Some(sig) = self.functions.get_mut(&set).and_then(|v| v.get_mut(slot)) {
            sig.returns = return_type;
        }
        Ok(TypedFn {
            name: symbol,
            params,
            return_type,
            body,
            span: m.span,
        })
    }

    /// `o.m(y)`. The receiver is checked first and becomes argument 0, after
    /// which this is an ordinary overload resolution over the method namespace.
    fn method_call(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        span: Span,
        dot_span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let receiver = self.expr(base, None)?;
        self.dispatch_method(receiver, name, args, span, dot_span, expected)
    }

    /// Shared by `o.m(y)` and by the unqualified `m(y)` inside a method body.
    fn dispatch_method(
        &mut self,
        receiver: TypedExpr,
        name: &str,
        args: &[Expr],
        span: Span,
        dot_span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let Some(all) = self.methods.get(name) else {
            return Err(TypeError::DottedMethodUnsupported {
                span: dot_span,
                name: name.to_owned(),
            });
        };
        let arity = args.len() + 1;
        let candidates: Vec<Signature> = all.iter().filter(|s| live(s, arity)).cloned().collect();
        if candidates.is_empty() {
            return Err(TypeError::ArityMismatch {
                span,
                name: name.to_owned(),
                expected: all.first().map_or(1, |s| s.params.len()) - 1,
                found: args.len(),
            });
        }

        let mut typed = Vec::with_capacity(arity);
        typed.push(receiver);
        for (index, arg) in args.iter().enumerate() {
            let hint = agreed(&candidates, index + 1);
            typed.push(self.expr(arg, hint)?);
        }

        let statics: Vec<Type> = typed.iter().map(|t| t.ty).collect();
        let refs: Vec<&Signature> = candidates.iter().collect();
        let applicable = self.typing_candidates(&refs, &statics);
        if applicable.is_empty() {
            return Err(TypeError::NoApplicableDeclaration {
                span,
                name: name.to_owned(),
                arguments: render(&statics),
            });
        }
        let returns = self.winner(name, &applicable, &statics, span)?.returns;
        let target = self.dispatch_target(name, &candidates, &statics, returns, span)?;
        self.require(returns, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target,
                args: typed,
            },
            ty: returns,
            span,
        })
    }

    /// `m(y)` written inside a method body, meaning `self.m(y)`.
    fn self_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        callee_span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let Some(ctx) = &self.self_ctx else {
            return Err(TypeError::UnknownName {
                span: callee_span,
                name: name.to_owned(),
            });
        };
        let receiver = TypedExpr {
            kind: TypedExprKind::Var("self".to_owned()),
            ty: ctx.ty,
            span: callee_span,
        };
        self.dispatch_method(receiver, name, args, span, callee_span, expected)
    }

    // -------------------------------------------------------------- scopes

    fn lookup(&self, name: &str) -> Option<Local> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn declare(&mut self, name: String, ty: Type, mutable: bool) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, Local { ty, mutable });
        }
    }

    // --------------------------------------------------------- expressions

    /// `expected` is the context. It pins literals and it is what values are
    /// checked against; it is never used to convert anything.
    fn expr(&mut self, e: &Expr, expected: Option<Type>) -> Checked<TypedExpr> {
        match e {
            Expr::Unit { span } => {
                self.require(Type::Void, expected, *span)?;
                Ok(TypedExpr {
                    kind: TypedExprKind::Unit,
                    ty: Type::Void,
                    span: *span,
                })
            }
            Expr::Tuple { span, .. } => Err(TypeError::TypeNotImplemented {
                span: *span,
                form: "a tuple expression",
            }),
            Expr::IntLit { digits, span } => self.int_literal(digits, *span, expected),
            Expr::FloatLit {
                int_digits,
                frac_digits,
                span,
            } => {
                let text = format!("{int_digits}.{frac_digits}");
                let value = text.parse::<f64>().unwrap_or(f64::NAN);
                let typed = TypedExpr {
                    kind: TypedExprKind::FloatConst(value),
                    ty: Type::RR64,
                    span: *span,
                };
                self.require(typed.ty, expected, *span)?;
                Ok(typed)
            }
            Expr::StrLit { value, span } => {
                self.require(Type::String, expected, *span)?;
                Ok(TypedExpr {
                    kind: TypedExprKind::StrConst(value.clone()),
                    ty: Type::String,
                    span: *span,
                })
            }
            Expr::BoolLit { value, span } => {
                self.require(Type::Boolean, expected, *span)?;
                Ok(TypedExpr {
                    kind: TypedExprKind::BoolConst(*value),
                    ty: Type::Boolean,
                    span: *span,
                })
            }
            Expr::Var { name, span } => self.variable(name, *span, expected),
            Expr::Prefix { op, operand, span } => self.prefix(*op, operand, *span, expected),
            Expr::Infix {
                op, lhs, rhs, span, ..
            } => self.infix(*op, lhs, rhs, *span, expected),
            Expr::Juxt { items, span } => self.juxtaposition(items, *span, expected),
            Expr::Call { callee, args, span } => self.call(callee, args, *span, expected),
            Expr::If {
                cond,
                then_branch,
                else_branch,
                span,
            } => self.if_expr(cond, then_branch, else_branch.as_deref(), *span, expected),
            Expr::Block { items, span } => self.block(items, *span, expected),
            Expr::ArrayLit { items, span } => self.array_literal(items, *span, expected),
            Expr::Index { base, index, span } => self.index(base, index, *span, expected),
            Expr::While { cond, body, span } => self.while_expr(cond, body, *span, expected),
            Expr::Field { base, name, span } => self.field(base, name, *span, expected),
            // Unreachable: expansion rewrites every instantiation to a plain
            // name before the checker is constructed.
            Expr::Instantiate { callee, span, .. } => Err(TypeError::NotGeneric {
                span: *span,
                name: match callee.as_ref() {
                    Expr::Var { name, .. } => name.clone(),
                    _ => "<expression>".to_owned(),
                },
            }),
        }
    }

    /// A name in scope, or -- failing that -- a singleton object, which is the
    /// only kind of type name that is also a value.
    fn variable(&mut self, name: &str, span: Span, expected: Option<Type>) -> Checked<TypedExpr> {
        if let Some(local) = self.lookup(name) {
            self.require(local.ty, expected, span)?;
            return Ok(TypedExpr {
                kind: TypedExprKind::Var(name.to_owned()),
                ty: local.ty,
                span,
            });
        }
        // Inside a method body a bare name may be one of the receiver's
        // fields. Locals win, which is the shadowing rule a parameter needs.
        if let Some(ctx) = &self.self_ctx {
            if let Some((index, field)) =
                ctx.fields.iter().enumerate().find(|(_, f)| f.name == name)
            {
                let ty = field.ty;
                let receiver = ctx.ty;
                self.require(ty, expected, span)?;
                return Ok(TypedExpr {
                    kind: TypedExprKind::Field {
                        base: Box::new(TypedExpr {
                            kind: TypedExprKind::Var("self".to_owned()),
                            ty: receiver,
                            span,
                        }),
                        index: index as u32,
                    },
                    ty,
                    span,
                });
            }
        }
        let Some((interned, info)) = self.registry.objects.get_key_value(name) else {
            return Err(TypeError::UnknownName {
                span,
                name: name.to_owned(),
            });
        };
        if !info.singleton {
            return Err(TypeError::ArityMismatch {
                span,
                name: name.to_owned(),
                expected: info.param_count,
                found: 0,
            });
        }
        if self.object_init {
            return Err(TypeError::SingletonInitializerRestricted {
                span,
                name: name.to_owned(),
            });
        }
        let ty = Type::Object(interned);
        self.require(ty, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Singleton { name: interned },
            ty,
            span,
        })
    }

    fn field(
        &mut self,
        base: &Expr,
        name: &str,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let base = self.expr(base, None)?;
        // A getter is read exactly like a field, so a program reaching here for
        // an accessor's name is not asking for a field that does not exist --
        // it is asking for a getter, which parses and is not implemented.
        // Saying "has no field" would send it to the wrong bucket, the way the
        // static-argument catch-all did in M3g.
        if self.accessors.contains(name) {
            return Err(TypeError::AccessorUnsupported {
                span,
                name: name.to_owned(),
            });
        }
        let unknown = || TypeError::UnknownField {
            span,
            found: base.ty,
            name: name.to_owned(),
        };
        let Type::Object(object) = base.ty else {
            return Err(unknown());
        };
        let Some((index, ty)) = self.registry.field(object, name) else {
            return Err(unknown());
        };
        self.require(ty, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Field {
                base: Box::new(base),
                index,
            },
            ty,
            span,
        })
    }

    /// The element type comes from the first element that can supply one, or
    /// from the slot the literal lands in. A literal of bare integers with
    /// neither defaults to ZZ32, exactly as a bare integer literal does.
    fn array_literal(
        &mut self,
        items: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let mut elem = match expected {
            Some(Type::Array(e)) => Some(e),
            _ => None,
        };
        if elem.is_none() {
            for item in items {
                if is_int_literal(item) {
                    continue;
                }
                let probe = self.expr(item, None)?;
                elem = Elem::of(probe.ty);
                if elem.is_none() {
                    return Err(TypeError::UnsupportedElementType {
                        span,
                        name: probe.ty.name().to_owned(),
                    });
                }
                break;
            }
        }
        let elem = match elem {
            Some(e) => e,
            None if items.is_empty() => return Err(TypeError::ElementTypeUnknown { span }),
            // Nothing but literals: the same default a bare literal takes.
            None => Elem::ZZ32,
        };

        let mut typed = Vec::with_capacity(items.len());
        for item in items {
            typed.push(self.expr(item, Some(elem.as_type()))?);
        }
        let ty = Type::Array(elem);
        self.require(ty, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::ArrayLit { elem, items: typed },
            ty,
            span,
        })
    }

    fn index(
        &mut self,
        base: &Expr,
        index: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let base = self.expr(base, None)?;
        let Type::Array(elem) = base.ty else {
            return Err(TypeError::NotAnArray {
                span,
                found: base.ty,
            });
        };
        // Subscripts are ZZ64 so that an array can be longer than 2^31, which
        // is the ceiling the JVM implementation could never get past.
        let index = self.expr(index, Some(Type::ZZ64))?;
        let ty = elem.as_type();
        self.require(ty, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
                elem,
            },
            ty,
            span,
        })
    }

    fn while_expr(
        &mut self,
        cond: &Expr,
        body: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let cond_typed = self.expr(cond, Some(Type::Boolean)).map_err(|e| match e {
            TypeError::Mismatch { span, found, .. }
            | TypeError::LiteralNotApplicable {
                span,
                required: found,
            } => TypeError::ConditionNotBoolean { span, found },
            other => other,
        })?;
        let body_typed = self.expr(body, None)?;
        self.require(Type::Void, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::While {
                cond: Box::new(cond_typed),
                body: Box::new(body_typed),
            },
            ty: Type::Void,
            span,
        })
    }

    fn assign(&mut self, a: &Assign) -> Checked<TypedBlockItem> {
        match &a.target {
            Expr::Var { name, span } => {
                let local = self
                    .lookup(name)
                    .ok_or_else(|| TypeError::AssignToUndeclared {
                        span: *span,
                        name: name.clone(),
                    })?;
                if !local.mutable {
                    return Err(TypeError::AssignToImmutable {
                        span: *span,
                        name: name.clone(),
                    });
                }
                let value = self.expr(&a.value, Some(local.ty))?;
                Ok(TypedBlockItem::Assign {
                    target: AssignTarget::Var {
                        name: name.clone(),
                        ty: local.ty,
                    },
                    value,
                    span: a.span,
                })
            }
            // The binding is immutable, the container is not: `a` cannot be
            // rebound, but its elements are storage.
            Expr::Index { base, index, span } => {
                let base = self.expr(base, None)?;
                let Type::Array(elem) = base.ty else {
                    return Err(TypeError::NotAnArray {
                        span: *span,
                        found: base.ty,
                    });
                };
                let index = self.expr(index, Some(Type::ZZ64))?;
                let value = self.expr(&a.value, Some(elem.as_type()))?;
                Ok(TypedBlockItem::Assign {
                    target: AssignTarget::Element {
                        base: Box::new(base),
                        index: Box::new(index),
                        elem,
                    },
                    value,
                    span: a.span,
                })
            }
            other => Err(TypeError::InvalidAssignTarget { span: other.span() }),
        }
    }

    /// The literal rule. An integer literal has no type of its own; the slot it
    /// lands in decides. With no slot it is `ZZ32`, Fortress's default integer.
    fn int_literal(&self, digits: &str, span: Span, expected: Option<Type>) -> Checked<TypedExpr> {
        let ty = match expected {
            None => Type::ZZ32,
            Some(t) if t.is_integer() => t,
            Some(Type::RR64) => Type::RR64,
            Some(other) => {
                return Err(TypeError::LiteralNotApplicable {
                    span,
                    required: other,
                })
            }
        };
        let value: i128 = digits
            .parse()
            .map_err(|_| TypeError::LiteralOutOfRange { span, ty })?;
        let fits = match ty {
            Type::ZZ32 => i128::from(i32::MIN) <= value && value <= i128::from(i32::MAX),
            Type::ZZ64 | Type::RR64 => {
                i128::from(i64::MIN) <= value && value <= i128::from(i64::MAX)
            }
            _ => false,
        };
        if !fits {
            return Err(TypeError::LiteralOutOfRange { span, ty });
        }
        Ok(TypedExpr {
            // A literal that took RR64 from context is a float constant. Left
            // an IntConst it reaches `arith`, which requires a float value and
            // has no conversion in between: `halve(x: RR64): RR64 = x/2` is
            // ordinary Fortress and it panicked.
            kind: if ty == Type::RR64 {
                TypedExprKind::FloatConst(value as f64)
            } else {
                TypedExprKind::IntConst(value)
            },
            ty,
            span,
        })
    }

    fn prefix(
        &mut self,
        op: UnOp,
        operand: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        if op == UnOp::Not {
            return self.negation(operand, span, expected);
        }
        let inner = self.expr(operand, expected)?;
        if !inner.ty.is_numeric() {
            return Err(TypeError::Mismatch {
                span,
                found: inner.ty,
                required: Type::ZZ64,
            });
        }
        let ty = inner.ty;
        match op {
            // Unary plus is the identity; it does not survive into codegen.
            UnOp::Pos => Ok(TypedExpr {
                kind: inner.kind,
                ty,
                span,
            }),
            UnOp::Neg => Ok(TypedExpr {
                kind: TypedExprKind::Apply {
                    target: Target::Negate { ty },
                    args: vec![inner],
                },
                ty,
                span,
            }),
            // Routed above, before the operand is checked against a numeric
            // type it was never going to have.
            UnOp::Not => Err(TypeError::LogicalOperandNotBoolean {
                span,
                op: "NOT",
                found: ty,
            }),
        }
    }

    /// `NOT b`. One `xor` and no branch: `NOT` does not short circuit, and
    /// three basic blocks for one instruction is worse code at `-O0`, which is
    /// where this project checks its claims.
    fn negation(
        &mut self,
        operand: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let inner = self
            .expr(operand, Some(Type::Boolean))
            .map_err(|e| match e {
                TypeError::Mismatch { span, found, .. }
                | TypeError::LiteralNotApplicable {
                    span,
                    required: found,
                } => TypeError::LogicalOperandNotBoolean {
                    span,
                    op: "NOT",
                    found,
                },
                other => other,
            })?;
        if inner.ty != Type::Boolean {
            return Err(TypeError::LogicalOperandNotBoolean {
                span,
                op: "NOT",
                found: inner.ty,
            });
        }
        self.require(Type::Boolean, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::Not,
                args: vec![inner],
            },
            ty: Type::Boolean,
            span,
        })
    }

    /// `a AND b` and `a OR b`, which SHORT CIRCUIT.
    ///
    /// The construct that already emits a conditional branch, two blocks and a
    /// phi is `If`, so that is what these become -- after both operands are
    /// checked as Boolean, so the diagnostic names the operator instead of
    /// talking about an `if` the user did not write. Desugaring in the parser
    /// would have been cheaper and would have reported the wrong mechanism.
    fn logical(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let name = if op == BinOp::And { "AND" } else { "OR" };
        let operand = |checker: &mut Self, e: &Expr| -> Checked<TypedExpr> {
            let typed = checker
                .expr(e, Some(Type::Boolean))
                .map_err(|err| match err {
                    TypeError::Mismatch { span, found, .. }
                    | TypeError::LiteralNotApplicable {
                        span,
                        required: found,
                    } => TypeError::LogicalOperandNotBoolean {
                        span,
                        op: name,
                        found,
                    },
                    other => other,
                })?;
            if typed.ty == Type::Boolean {
                return Ok(typed);
            }
            Err(TypeError::LogicalOperandNotBoolean {
                span: typed.span,
                op: name,
                found: typed.ty,
            })
        };
        let left = operand(self, lhs)?;
        let right = operand(self, rhs)?;
        let constant = |value: bool| TypedExpr {
            kind: TypedExprKind::BoolConst(value),
            ty: Type::Boolean,
            span,
        };
        let (then_branch, else_branch) = if op == BinOp::And {
            (right, constant(false))
        } else {
            (constant(true), right)
        };
        self.require(Type::Boolean, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::If {
                cond: Box::new(left),
                then_branch: Box::new(then_branch),
                else_branch: Some(Box::new(else_branch)),
            },
            ty: Type::Boolean,
            span,
        })
    }

    fn infix(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        if matches!(op, BinOp::And | BinOp::Or) {
            return self.logical(op, lhs, rhs, span, expected);
        }
        let comparison = matches!(
            op,
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne
        );
        // A comparison's result type says nothing about its operands.
        let operand_hint = if comparison { None } else { expected };

        // If the left side is a bare literal it cannot supply a type, so the
        // right side goes first and supplies one instead.
        let (left, right) = if operand_hint.is_none() && is_int_literal(lhs) && !is_int_literal(rhs)
        {
            let right = self.expr(rhs, None)?;
            let left = self.expr(lhs, Some(right.ty))?;
            (left, right)
        } else {
            let left = self.expr(lhs, operand_hint)?;
            let right = self.expr(rhs, Some(left.ty))?;
            (left, right)
        };

        if left.ty != right.ty {
            return Err(TypeError::MixedNumericOperands {
                span,
                left: left.ty,
                right: right.ty,
            });
        }
        // Equality is defined on Boolean and is the same `icmp` the numeric
        // path emits. Ordering is not defined on it, and inventing one would
        // be a silently wrong answer rather than a missing feature.
        if left.ty == Type::Boolean {
            if !matches!(op, BinOp::Eq | BinOp::Ne) {
                return Err(TypeError::BooleanNotOrdered {
                    span,
                    op: op_name(op),
                });
            }
        } else if !left.ty.is_numeric() {
            return Err(TypeError::Mismatch {
                span,
                found: left.ty,
                required: Type::ZZ64,
            });
        }

        let (target, ty) = match op {
            BinOp::Add => (
                Target::Arith {
                    op: ArithOp::Add,
                    ty: left.ty,
                },
                left.ty,
            ),
            BinOp::Sub => (
                Target::Arith {
                    op: ArithOp::Sub,
                    ty: left.ty,
                },
                left.ty,
            ),
            BinOp::Mul => (
                Target::Arith {
                    op: ArithOp::Mul,
                    ty: left.ty,
                },
                left.ty,
            ),
            BinOp::Div => (
                Target::Arith {
                    op: ArithOp::Div,
                    ty: left.ty,
                },
                left.ty,
            ),
            BinOp::Lt => (
                Target::Compare {
                    op: CompareOp::Lt,
                    ty: left.ty,
                },
                Type::Boolean,
            ),
            BinOp::Gt => (
                Target::Compare {
                    op: CompareOp::Gt,
                    ty: left.ty,
                },
                Type::Boolean,
            ),
            BinOp::Le => (
                Target::Compare {
                    op: CompareOp::Le,
                    ty: left.ty,
                },
                Type::Boolean,
            ),
            BinOp::Ge => (
                Target::Compare {
                    op: CompareOp::Ge,
                    ty: left.ty,
                },
                Type::Boolean,
            ),
            BinOp::Eq => (
                Target::Compare {
                    op: CompareOp::Eq,
                    ty: left.ty,
                },
                Type::Boolean,
            ),
            BinOp::Ne => (
                Target::Compare {
                    op: CompareOp::Ne,
                    ty: left.ty,
                },
                Type::Boolean,
            ),
            // Routed above, before either operand is checked.
            BinOp::And | BinOp::Or => {
                return Err(TypeError::LogicalOperandNotBoolean {
                    span,
                    op: op_name(op),
                    found: left.ty,
                })
            }
        };
        self.require(ty, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target,
                args: vec![left, right],
            },
            ty,
            span,
        })
    }

    /// Specification rule (c), `juxtameaning.tex:44-46`: an identifier with no
    /// visible declaration is a function element. `lookup` is what "visible
    /// declaration" means here, and it is the whole guard -- a local or a
    /// parameter sharing a name with a function is a value, so `f y` stays
    /// multiplication. A singleton object is a value too (`Self::variable`), so
    /// only a constructible object counts.
    fn is_function_element(&self, name: &str) -> bool {
        if self.lookup(name).is_some() {
            return false;
        }
        MpiOp::from_name(name).is_some()
            || matches!(name, "widen" | "println" | "array" | "length")
            || self
                .registry
                .objects
                .get(name)
                .is_some_and(|info| !info.singleton)
            || self.functions.contains_key(name)
    }

    /// The fold. A juxtaposition is multiplication when every operand is the
    /// same numeric type, and concatenation when any operand is a string.
    /// Nothing else resolves.
    fn juxtaposition(
        &mut self,
        items: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        // Application first, and only on a leading function element: every
        // juxtaposition that resolved before this milestone still takes the
        // same path. The probe loop below is what reports `unknown name
        // println`, so this has to run ahead of it.
        if let Some((callee, args)) = items.split_first() {
            if let Expr::Var { name, .. } = callee {
                if self.is_function_element(name) {
                    // `f ()` is the nullary call: in Fortress a zero-argument
                    // function's argument is the unit value.
                    if let [Expr::Unit { .. }] = args {
                        return self.call(callee, &[], span, expected);
                    }
                    if args.len() != 1 {
                        return Err(TypeError::JuxtapositionNotBinary {
                            span,
                            found: items.len(),
                        });
                    }
                    return self.call(callee, args, span, expected);
                }
            }
        }

        // Literals cannot supply a type, so the non-literal operands go first.
        let mut discovered: Option<Type> = None;
        let mut has_string = false;
        for item in items {
            if is_int_literal(item) {
                continue;
            }
            let probe = self.expr(item, None)?;
            if probe.ty == Type::String {
                has_string = true;
            }
            if discovered.is_none() {
                discovered = Some(probe.ty);
            }
        }

        if has_string {
            return self.concatenation(items, span, expected);
        }

        let ty = discovered.or(expected).unwrap_or(Type::ZZ32);
        if !ty.is_numeric() {
            return Err(TypeError::UnresolvableJuxtaposition {
                span,
                left: ty,
                right: ty,
            });
        }

        let mut typed = Vec::with_capacity(items.len());
        for item in items {
            // Only literals take the hint. A value that disagrees is reported
            // as a juxtaposition problem rather than as a generic mismatch,
            // because neither operand is "the required" one.
            let t = if is_int_literal(item) {
                self.expr(item, Some(ty))?
            } else {
                self.expr(item, None)?
            };
            if t.ty != ty {
                return Err(TypeError::MixedNumericOperands {
                    span,
                    left: ty,
                    right: t.ty,
                });
            }
            typed.push(t);
        }

        let mut folded = typed
            .drain(..1)
            .next()
            .ok_or(TypeError::UnresolvableJuxtaposition {
                span,
                left: ty,
                right: ty,
            })?;
        for next in typed {
            folded = TypedExpr {
                kind: TypedExprKind::Apply {
                    target: Target::Arith {
                        op: ArithOp::Mul,
                        ty,
                    },
                    args: vec![folded, next],
                },
                ty,
                span,
            };
        }
        self.require(ty, expected, span)?;
        Ok(folded)
    }

    /// String juxtaposition. Non-string operands get an explicit `to_string_*`
    /// target: that is what concatenation is defined to do, and it is not a
    /// widening, so it does not violate the no-implicit-conversion rule.
    fn concatenation(
        &mut self,
        items: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let mut parts = Vec::with_capacity(items.len());
        for item in items {
            let t = self.expr(item, None)?;
            parts.push(if t.ty == Type::String {
                t
            } else {
                let from = t.ty;
                TypedExpr {
                    kind: TypedExprKind::Apply {
                        target: Target::ToString { from },
                        args: vec![t],
                    },
                    ty: Type::String,
                    span,
                }
            });
        }

        let mut folded = parts
            .drain(..1)
            .next()
            .ok_or(TypeError::UnresolvableJuxtaposition {
                span,
                left: Type::String,
                right: Type::String,
            })?;
        for next in parts {
            folded = TypedExpr {
                kind: TypedExprKind::Apply {
                    target: Target::Concat,
                    args: vec![folded, next],
                },
                ty: Type::String,
                span,
            };
        }
        self.require(Type::String, expected, span)?;
        Ok(folded)
    }

    fn call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        // `x.f(y)` is a dotted method, which the specification gives its own
        // namespace and its own shadowing rules. It is not `f(x, y)`.
        if let Expr::Field {
            base,
            name,
            span: dot_span,
        } = callee
        {
            return self.method_call(base, name, args, span, *dot_span, expected);
        }
        let Expr::Var {
            name,
            span: callee_span,
        } = callee
        else {
            return Err(TypeError::UnknownName {
                span,
                name: "<expression>".to_owned(),
            });
        };

        if let Some(op) = MpiOp::from_name(name) {
            return self.mpi(op, args, span, expected);
        }
        // The builtins keep precedence over user declarations, exactly as
        // before M3c; a user function named `println` is unreachable.
        match name.as_str() {
            "widen" => self.widen(args, span, expected),
            "println" => self.println(args, span, expected),
            "array" => self.array_new(args, span, expected),
            "length" => self.array_length(args, span, expected),
            _ if self.registry.is_object(name) => {
                self.construct(name, args, span, *callee_span, expected)
            }
            _ => self.user_call(name, args, span, *callee_span, expected),
        }
    }

    fn construct(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        callee_span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let Some((interned, info)) = self.registry.objects.get_key_value(name) else {
            return Err(TypeError::UnknownName {
                span: callee_span,
                name: name.to_owned(),
            });
        };
        if info.singleton {
            return Err(TypeError::SingletonNotConstructible {
                span: callee_span,
                name: name.to_owned(),
            });
        }
        let interned = *interned;
        let wanted: Vec<Type> = info
            .fields
            .iter()
            .take(info.param_count)
            .map(|f| f.ty)
            .collect();
        if wanted.len() != args.len() {
            return Err(TypeError::ArityMismatch {
                span,
                name: name.to_owned(),
                expected: wanted.len(),
                found: args.len(),
            });
        }
        if self.object_init {
            return Err(TypeError::SingletonInitializerRestricted {
                span,
                name: name.to_owned(),
            });
        }
        let mut typed = Vec::with_capacity(args.len());
        for (arg, want) in args.iter().zip(wanted) {
            typed.push(self.expr(arg, Some(want))?);
        }
        let ty = Type::Object(interned);
        self.require(ty, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::ObjectNew {
                    symbol: constructor_symbol(interned),
                },
                args: typed,
            },
            ty,
            span,
        })
    }

    /// A call to an overload set. Everything that can be decided statically is,
    /// and what is left is one tag load and one switch per undecided position.
    fn user_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        callee_span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        // Inside a method body an unqualified call may be a method on the
        // receiver: 1.0 lets `m()` mean `self.m()`. A top-level function of the
        // same name wins, which is the shadowing direction the two namespaces
        // already imply.
        if !self.functions.contains_key(name)
            && self.methods.contains_key(name)
            && self.self_ctx.is_some()
        {
            return self.self_call(name, args, span, callee_span, expected);
        }
        let Some(all) = self.functions.get(name) else {
            if self.generic_functional.contains(name) {
                return Err(TypeError::GenericFunctionalMethodUnsupported {
                    span: callee_span,
                    name: name.to_owned(),
                });
            }
            return Err(TypeError::UnknownName {
                span: callee_span,
                name: name.to_owned(),
            });
        };
        let candidates: Vec<Signature> = all
            .iter()
            .filter(|s| live(s, args.len()))
            .cloned()
            .collect();
        if candidates.is_empty() {
            return Err(TypeError::ArityMismatch {
                span,
                name: name.to_owned(),
                expected: all.first().map_or(0, |s| s.params.len()),
                found: args.len(),
            });
        }
        if self.object_init {
            return Err(TypeError::SingletonInitializerRestricted {
                span,
                name: name.to_owned(),
            });
        }

        // A position takes a hint only where every candidate agrees on it.
        // With one candidate that is the whole signature, which is what the
        // literal rule did before overloading existed.
        let mut typed = Vec::with_capacity(args.len());
        for (index, arg) in args.iter().enumerate() {
            let hint = agreed(&candidates, index);
            typed.push(self.expr(arg, hint)?);
        }

        let statics: Vec<Type> = typed.iter().map(|t| t.ty).collect();
        let refs: Vec<&Signature> = candidates.iter().collect();
        let applicable = self.typing_candidates(&refs, &statics);
        if applicable.is_empty() {
            return Err(TypeError::NoApplicableDeclaration {
                span,
                name: name.to_owned(),
                arguments: render(&statics),
            });
        }
        // The statically computed return type: every cell's winner has to
        // return a subtype of this one. Its existence is also what makes the
        // table total, because a declaration applicable to the static tuple is
        // applicable to every concrete tuple beneath it.
        let returns = self.winner(name, &applicable, &statics, span)?.returns;

        let target = self.dispatch_target(name, &candidates, &statics, returns, span)?;
        self.require(returns, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target,
                args: typed,
            },
            ty: returns,
            span,
        })
    }

    /// Applicable to one argument tuple. `targets_only` is what separates the
    /// two questions M3i's design note already distinguished but the code did
    /// not: a bodiless declaration TYPES a call and names its return, and can
    /// never BE a dispatch target.
    fn applicable<'s>(
        &self,
        candidates: &[&'s Signature],
        arguments: &[Type],
        targets_only: bool,
    ) -> Vec<&'s Signature> {
        candidates
            .iter()
            .copied()
            .filter(|c| {
                (c.concrete || !targets_only)
                    && c.params
                        .iter()
                        .zip(arguments)
                        .all(|(p, a)| self.registry.is_subtype(*a, *p))
            })
            .collect()
    }

    /// What types the call. Implementations first, and a bodiless declaration
    /// only when there is no implementation at all -- which is the whole of the
    /// rule "a requirement types a call and never wins one".
    ///
    /// Both halves are load bearing. Taking targets only made `o.f()` on a
    /// trait-typed `o` refuse whenever the trait declared `f` abstractly and
    /// the objects beneath it implemented it -- ordinary Fortress, and the
    /// shape `compiler_tests/Compiled15.fss` is written in. Taking everything
    /// made an inherited implementation tie with an inherited *requirement*
    /// and reported an ambiguity that is not one, which is
    /// `long_term_not_working/abstract/DiamondInheritance7.fss`.
    fn typing_candidates<'s>(
        &self,
        candidates: &[&'s Signature],
        arguments: &[Type],
    ) -> Vec<&'s Signature> {
        let targets = self.applicable(candidates, arguments, true);
        if targets.is_empty() {
            return self.applicable(candidates, arguments, false);
        }
        targets
    }

    /// The single most specific applicable declaration. Specification 1.0 would
    /// pick one of the maximal declarations arbitrarily here; an arbitrary
    /// winner is a silently wrong answer, so this refuses instead.
    fn winner<'s>(
        &self,
        name: &str,
        applicable: &[&'s Signature],
        arguments: &[Type],
        span: Span,
    ) -> Checked<&'s Signature> {
        let maximal: Vec<&'s Signature> = applicable
            .iter()
            .copied()
            .filter(|c| {
                !applicable
                    .iter()
                    .any(|other| more_specific(other, c, &self.registry))
            })
            .collect();
        if maximal.len() != 1 {
            let mut tied = maximal.iter();
            return match (tied.next(), tied.next()) {
                (Some(first), Some(second)) => Err(TypeError::AmbiguousDispatch {
                    span,
                    name: name.to_owned(),
                    arguments: render(arguments),
                    first: first.span,
                    second: second.span,
                }),
                _ => Err(TypeError::NoApplicableDeclaration {
                    span,
                    name: name.to_owned(),
                    arguments: render(arguments),
                }),
            };
        }
        maximal
            .first()
            .copied()
            .ok_or_else(|| TypeError::NoApplicableDeclaration {
                span,
                name: name.to_owned(),
                arguments: render(arguments),
            })
    }

    /// Enumerate every concrete tuple that can reach this call, decide each one
    /// statically, and flatten the result into a decision tree. A tree that
    /// collapses to a single winner is a plain direct call, which is what the
    /// overwhelming majority of call sites get.
    fn dispatch_target(
        &mut self,
        name: &str,
        candidates: &[Signature],
        statics: &[Type],
        returns: Type,
        span: Span,
    ) -> Checked<Target> {
        // One candidate is one winner in every cell, so there is nothing to
        // enumerate and no size to bound. This is the whole pre-M3c language.
        // It has to be a real target: a lone bodiless declaration would give
        // codegen a symbol nothing defines, which is a link failure rather
        // than a diagnostic.
        if let [only] = candidates {
            if only.concrete {
                return Ok(Target::UserFn {
                    name: only.symbol.clone(),
                });
            }
        }

        let domain: Vec<Vec<Type>> = statics
            .iter()
            .map(|t| match *t {
                Type::Trait(above) => self
                    .registry
                    .concretes_below(above)
                    .into_iter()
                    .map(Type::Object)
                    .collect(),
                concrete => vec![concrete],
            })
            .collect();

        let cells = domain
            .iter()
            .try_fold(1usize, |total, column| total.checked_mul(column.len()))
            .unwrap_or(usize::MAX);
        if cells > MAX_DISPATCH_CELLS {
            return Err(TypeError::DispatchTableTooLarge {
                span,
                name: name.to_owned(),
                cells,
            });
        }

        let refs: Vec<&Signature> = candidates.iter().collect();
        let mut table: Vec<(Vec<Type>, String)> = Vec::with_capacity(cells);
        for tuple in cartesian(&domain) {
            let applicable = self.applicable(&refs, &tuple, true);
            let winner = self.winner(name, &applicable, &tuple, span)?;
            if !self.registry.is_subtype(winner.returns, returns) {
                return Err(TypeError::ReturnTypeNotCovariant {
                    span,
                    name: name.to_owned(),
                    arguments: render(&tuple),
                    found: winner.returns,
                    required: returns,
                });
            }
            table.push((tuple, winner.symbol.clone()));
        }

        let positions: Vec<usize> = domain
            .iter()
            .enumerate()
            .filter(|(_, column)| column.len() > 1)
            .map(|(index, _)| index)
            .collect();
        let tree = self.tree(&table, &positions, &domain);

        if let DispatchNode::Call { symbol } = &tree {
            return Ok(Target::UserFn {
                name: symbol.clone(),
            });
        }
        let symbol = format!(
            "{name}$dispatch${}",
            statics
                .iter()
                .map(|t| t.symbol())
                .collect::<Vec<&str>>()
                .join("_")
        );
        self.dispatches
            .entry(symbol.clone())
            .or_insert_with(|| DispatchFn {
                symbol: symbol.clone(),
                set_name: name.to_owned(),
                params: statics.to_vec(),
                returns,
                tree,
            });
        Ok(Target::Dispatch { symbol })
    }

    /// A row whose cells all name the same winner collapses, so the tree is
    /// usually shallower than the arity, and often a single leaf.
    fn tree(
        &self,
        table: &[(Vec<Type>, String)],
        positions: &[usize],
        domain: &[Vec<Type>],
    ) -> DispatchNode {
        if let Some((_, first)) = table.first() {
            if table.iter().all(|(_, symbol)| symbol == first) {
                return DispatchNode::Call {
                    symbol: first.clone(),
                };
            }
        }
        // An empty table means the trait has no concrete implementor, so no
        // value can reach this call. The arms are empty and the fail arm halts.
        let Some((&position, rest)) = positions.split_first() else {
            return DispatchNode::Switch {
                position: empty_column(domain),
                arms: Vec::new(),
            };
        };
        let mut arms = Vec::new();
        for candidate in domain.get(position).into_iter().flatten() {
            let Type::Object(object) = *candidate else {
                continue;
            };
            let Some(tag) = self.registry.tag_of(object) else {
                continue;
            };
            let subset: Vec<(Vec<Type>, String)> = table
                .iter()
                .filter(|(tuple, _)| tuple.get(position) == Some(candidate))
                .cloned()
                .collect();
            if subset.is_empty() {
                continue;
            }
            arms.push((tag, self.tree(&subset, rest, domain)));
        }
        DispatchNode::Switch { position, arms }
    }

    /// The MPI builtins. All four take no arguments: the communicator is fixed
    /// to `MPI_COMM_WORLD` inside the shim, because its expansion is
    /// implementation specific and must not reach generated code.
    fn mpi(
        &mut self,
        op: MpiOp,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        if !args.is_empty() {
            return Err(TypeError::ArityMismatch {
                span,
                name: op.name().to_owned(),
                expected: 0,
                found: args.len(),
            });
        }
        let ty = op.returns();
        self.require(ty, expected, span)?;
        self.uses_mpi = true;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::Mpi(op),
                args: Vec::new(),
            },
            ty,
            span,
        })
    }

    /// `array(n)`. There is nothing in the call to say what it holds, so the
    /// element type comes from the slot and its absence is a diagnostic rather
    /// than a guess.
    fn array_new(
        &mut self,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let [count] = args else {
            return Err(TypeError::ArityMismatch {
                span,
                name: "array".to_owned(),
                expected: 1,
                found: args.len(),
            });
        };
        let Some(Type::Array(elem)) = expected else {
            return Err(TypeError::ElementTypeUnknown { span });
        };
        // `array(n)` hands back slots nothing has written, so every element type
        // it accepts needs a value the runtime can legitimately put there.
        // `fortress_array_alloc` writes a one-byte static "" into pointer slots,
        // which is a valid String and nothing else -- an object slot read before
        // it is written would give dispatch a tag load four bytes into that
        // one-byte object. This is an allowlist on purpose: widening `Elem` to
        // reference types has to come back through here.
        if elem.is_pointer() && elem != Elem::String {
            return Err(TypeError::UninitialisedArrayOfReferences {
                span,
                found: elem.as_type(),
            });
        }
        let count = self.expr(count, Some(Type::ZZ64))?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::ArrayNew { elem },
                args: vec![count],
            },
            ty: Type::Array(elem),
            span,
        })
    }

    fn array_length(
        &mut self,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let [array] = args else {
            return Err(TypeError::ArityMismatch {
                span,
                name: "length".to_owned(),
                expected: 1,
                found: args.len(),
            });
        };
        let array = self.expr(array, None)?;
        if !matches!(array.ty, Type::Array(_)) {
            return Err(TypeError::NotAnArray {
                span,
                found: array.ty,
            });
        }
        self.require(Type::ZZ64, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::ArrayLength,
                args: vec![array],
            },
            ty: Type::ZZ64,
            span,
        })
    }

    /// The only numeric conversion in M1, and the only way to get one.
    fn widen(&mut self, args: &[Expr], span: Span, expected: Option<Type>) -> Checked<TypedExpr> {
        let [arg] = args else {
            return Err(TypeError::ArityMismatch {
                span,
                name: "widen".to_owned(),
                expected: 1,
                found: args.len(),
            });
        };
        let inner = self.expr(arg, Some(Type::ZZ32))?;
        self.require(Type::ZZ64, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::Widen {
                    from: Type::ZZ32,
                    to: Type::ZZ64,
                },
                args: vec![inner],
            },
            ty: Type::ZZ64,
            span,
        })
    }

    fn println(&mut self, args: &[Expr], span: Span, expected: Option<Type>) -> Checked<TypedExpr> {
        let [arg] = args else {
            return Err(TypeError::ArityMismatch {
                span,
                name: "println".to_owned(),
                expected: 1,
                found: args.len(),
            });
        };
        let inner = self.expr(arg, None)?;
        let ty = inner.ty;
        // There is one shim per scalar and none for anything else. Saying so
        // here is a diagnostic; leaving it to codegen is an internal error.
        if ty != Type::Void && Elem::of(ty).is_none() {
            return Err(TypeError::NotPrintable { span, found: ty });
        }
        self.require(Type::Void, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::Println { ty },
                args: vec![inner],
            },
            ty: Type::Void,
            span,
        })
    }

    fn if_expr(
        &mut self,
        cond: &Expr,
        then_branch: &Expr,
        else_branch: Option<&Expr>,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let cond_typed = self.expr(cond, Some(Type::Boolean)).map_err(|e| match e {
            TypeError::Mismatch { span, found, .. } => {
                TypeError::ConditionNotBoolean { span, found }
            }
            other => other,
        })?;

        let then_typed = self.expr(then_branch, expected)?;
        let Some(else_expr) = else_branch else {
            if expected.is_some_and(|t| t != Type::Void) || then_typed.ty != Type::Void {
                return Err(TypeError::MissingElseBranch { span });
            }
            return Ok(TypedExpr {
                kind: TypedExprKind::If {
                    cond: Box::new(cond_typed),
                    then_branch: Box::new(then_typed),
                    else_branch: None,
                },
                ty: Type::Void,
                span,
            });
        };

        let else_typed = self.expr(else_expr, expected.or(Some(then_typed.ty)))?;
        // With a context the branches were both checked against it, and it is
        // the type of the whole expression -- which is how one arm can be an
        // Alpha and the other a Beta under a shared trait. Without one they
        // have to agree exactly.
        if expected.is_none() && then_typed.ty != else_typed.ty {
            return Err(TypeError::BranchTypeMismatch {
                span,
                then_type: then_typed.ty,
                else_type: else_typed.ty,
            });
        }
        let ty = expected.unwrap_or(then_typed.ty);
        Ok(TypedExpr {
            kind: TypedExprKind::If {
                cond: Box::new(cond_typed),
                then_branch: Box::new(then_typed),
                else_branch: Some(Box::new(else_typed)),
            },
            ty,
            span,
        })
    }

    fn block(
        &mut self,
        items: &[BlockItem],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        self.scopes.push(HashMap::new());
        let result = self.block_inner(items, span, expected);
        self.scopes.pop();
        result
    }

    /// Checks a computed type against its context. This is where the
    /// no-implicit-widening rule is enforced, and it never converts anything:
    /// an object in a trait slot stays the object it was.
    fn require(&self, found: Type, expected: Option<Type>, span: Span) -> Checked<()> {
        match expected {
            None => Ok(()),
            Some(want) if self.registry.is_subtype(found, want) => Ok(()),
            Some(want) if want.is_widening_of(found) => Err(TypeError::ImplicitWideningRejected {
                span,
                from: found,
                to: want,
            }),
            Some(want) => Err(TypeError::Mismatch {
                span,
                found,
                required: want,
            }),
        }
    }

    fn block_inner(
        &mut self,
        items: &[BlockItem],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let mut typed = Vec::new();
        let last = items.len().saturating_sub(1);

        for (index, item) in items.iter().enumerate() {
            match item {
                BlockItem::Binding(b) => {
                    let declared = match &b.ty {
                        Some(t) => Some(self.registry.resolve(t)?),
                        None => None,
                    };
                    let value = self.expr(&b.value, declared)?;
                    let ty = declared.unwrap_or(value.ty);
                    if ty == Type::Void {
                        return Err(TypeError::VoidNotStorable {
                            span: b.span,
                            position: "a binding",
                        });
                    }
                    self.declare(b.name.clone(), ty, b.mutable);
                    typed.push(TypedBlockItem::Binding {
                        name: b.name.clone(),
                        ty,
                        value,
                        mutable: b.mutable,
                        span: b.span,
                    });
                }
                BlockItem::Assign(a) => typed.push(self.assign(a)?),
                BlockItem::Expr(e) => {
                    // Only the final expression is in value position.
                    let want = if index == last { expected } else { None };
                    let value = self.expr(e, want)?;
                    if index == last {
                        let ty = value.ty;
                        return Ok(TypedExpr {
                            kind: TypedExprKind::Block {
                                items: typed,
                                tail: Some(Box::new(value)),
                            },
                            ty,
                            span,
                        });
                    }
                    typed.push(TypedBlockItem::Expr(value));
                }
            }
        }

        self.require(Type::Void, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Block {
                items: typed,
                tail: None,
            },
            ty: Type::Void,
            span,
        })
    }
}

const fn is_int_literal(e: &Expr) -> bool {
    matches!(e, Expr::IntLit { .. })
}

/// The type every candidate names at this position, when they all name the
/// same one. Anything else leaves the argument to type itself.
fn agreed(candidates: &[Signature], index: usize) -> Option<Type> {
    let mut types = candidates
        .iter()
        .filter_map(|c| c.params.get(index).copied());
    let first = types.next()?;
    types.all(|t| t == first).then_some(first)
}

fn empty_column(domain: &[Vec<Type>]) -> usize {
    domain
        .iter()
        .position(|column| column.is_empty())
        .unwrap_or(0)
}

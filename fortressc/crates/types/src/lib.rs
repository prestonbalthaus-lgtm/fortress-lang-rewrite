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

mod closure;
pub mod comprises;
pub mod deviations;
mod error;
mod mono;
mod registry;
mod types;

pub use mono::{expand, mangle_static, MAX_INSTANTIATIONS};

pub use error::TypeError;
pub use types::{
    intern, intern_types, ArithOp, AssignTarget, CompareOp, DispatchFn, DispatchNode, Elem, MpiOp,
    Target, Type, TypedBlockItem, TypedCapture, TypedComponent, TypedExpr, TypedExprKind,
    TypedField, TypedFn, TypedObject, TypedParam, TypedReduction, TypedTypeCaseArm, ARRAY_ALLOC,
    ARRAY_LENGTH, ARRAY_SLOT, ASSERT_FAILED, ATOMIC_ENTER, ATOMIC_LEAVE, CASE_FAILED,
    DISPATCH_FAILED, ENV_ALLOC, FIRST_TAG, OBJECT_ALLOC, PARALLEL_FOR, REDUCTION_ALLOC,
    REDUCTION_WORKERS,
};

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use fortress_ast::{
    Assign, BinOp, BlockItem, CaseArm, Component, Decl, Expr, FieldDecl, FnDecl, Member,
    MethodDecl, ObjectDecl, Span, TypeCaseArm, TypeRef, UnOp,
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
    // Closure lowering sits BETWEEN the two for the same reason expansion sits
    // before the checker: it appends object declarations, and tags freeze in
    // `Checker::new`. It runs after expansion so it never meets a static
    // parameter -- everything it sees is already ground.
    let ground = closure::lower(&ground)?;
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

/// One parallel loop being checked.
#[derive(Clone)]
struct LoopCtx {
    binder: String,
    /// A `seq(...)` loop runs in order on one thread, so the scope boundary
    /// below does not apply to it. Only a parallel body has to be race free.
    parallel: bool,
    /// How deep the scope stack was when the loop body opened. A lookup that
    /// resolves below this is a capture; at or above it is loop-local.
    floor: usize,
    captures: BTreeMap<String, Type>,
    /// Names from below the floor that the body ASSIGNS. `assign` resolves its
    /// target with `lookup`, which records nothing, so a name the body only
    /// WRITES would never reach the environment at all -- and codegen would
    /// meet an assignment to a name it has no binding for.
    assigned: BTreeMap<String, AssignRecord>,
    /// Loop-LOCAL names that nevertheless name shared storage, because their
    /// initializer read something from below the floor. `b = shared` makes `b`
    /// loop-local by scope and shared by reference, and the depth comparison
    /// alone cannot tell the two apart. A reference type only: copying a
    /// scalar out of a shared name copies its value.
    tainted: BTreeSet<String>,
}

/// One `label` open around the walk. The two depths are what an `exit` is
/// checked against: crossing either of them is a branch the lowering cannot
/// make, and each has its own diagnostic.
struct LabelCtx {
    name: String,
    /// Fixed by the first `exit` carrying a value, or by the context the label
    /// itself was checked in.
    ty: Option<Type>,
    /// `self.atomic_depth` where the label opened. An exit from deeper inside
    /// an `atomic` would branch past `fortress_atomic_leave`.
    atomic_depth: usize,
    /// `self.loop_ctx.len()` where the label opened. Every `for` body is
    /// outlined, so an exit from deeper inside one is a cross-function jump.
    loop_depth: usize,
}

/// What the body did to one escaping name, collected across the whole walk.
/// The verdict cannot be reached at the assignment: reduction.tex:35 asks
/// whether the name is otherwise READ, and the read may not have happened yet.
#[derive(Clone)]
struct AssignRecord {
    ty: Type,
    /// The first assignment, which is what a failed recogniser points at.
    span: Span,
    /// Every assignment to this name was compound -- reduction.tex:30-31. One
    /// `l := e` disqualifies it for good.
    only_compound: bool,
    /// What the partials must be folded with. `+=` and `-=` agree on `Add`;
    /// `*=` is `Mul`. `None` once two assignments disagree, which disqualifies
    /// the name: one accumulator cannot be both.
    merge: Option<ArithOp>,
    /// Every assignment to this name was inside an `atomic`. That is the other
    /// carve-out, and it is the one that actually takes the lock.
    all_atomic: bool,
}

/// One declaration whose return type was INFERRED, and the slot that holds the
/// signature to backpatch. Collected once and walked repeatedly, because the
/// pre-pass is a fixpoint and the filters that decide membership have to read
/// the same in both passes.
enum InferredBody<'a> {
    Function {
        decl: &'a FnDecl,
        index: usize,
    },
    Method {
        decl: &'a MethodDecl,
        owner: &'static str,
        index: usize,
    },
    Functional {
        decl: &'a MethodDecl,
        owner: &'static str,
        index: usize,
    },
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
    /// While a parallel loop body is checked: the binder's name, and the names
    /// the body read from OUTSIDE the loop. The captures are collected here
    /// rather than recomputed by walking the typed tree, because the scope
    /// stack already knows which lookups crossed the boundary.
    loop_ctx: Vec<LoopCtx>,
    /// Set while an object's field initializers are checked. They run when the
    /// object is built -- for a singleton, before `run` -- so they may not
    /// reach a singleton, a user function or another constructor. That is what
    /// makes construction order a non-question instead of a null dereference.
    object_init: bool,
    /// Numbers the outlined loop bodies so their symbols are unique.
    loops: usize,
    /// Numbers the bindings `case` desugars its subject into. `$` cannot be
    /// lexed, so no source name can collide with one.
    cases: usize,
    /// The labels open around the walk, innermost last. A bare `exit` names the
    /// last of them.
    labels: Vec<LabelCtx>,
    /// How deep inside `atomic` the walk is. A parallel body MAY assign to an
    /// escaping name inside one: the lock serialises the write, and the
    /// capture becomes a by-reference one so the write lands on real storage.
    atomic_depth: usize,
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

/// Whether an abstract member may keep a type that did not resolve.
///
/// THE LINE IS BETWEEN A TYPE THIS COMPILER CANNOT REPRESENT AND A TYPE THAT
/// DOES NOT EXIST. The first is our limitation: `ProjectFortress/tests/
/// tupleTypeParam2.fss` instantiates `A[\(ZZ32, ZZ32)\]`, so the instance's
/// abstract `f(x: (ZZ32, ZZ32))` cannot be typed here -- and nothing calls it,
/// because the object that extends it declares its own concrete `f`. Refusing
/// that would refuse a program that runs and prints the right answer.
///
/// The second is the PROGRAM's error, and it used to be accepted in silence.
/// An abstract member is the one place in this compiler where a type is written
/// and read by nothing, so `m(x: Foo): ZZ32` with no `Foo` compiled to exit 0 --
/// which is how the closure pass came to need its own `liftable` guard against
/// exactly this. It is a diagnostic now.
///
/// The original comment here said the concession was for a generic trait's own
/// static parameter. That reason is dead: `mono::emit` never emits a template,
/// only its instances, so a bare static parameter cannot reach this pass at all.
/// What the per-worker partials of a compound assignment are folded with.
///
/// `+=` and `-=` share `Add` on purpose, and it is not a simplification: `-=`
/// accumulates `Identity - e` into a slot that starts at the identity, so the
/// group inverse is ALREADY INSIDE the partial. Folding with `Sub` would take
/// it back out. `*=` is its own, because 0 is not the identity for it and the
/// sum of partial products is not a product.
const fn merge_op(op: ArithOp) -> ArithOp {
    match op {
        ArithOp::Add | ArithOp::Sub => ArithOp::Add,
        ArithOp::Mul => ArithOp::Mul,
        ArithOp::Div => ArithOp::Div,
        // Idempotent and associative, so a partial maximum folds with the same
        // operator that produced it.
        ArithOp::Max => ArithOp::Max,
        ArithOp::Min => ArithOp::Min,
    }
}

fn excusable(e: &TypeError, abstract_: bool) -> bool {
    abstract_
        && matches!(
            e,
            TypeError::TypeNotImplemented { .. } | TypeError::UnsupportedElementType { .. }
        )
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
        // A static value holds no type name, so `Self` cannot occur in one.
        TypeRef::Static { .. } => t.clone(),
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
        BinOp::Max => "MAX",
        BinOp::Min => "MIN",
        BinOp::Pow => "^",
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
            loop_ctx: Vec::new(),
            atomic_depth: 0,
            loops: 0,
            cases: 0,
            labels: Vec::new(),
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

        // A TRAIT'S FIELDS CARRY NO STORAGE HERE and are dropped -- a trait
        // typed value is a pointer to some concrete object, and the object
        // declares its own. Their TYPES were dropped with them, which made
        // `trait T  x: Foo  end` with no `Foo` compile to exit 0 in silence,
        // the same hole an abstract member had. Resolving them changes nothing
        // downstream and refuses that.
        for decl in &component.decls {
            let Decl::Trait(t) = decl else { continue };
            for member in &t.members {
                let Member::Field(f) = member else { continue };
                self.storable(&substitute_self(&f.ty, intern(&t.name)), "a field")?;
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
                mutable: false,
            });
        }
        for member in &o.members {
            let Member::Field(f) = member else { continue };
            if f.init.is_none() {
                return Err(TypeError::FieldNeedsInitializer {
                    span: f.span,
                    name: f.name.clone(),
                });
            }
            fields.push(TypedField {
                name: f.name.clone(),
                ty: self.storable(&f.ty, "a field")?,
                mutable: f.mutable,
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
                let mut unrepresentable = false;
                for p in &m.params {
                    // `Self` stands for the declaring type here exactly as it
                    // does in a functional method. The two kinds disagreeing
                    // about it would be a difference with no reason behind it.
                    let written = substitute_self(&p.ty, owner);
                    match self.storable(&written, "a parameter") {
                        Ok(ty) => params.push(ty),
                        Err(e) if excusable(&e, abstract_) => {
                            unrepresentable = true;
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
                if unrepresentable {
                    continue;
                }
                let returns = match &m.return_type {
                    Some(t) => match self.registry.resolve(&substitute_self(t, owner)) {
                        Ok(ty) => ty,
                        Err(e) if excusable(&e, abstract_) => continue,
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
                let mut unrepresentable = false;
                // The parser gives the `self` parameter the written type
                // `Self`, so it needs no case of its own: one substitution
                // covers the receiver, `x: Self` in another position, and the
                // return type alike.
                for p in &m.params {
                    let written = substitute_self(&p.ty, owner);
                    match self.storable(&written, "a parameter") {
                        Ok(ty) => params.push(ty),
                        Err(e) if excusable(&e, abstract_) => {
                            unrepresentable = true;
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
                if unrepresentable {
                    continue;
                }
                let returns = match &m.return_type {
                    Some(t) => match self.registry.resolve(&substitute_self(t, owner)) {
                        Ok(ty) => ty,
                        Err(e) if excusable(&e, abstract_) => continue,
                        Err(e) => return Err(e),
                    },
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
            return self.check_api(component);
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

        // Before ANY body is checked, including an object initializer's: the
        // signatures a caller reads have to be finished first.
        self.resolve_inferred_returns(component);

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
            is_api: false,
        })
    }

    /// SIGNATURES ONLY -- `SPIKE-API-CHECK-MODE`.
    ///
    /// This replaced an unconditional refusal that was the FIRST STATEMENT of
    /// `run`, before `discharge_bounds` and before any body: a stub sitting
    /// exactly where phase 3 goes. `source-code.tex:313-320` makes an api a set
    /// of top-level declarations without bodies, so "there is nothing to
    /// compile" was true about CODEGEN and wrong about CHECKING. Everything an
    /// api can get wrong is in its headers.
    ///
    /// MOST OF THE WORK IS ALREADY DONE BY THE TIME THIS RUNS, and that is the
    /// point rather than a shortcut. `Checker::new` builds the registry, which
    /// resolves every `extends`, `comprises` and `excludes` name, closes the
    /// trait graph, and computes a `Signature` -- parameter types and return
    /// type, RESOLVED -- for every declaration and every member. That is why an
    /// api has always been able to fail with `unknown type` before reaching
    /// here. What was missing after it is the bound discharge and the
    /// no-bodies rule.
    ///
    /// It does NOT check bodies, resolve inferred returns, build dispatch
    /// tables or emit anything: an api has no bodies to check, no returns to
    /// infer from them, and no code.
    fn check_api(mut self, component: &Component) -> Checked<TypedComponent> {
        self.discharge_bounds(component)?;

        // `source-code.tex:313-320`: an api holds declarations, and a
        // declaration with a body is a definition. The old diagnostic said this
        // about the whole file; it belongs on the one declaration that broke it.
        for decl in &component.decls {
            let Decl::Function(f) = decl else { continue };
            if f.body.is_some() {
                return Err(TypeError::ApiDeclarationHasBody {
                    span: f.span,
                    name: f.name.clone(),
                });
            }
        }
        for decl in &component.decls {
            let (owner, members) = match decl {
                Decl::Trait(t) => (&t.name, &t.members),
                Decl::Object(o) => (&o.name, &o.members),
                Decl::Function(_) => continue,
            };
            for member in members {
                let Member::Method(m) = member else { continue };
                // An ABSTRACT method in an api is ordinary; a method with a
                // body is a definition wherever it is written.
                if m.body.is_some() {
                    return Err(TypeError::ApiDeclarationHasBody {
                        span: m.span,
                        name: format!("{owner}.{}", m.name),
                    });
                }
            }
        }

        self.api_overloads_are_unambiguous()?;

        Ok(TypedComponent {
            name: component.name.clone(),
            exports: component.exports.clone(),
            objects: Vec::new(),
            functions: Vec::new(),
            dispatches: Vec::new(),
            uses_mpi: false,
            is_api: true,
        })
    }

    /// `overloading.tex` makes a valid overload set a property of the
    /// DECLARATIONS, not of the calls that reach them -- and an api has no
    /// calls at all, so M3c's check, which is driven by the tuples a call site
    /// can produce, never runs on one. `Compiled3.w.fsi` is the witness: two
    /// declarations `f(x:O,y:T)` and `f(x:T,y:O)` with `O extends T`, which are
    /// ambiguous at `(O, O)` and which this compiler accepted in silence the
    /// day api check mode landed.
    ///
    /// THE DOMAIN IS SYNTHESIZED FROM THE DECLARATIONS, since there is no call:
    /// each column is every concrete type below any of the declared parameter
    /// types in that position. A set too large to enumerate is SKIPPED rather
    /// than refused -- the bound exists so a big library api cannot be made to
    /// fail by a check that is new, and a missed ambiguity is the state this
    /// replaces rather than a regression from it.
    fn api_overloads_are_unambiguous(&self) -> Checked<()> {
        for (name, sigs) in &self.functions {
            let mut arities: Vec<usize> = sigs.iter().map(|s| s.params.len()).collect();
            arities.sort_unstable();
            arities.dedup();
            for arity in arities {
                let group: Vec<&Signature> =
                    sigs.iter().filter(|s| s.params.len() == arity).collect();
                if group.len() < 2 {
                    continue;
                }
                let mut domain: Vec<Vec<Type>> = Vec::with_capacity(arity);
                for column in 0..arity {
                    let mut here: Vec<Type> = Vec::new();
                    for sig in &group {
                        let Some(param) = sig.params.get(column) else {
                            continue;
                        };
                        match *param {
                            Type::Trait(above) => here.extend(
                                self.registry
                                    .concretes_below(above)
                                    .into_iter()
                                    .map(Type::Object),
                            ),
                            concrete => here.push(concrete),
                        }
                    }
                    here.sort_unstable_by_key(|t| format!("{t:?}"));
                    here.dedup();
                    domain.push(here);
                }
                let cells = domain
                    .iter()
                    .try_fold(1usize, |total, column| total.checked_mul(column.len()))
                    .unwrap_or(usize::MAX);
                if cells == 0 || cells > MAX_DISPATCH_CELLS {
                    continue;
                }
                let span = group.first().map_or(Span::new(0, 0), |s| s.span);
                for tuple in cartesian(&domain) {
                    let applicable = self.typing_candidates(&group, &tuple);
                    if applicable.len() < 2 {
                        continue;
                    }
                    self.winner(name, &applicable, &tuple, span)?;
                }
            }
        }
        Ok(())
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

    /// Pass one and a half: resolve every INFERRED return type before a single
    /// caller body is checked.
    ///
    /// `function`, `method` and `functional_method` learn what a declaration
    /// with no written return type returns by walking its body, and backpatch
    /// `sig.returns` afterwards. That is too late for any call site typed
    /// earlier -- and for a method it is EVERY call site, because method bodies
    /// are checked after every function body whatever the source order. The
    /// caller read the `Void` placeholder `build_signatures` left, so
    /// `println(f())` printed an empty line and exited 0. No diagnostic, and
    /// the compile metric cannot see it: exit 0 is exit 0.
    ///
    /// So the inferred bodies are walked here first, for their signatures only,
    /// and everything else the walk produced is thrown away. This is the same
    /// phase split M3d uses for expansion and M5 for the reduction recogniser,
    /// and it is load bearing for the same reason: a caller must never be able
    /// to observe a half-built signature.
    ///
    /// Three properties make the speculative walk safe:
    ///
    /// * only a declaration that INFERRED its return type is walked. A written
    ///   one is already final, and re-walking it would buy nothing while
    ///   multiplying the state churn below;
    /// * an error is SWALLOWED. Round one reads placeholders, so a body may
    ///   fail against a signature that simply is not resolved yet. Anything
    ///   genuinely wrong recurs in the real pass, which walks every body, and
    ///   the diagnostic comes from there -- with the signatures right, so
    ///   `s: String = O.m()` reports what is actually wrong or compiles;
    /// * every side effect is DISCARDED. `dispatch_target` memoises with
    ///   `or_insert_with`, so a table computed against an unresolved `returns`
    ///   would be the table codegen emitted -- that reset is the one that is
    ///   not optional.
    ///
    /// A round that changes no signature is the fixpoint. Written worst case
    /// -- `a() = b()`, `b() = c()`, `c() = "s"`, in that order -- one round in
    /// declaration order resolves exactly one link, so the cap is the number of
    /// inferred declarations and past it nothing can still be improving.
    /// Reaching the cap is not an error: the real pass runs either way. A
    /// self-recursive inferred function reads its own placeholder here exactly
    /// as it did before, so no diagnostic changes shape for it.
    ///
    /// The cost is |inferred| x |rounds|, and rounds is CHAIN DEPTH IN
    /// DECLARATION ORDER rather than a count -- a callee written above its
    /// caller resolves in one. Measured rather than argued: the 1956-file
    /// corpus sweep does not move (11.15/11.51/11.64s against
    /// 11.62/10.90/11.12s), a 500-link chain written worst-order costs 0.12s
    /// against 0.07s written best-order, and 2000 links cost 0.82s. There is
    /// no cap here because nothing plausible approaches one.
    fn resolve_inferred_returns(&mut self, component: &Component) {
        let pending = self.inferred_bodies(component);
        for _ in 0..pending.len() {
            let before: Vec<Option<Type>> =
                pending.iter().map(|b| self.inferred_return(b)).collect();
            for body in &pending {
                let _ = match body {
                    InferredBody::Function { decl, index } => {
                        self.function(decl, *index).map(|_| ())
                    }
                    InferredBody::Method { decl, owner, index } => {
                        self.method(decl, owner, *index).map(|_| ())
                    }
                    InferredBody::Functional { decl, owner, index } => {
                        self.functional_method(decl, owner, *index).map(|_| ())
                    }
                };
                // A swallowed error can return from the middle of a body, so
                // nothing a body pushed may be assumed to have been popped.
                self.reset_body_state();
            }
            let after: Vec<Option<Type>> =
                pending.iter().map(|b| self.inferred_return(b)).collect();
            if after == before {
                break;
            }
        }
        self.discard_speculative_walk();
    }

    /// The declarations pass one and a half walks. The filters mirror `run`'s
    /// three loops exactly -- a member this skips is a member `run` skips, and
    /// the indices are counted the same way -- because a slot read differently
    /// in the two passes lands the backpatch on the wrong overload.
    fn inferred_bodies<'a>(&self, component: &'a Component) -> Vec<InferredBody<'a>> {
        let mut out = Vec::new();

        let mut index = 0usize;
        for decl in &component.decls {
            if let Decl::Function(f) = decl {
                if f.return_type.is_none() && f.body.is_some() && !f.value_binding {
                    out.push(InferredBody::Function { decl: f, index });
                }
                index += 1;
            }
        }

        for decl in &component.decls {
            let owner = match decl {
                Decl::Trait(t) => intern(&t.name),
                Decl::Object(o) => intern(&o.name),
                Decl::Function(_) => continue,
            };
            for (index, member) in members_of(decl).iter().enumerate() {
                let Member::Method(m) = member else { continue };
                if m.return_type.is_some() || m.body.is_none() {
                    continue;
                }
                if is_functional(m) {
                    if self.functional_slots.contains_key(&(owner, index)) {
                        out.push(InferredBody::Functional {
                            decl: m,
                            owner,
                            index,
                        });
                    }
                    continue;
                }
                if m.accessor || !m.static_params.is_empty() {
                    continue;
                }
                if !self.method_slots.contains_key(&(owner, index))
                    || self.pruned_method(owner, index)
                {
                    continue;
                }
                out.push(InferredBody::Method {
                    decl: m,
                    owner,
                    index,
                });
            }
        }
        out
    }

    /// What one pending declaration's signature currently says it returns.
    /// `None` means the slot is gone, which `run` reports as a diagnostic when
    /// it reaches the same declaration.
    fn inferred_return(&self, body: &InferredBody) -> Option<Type> {
        let (sets, set, slot) = match body {
            InferredBody::Function { index, .. } => {
                let (set, slot) = self.slots.get(*index)?;
                (&self.functions, set, *slot)
            }
            InferredBody::Method { owner, index, .. } => {
                let (set, slot) = self.method_slots.get(&(*owner, *index))?;
                (&self.methods, set, *slot)
            }
            InferredBody::Functional { owner, index, .. } => {
                let (set, slot) = self.functional_slots.get(&(*owner, *index))?;
                (&self.functions, set, *slot)
            }
        };
        sets.get(set)?.get(slot).map(|s| s.returns)
    }

    /// The per-body walk state. Cleared rather than popped, because a swallowed
    /// error may have returned from anywhere inside the body.
    ///
    /// It catches nothing TODAY and the mutation table says so: all six
    /// `scopes.push` sites pop before they propagate, and `atomic` and
    /// `for_expr` do the same for their own state, so a failed body already
    /// unwinds clean. This is the invariant those six sites are keeping, held
    /// in one place -- the first of them to grow an early `?` breaks a
    /// speculative walk and nothing else, which is a defect that would not
    /// surface as a compile error.
    fn reset_body_state(&mut self) {
        self.scopes.clear();
        self.self_ctx = None;
        self.loop_ctx.clear();
        self.atomic_depth = 0;
        self.object_init = false;
    }

    /// Everything pass one and a half produced other than the signatures.
    /// `dispatches` is the one that MUST go: `dispatch_target` memoises with
    /// `or_insert_with`, so the first table computed is the one codegen emits,
    /// and a table built while a `returns` was still `Void` would carry that
    /// hole into the output.
    ///
    /// `loops` and `uses_mpi` are hygiene rather than correctness, and the
    /// mutation table separates them from `dispatches` on exactly that: leaving
    /// `loops` alone renumbers `$loop1` to `$loop5` and the program still
    /// prints the right answer, while leaving `dispatches` alone makes LLVM
    /// reject the module. A symbol that is a function of how many rounds the
    /// fixpoint took is still worth not having.
    fn discard_speculative_walk(&mut self) {
        self.dispatches.clear();
        self.loops = 0;
        self.uses_mpi = false;
        self.reset_body_state();
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

    /// The same lookup, recording a crossing. A name resolved from below the
    /// innermost loop's floor is read by the loop body from the enclosing
    /// scope, so it has to travel to the worker in the environment struct.
    fn lookup_capturing(&mut self, name: &str) -> Option<Local> {
        let found = self
            .scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(depth, s)| s.get(name).map(|local| (depth, *local)));
        let (depth, local) = found?;
        for ctx in &mut self.loop_ctx {
            if depth < ctx.floor && name != ctx.binder {
                ctx.captures.insert(name.to_owned(), local.ty);
            }
        }
        Some(local)
    }

    /// Records an assignment to `name` in every enclosing loop whose floor it
    /// resolves below. The counterpart to `lookup_capturing`, and it exists
    /// because `assign` reads its target with `lookup`, which records nothing:
    /// a name the body only writes has no crossing READ to notice.
    fn record_assignment(
        &mut self,
        name: &str,
        compound: Option<ArithOp>,
        in_atomic: bool,
        span: Span,
    ) {
        let Some(depth) = self.depth_of(name) else {
            return;
        };
        let Some(ty) = self.lookup(name).map(|local| local.ty) else {
            return;
        };
        for ctx in &mut self.loop_ctx {
            if depth >= ctx.floor || name == ctx.binder {
                continue;
            }
            let record = ctx.assigned.entry(name.to_owned()).or_insert(AssignRecord {
                ty,
                span,
                only_compound: true,
                merge: compound,
                all_atomic: true,
            });
            record.only_compound &= compound.is_some();
            // Two assignments that disagree about the operator cannot share one
            // accumulator, and folding a product with `+` is the silent wrong
            // answer this carries.
            if record.merge != compound {
                record.merge = None;
            }
            record.all_atomic &= in_atomic;
        }
    }

    /// `atomic e`. The depth is what the assignment carve-out reads, and it is
    /// a depth rather than a flag because atomic.tex:72-75 permits nesting and
    /// `ProjectFortress/tests/atomic4.fss` uses it.
    fn atomic(&mut self, body: &Expr, span: Span, expected: Option<Type>) -> Checked<TypedExpr> {
        self.atomic_depth += 1;
        let checked = self.expr(body, expected);
        self.atomic_depth -= 1;
        let body = checked?;
        let ty = body.ty;
        Ok(TypedExpr {
            kind: TypedExprKind::Atomic {
                body: Box::new(body),
            },
            ty,
            span,
        })
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
            Expr::For {
                binder,
                lo,
                hi,
                inclusive,
                sequential,
                body,
                span,
            } => self.for_expr(
                binder,
                lo,
                hi,
                *inclusive,
                *sequential,
                body,
                *span,
                expected,
            ),
            Expr::Atomic { body, span } => self.atomic(body, *span, expected),
            Expr::Case {
                subject,
                arms,
                else_arm,
                span,
            } => self.case_expr(subject, arms, else_arm.as_deref(), *span, expected),
            Expr::TypeCase {
                subject,
                arms,
                else_arm,
                span,
            } => self.typecase_expr(subject, arms, else_arm, *span, expected),
            Expr::Label { name, body, span } => self.label_expr(name, body, *span, expected),
            Expr::AlsoDo { blocks, span } => self.also_do(blocks, *span, expected),
            Expr::ForIn {
                binder,
                source,
                sequential,
                body,
                span,
            } => self.for_in(binder, source, *sequential, body, *span, expected),
            Expr::BigReduction {
                op,
                binder,
                lo,
                hi,
                inclusive,
                sequential,
                body,
                span,
            } => self.big_reduction(
                *op,
                binder,
                lo,
                hi,
                *inclusive,
                *sequential,
                body,
                *span,
                expected,
            ),
            // Unreachable: closure lowering runs before the checker and either
            // rewrites a lambda into a construction or refuses it by name.
            Expr::Lambda { span, .. } => Err(TypeError::LambdaUnsupported {
                span: *span,
                form: "a `fn` in this position",
            }),
            Expr::Exit { name, value, span } => {
                self.exit_expr(name.as_deref(), value.as_deref(), *span, expected)
            }
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
        if let Some(local) = self.lookup_capturing(name) {
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

    /// `for i <- lo#n do ... end`.
    ///
    /// The index is ZZ64 and so are the bounds. That is not a simplification
    /// for its own sake: array subscripts are ZZ64 because the JVM's 2^31
    /// ceiling is why this rewrite exists, and a loop that fills an array has
    /// to be able to reach every slot of it.
    ///
    /// The scope boundary is enforced here, and every rule is SYNTACTIC. That
    /// is the whole reason M4 needs no dataflow analysis: a body that cannot
    /// name anything outside itself on the left of an assignment cannot race,
    /// whatever order its iterations run in.
    #[allow(clippy::too_many_arguments)]
    fn for_expr(
        &mut self,
        binder: &str,
        lo: &Expr,
        hi: &Expr,
        inclusive: bool,
        sequential: bool,
        body: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let lo_typed = self.expr(lo, Some(Type::ZZ64))?;
        let bound = self.expr(hi, Some(Type::ZZ64))?;
        for operand in [&lo_typed, &bound] {
            if operand.ty != Type::ZZ64 {
                return Err(TypeError::Mismatch {
                    span: operand.span,
                    found: operand.ty,
                    required: Type::ZZ64,
                });
            }
        }

        // One shape reaches codegen: a half-open [lo, hi). `a:b` is inclusive,
        // so its end is b + 1; `a#n` is a count, so its end is a + n. The
        // difference between the two generator forms stops existing here
        // rather than in the runtime.
        let add = |left: TypedExpr, right: TypedExpr| TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::Arith {
                    op: ArithOp::Add,
                    ty: Type::ZZ64,
                },
                args: vec![left, right],
            },
            ty: Type::ZZ64,
            span,
        };
        let one = TypedExpr {
            kind: TypedExprKind::IntConst(1),
            ty: Type::ZZ64,
            span,
        };
        let hi_typed = if inclusive {
            add(bound, one)
        } else {
            add(lo_typed.clone(), bound)
        };

        let mut scope = HashMap::new();
        scope.insert(
            binder.to_owned(),
            Local {
                ty: Type::ZZ64,
                mutable: false,
            },
        );
        self.scopes.push(scope);
        self.loop_ctx.push(LoopCtx {
            binder: binder.to_owned(),
            parallel: !sequential,
            floor: self.scopes.len() - 1,
            captures: BTreeMap::new(),
            assigned: BTreeMap::new(),
            tainted: BTreeSet::new(),
        });

        let checked = self.expr(body, Some(Type::Void));

        let ctx = self.loop_ctx.pop();
        self.scopes.pop();
        let body_typed = checked?;
        let Some(ctx) = ctx else {
            return Err(TypeError::ParallelFormUnsupported {
                span,
                form: "a loop body",
            });
        };
        if body_typed.ty != Type::Void {
            return Err(TypeError::ParallelFormUnsupported {
                span,
                form: "a loop body with a value",
            });
        }

        // THE ORDER OF THESE THREE DECISIONS IS FIXED, and two of the other
        // orderings are races.
        //
        // 1. the body walk is done, so `ctx.captures` is finally complete;
        // 2. recognise reductions against it -- decide this at the assignment
        //    instead and `atomic sum += a[i]` followed by `println(sum)` reads
        //    as a private accumulator AND a captured read of the same name;
        // 3. only then compute capture mode. A recognised reduction is
        //    captured NOT AT ALL, by value or by reference; a name that stays
        //    on the lock path is captured by reference. Do it the other way
        //    round and a reduction lands in the environment as a shared
        //    pointer with the lock already elided, which is an unsynchronised
        //    load-add-store from up to sixteen threads.
        let mut reductions: Vec<TypedReduction> = Vec::new();
        let mut by_ref: BTreeMap<String, Type> = BTreeMap::new();
        for (name, record) in ctx.assigned {
            let recognised = !sequential
                && record.only_compound
                && record.merge.is_some()
                && !ctx.captures.contains_key(&name)
                && Self::reducible(record.ty);
            if let (true, Some(op)) = (recognised, record.merge) {
                reductions.push(TypedReduction {
                    name,
                    ty: record.ty,
                    op,
                });
                continue;
            }
            // The compound carve-out let this through at the assignment on the
            // promise that it would turn out to be a reduction. It did not, so
            // the boundary applies after all -- unless the lock does.
            if !sequential && !record.all_atomic {
                return Err(TypeError::ParallelEscape {
                    span: record.span,
                    name,
                });
            }
            by_ref.insert(name, record.ty);
        }

        let mut captures: Vec<TypedCapture> = ctx
            .captures
            .into_iter()
            .map(|(name, ty)| TypedCapture {
                by_ref: by_ref.contains_key(&name),
                name,
                ty,
            })
            .collect();
        for (name, ty) in by_ref {
            if !captures.iter().any(|c| c.name == name) {
                captures.push(TypedCapture {
                    name,
                    ty,
                    by_ref: true,
                });
            }
        }
        // Sorted, so the environment's field order is a function of the source
        // and not of the order two maps happened to be drained in.
        captures.sort_by(|a, b| a.name.cmp(&b.name));
        reductions.sort_by(|a, b| a.name.cmp(&b.name));

        self.loops += 1;
        let symbol = format!("$loop{}", self.loops);
        self.require(Type::Void, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::ParallelFor {
                binder: binder.to_owned(),
                lo: Box::new(lo_typed),
                hi: Box::new(hi_typed),
                body: Box::new(body_typed),
                captures,
                reductions,
                symbol,
                sequential,
            },
            ty: Type::Void,
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

    /// `x op= e`. Only `+` and `-` -- everything past them needs a
    /// `Monoid[\\T,op\\]` and a user-declared identity, and `||=` is the
    /// biggest single thing left on the table at 37 corpus uses.
    ///
    /// The refusal arm is unreachable through today's parser, which only reads
    /// `+` and `-` as compound operators at all. It is a diagnostic rather than
    /// an `unreachable!()` because the rule here is that malformed input is
    /// never a crash, and the day the parser learns another operator this is
    /// what it lands on.
    fn compound_op(&self, op: Option<BinOp>, ty: Type, span: Span) -> Checked<Option<ArithOp>> {
        let Some(op) = op else {
            return Ok(None);
        };
        let arith = match op {
            BinOp::Add => ArithOp::Add,
            BinOp::Sub => ArithOp::Sub,
            // Reachable only from a BIG reduction: none of these three is a
            // compound operator this parser reads, deliberately, because the
            // corpus writes none. The accumulator carries its own operator, so
            // the merge folds with the same one and the identity follows it.
            BinOp::Mul => ArithOp::Mul,
            BinOp::Max => ArithOp::Max,
            BinOp::Min => ArithOp::Min,
            _ => {
                return Err(TypeError::CompoundOperatorUnsupported {
                    span,
                    op: op_name(op),
                })
            }
        };
        if !ty.is_numeric() {
            return Err(TypeError::Mismatch {
                span,
                found: ty,
                required: Type::ZZ64,
            });
        }
        Ok(Some(arith))
    }

    /// Whether a reduction on this type is one the merge can perform. Identity
    /// for `+` and `-` is a zero bit pattern on all three, and every one of the
    /// corpus files M5 unlocks declares `var count: ZZ32`.
    const fn reducible(ty: Type) -> bool {
        matches!(ty, Type::ZZ32 | Type::ZZ64 | Type::RR64)
    }

    /// The innermost enclosing PARALLEL loop, if there is one. A `seq(...)`
    /// loop is not one: it runs in order on one thread and needs no boundary.
    fn parallel_loop(&self) -> Option<&LoopCtx> {
        self.loop_ctx.iter().rev().find(|c| c.parallel)
    }

    /// Which scope a name resolves in. A target that resolves BELOW the loop's
    /// floor is shared between iterations, and assigning to it is the race.
    fn depth_of(&self, name: &str) -> Option<usize> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(depth, s)| s.contains_key(name).then_some(depth))
    }

    /// Whether a name is shared across iterations: it resolves BELOW the
    /// loop's own scope, so every iteration sees the same storage. This one
    /// comparison is the whole of M4's race freedom.
    fn escapes_loop(&self, name: &str, floor: usize) -> bool {
        matches!(self.depth_of(name), Some(depth) if depth < floor)
    }

    /// The same question once ALIASING is possible. `escapes_loop` answers it
    /// by scope depth, which was exact while the only way to name shared
    /// storage was to be declared outside the loop. A binding whose value came
    /// from outside is loop-local and shared at once, so it is carried
    /// separately and asked about here.
    fn shared_in_loop(&self, name: &str, floor: usize) -> bool {
        self.escapes_loop(name, floor)
            || self
                .parallel_loop()
                .is_some_and(|c| c.tainted.contains(name))
    }

    /// Whether an initializer reads anything shared. Conservative and
    /// syntactic: any mention anywhere in the expression taints the binding,
    /// because `shared.child` and `pick(shared)` are both aliases of it.
    fn reads_shared(&self, e: &Expr, floor: usize) -> bool {
        match e {
            Expr::Var { name, .. } => self.shared_in_loop(name, floor),
            Expr::Unit { .. }
            | Expr::IntLit { .. }
            | Expr::FloatLit { .. }
            | Expr::StrLit { .. }
            | Expr::BoolLit { .. } => false,
            Expr::Tuple { items, .. } | Expr::Juxt { items, .. } | Expr::ArrayLit { items, .. } => {
                items.iter().any(|i| self.reads_shared(i, floor))
            }
            Expr::Infix { lhs, rhs, .. } => {
                self.reads_shared(lhs, floor) || self.reads_shared(rhs, floor)
            }
            Expr::Prefix { operand, .. } => self.reads_shared(operand, floor),
            Expr::Call { callee, args, .. } => {
                self.reads_shared(callee, floor) || args.iter().any(|a| self.reads_shared(a, floor))
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.reads_shared(cond, floor)
                    || self.reads_shared(then_branch, floor)
                    || else_branch
                        .as_deref()
                        .is_some_and(|e| self.reads_shared(e, floor))
            }
            Expr::Block { items, .. } => items.iter().any(|item| match item {
                BlockItem::Binding(b) => self.reads_shared(&b.value, floor),
                BlockItem::Assign(a) => self.reads_shared(&a.value, floor),
                BlockItem::Expr(e) => self.reads_shared(e, floor),
            }),
            Expr::Index { base, index, .. } => {
                self.reads_shared(base, floor) || self.reads_shared(index, floor)
            }
            Expr::While { cond, body, .. } => {
                self.reads_shared(cond, floor) || self.reads_shared(body, floor)
            }
            Expr::Field { base, .. } => self.reads_shared(base, floor),
            Expr::For { lo, hi, body, .. } => {
                self.reads_shared(lo, floor)
                    || self.reads_shared(hi, floor)
                    || self.reads_shared(body, floor)
            }
            Expr::Instantiate { callee, .. } => self.reads_shared(callee, floor),
            Expr::Atomic { body, .. } => self.reads_shared(body, floor),
            Expr::Case {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.reads_shared(subject, floor)
                    || arms.iter().any(|a| {
                        self.reads_shared(&a.guard, floor) || self.reads_shared(&a.body, floor)
                    })
                    || else_arm
                        .as_deref()
                        .is_some_and(|e| self.reads_shared(e, floor))
            }
            Expr::TypeCase {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.reads_shared(subject, floor)
                    || arms.iter().any(|a| self.reads_shared(&a.body, floor))
                    || self.reads_shared(else_arm, floor)
            }
            Expr::Label { body, .. } => self.reads_shared(body, floor),
            Expr::Lambda { body, .. } => self.reads_shared(body, floor),
            Expr::AlsoDo { blocks, .. } => blocks.iter().any(|b| self.reads_shared(b, floor)),
            Expr::ForIn { source, body, .. } => {
                self.reads_shared(source, floor) || self.reads_shared(body, floor)
            }
            Expr::BigReduction { lo, hi, body, .. } => {
                self.reads_shared(lo, floor)
                    || self.reads_shared(hi, floor)
                    || self.reads_shared(body, floor)
            }
            Expr::Exit { value, .. } => value
                .as_deref()
                .is_some_and(|e| self.reads_shared(e, floor)),
        }
    }

    /// The name an assignment target is rooted at. `o.left.count := 1` is
    /// rooted at `o`; a target rooted at no name at all -- the result of a
    /// call -- has no name to ask about and is treated as shared.
    fn target_root(target: &Expr) -> Option<(&str, Span)> {
        match target {
            Expr::Var { name, span } => Some((name, *span)),
            Expr::Field { base, .. } | Expr::Index { base, .. } => Self::target_root(base),
            _ => None,
        }
    }

    fn assign(&mut self, a: &Assign) -> Checked<TypedBlockItem> {
        let in_atomic = self.atomic_depth > 0;
        // Before anything can refuse it: the crossing has to be on the record
        // whether it is legal or not, because the environment is built from
        // exactly these two maps and a write-only name appears in neither
        // otherwise.
        if let Expr::Var { name, .. } = &a.target {
            // The MERGE operator, not the assignment's own: `-=` accumulates
            // `Identity - e`, so the group inverse is already inside the
            // partial and the fold is `+`. Recording `Sub` here folds it back
            // out -- 1000 - (-100000) = 101000 where -99000 belongs, which is
            // what tools/atomic-gate.sh's two-reduction case measures.
            let compound = self
                .lookup(name)
                .and_then(|local| self.compound_op(a.op, local.ty, a.span).ok())
                .flatten()
                .map(merge_op);
            self.record_assignment(name, compound, in_atomic, a.span);
        }
        // THE scope boundary, and it is still one comparison. Everything M4
        // claims about data races reduces to this: a parallel body may only
        // assign to storage it can prove belongs to its own iteration -- a
        // binding it declared itself, or the array slot its own index names.
        //
        // M5 gives it two carve-outs. `atomic` serialises the write, so it is
        // allowed here and the capture becomes a by-reference one. A COMPOUND
        // assignment is a candidate reduction, and its verdict needs the
        // finished body -- reduction.tex:35 asks whether the name is otherwise
        // READ, and the read may come later in the same body -- so it is
        // deferred to `for_expr`, where `captures` is complete.
        if let Some(floor) = self.parallel_loop().map(|c| c.floor) {
            let binder = self.parallel_loop().map(|c| c.binder.clone());
            match &a.target {
                Expr::Var { name, span } => {
                    if self.escapes_loop(name, floor) && !in_atomic && a.op.is_none() {
                        return Err(TypeError::ParallelEscape {
                            span: *span,
                            name: name.clone(),
                        });
                    }
                }
                // A field has no index, so there is no `a[binder]` carve-out
                // to make two iterations disjoint: a shared receiver is
                // refused outright. This is the rule that keeps M5's soundness
                // argument true now that a field store exists -- the argument
                // was "no field store exists in the language yet".
                Expr::Field { span, .. } if !in_atomic => {
                    let root = Self::target_root(&a.target).map(|(n, _)| n.to_owned());
                    let shared = root
                        .as_deref()
                        .is_none_or(|name| self.shared_in_loop(name, floor));
                    if shared {
                        let aliased = root
                            .as_deref()
                            .is_some_and(|name| !self.escapes_loop(name, floor));
                        return Err(TypeError::ParallelFieldEscape {
                            span: *span,
                            name: root.unwrap_or_default(),
                            aliased,
                        });
                    }
                }
                Expr::Index { base, index, span } if !in_atomic => {
                    // An array the body created ITSELF is private to this
                    // iteration, so any index into it is safe. Only a shared
                    // array -- one whose name resolves below the loop, or one
                    // bound from such a name -- has to be written at the slot
                    // this iteration owns.
                    let shared = match base.as_ref() {
                        Expr::Var { name, .. } => self.shared_in_loop(name, floor),
                        _ => true,
                    };
                    let names_binder = matches!(
                        (index.as_ref(), binder.as_deref()),
                        (Expr::Var { name, .. }, Some(b)) if name == b
                    );
                    if shared && !names_binder {
                        return Err(TypeError::ParallelIndexNotBinder {
                            span: *span,
                            binder: binder.unwrap_or_default(),
                        });
                    }
                }
                // Inside `atomic` the lock is what makes two iterations
                // writing the same slot safe, so any index is allowed.
                _ => {}
            }
        }
        match &a.target {
            Expr::Var { name, span } => {
                // Inside a method a bare name may be one of the receiver's
                // fields, exactly as it is when it is READ. Locals win, so
                // this is only reached when nothing else binds the name.
                if self.lookup(name).is_none() {
                    if let Some(target) = self.self_field_target(name, *span)? {
                        return self.field_assignment(target, a, *span);
                    }
                }
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
                let op = self.compound_op(a.op, local.ty, *span)?;
                let value = self.expr(&a.value, Some(local.ty))?;
                Ok(TypedBlockItem::Assign {
                    target: AssignTarget::Var {
                        name: name.clone(),
                        ty: local.ty,
                    },
                    op,
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
                let op = self.compound_op(a.op, elem.as_type(), *span)?;
                let value = self.expr(&a.value, Some(elem.as_type()))?;
                Ok(TypedBlockItem::Assign {
                    target: AssignTarget::Element {
                        base: Box::new(base),
                        index: Box::new(index),
                        elem,
                    },
                    op,
                    value,
                    span: a.span,
                })
            }
            Expr::Field { base, name, span } => {
                let base = self.expr(base, None)?;
                let Type::Object(object) = base.ty else {
                    return Err(TypeError::UnknownField {
                        span: *span,
                        found: base.ty,
                        name: name.clone(),
                    });
                };
                let Some((index, field)) = self.registry.field_decl(object, name) else {
                    return Err(TypeError::UnknownField {
                        span: *span,
                        found: base.ty,
                        name: name.clone(),
                    });
                };
                let (ty, mutable) = (field.ty, field.mutable);
                if !mutable {
                    return Err(TypeError::FieldIsImmutable {
                        span: *span,
                        name: name.clone(),
                    });
                }
                self.field_assignment(
                    AssignTarget::Field {
                        base: Box::new(base),
                        index,
                        ty,
                    },
                    a,
                    *span,
                )
            }
            other => Err(TypeError::InvalidAssignTarget { span: other.span() }),
        }
    }

    /// A bare name that is a mutable field of the receiver, as an assignment
    /// target. `None` when the name is not a field at all; the immutable case
    /// is an error rather than a miss, so that `w := 1` on a constructor
    /// parameter says what is wrong instead of "not declared".
    fn self_field_target(&mut self, name: &str, span: Span) -> Checked<Option<AssignTarget>> {
        let Some(ctx) = &self.self_ctx else {
            return Ok(None);
        };
        let Some((index, field)) = ctx.fields.iter().enumerate().find(|(_, f)| f.name == name)
        else {
            return Ok(None);
        };
        if !field.mutable {
            return Err(TypeError::FieldIsImmutable {
                span,
                name: name.to_owned(),
            });
        }
        let (ty, receiver) = (field.ty, ctx.ty);
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        Ok(Some(AssignTarget::Field {
            base: Box::new(TypedExpr {
                kind: TypedExprKind::Var("self".to_owned()),
                ty: receiver,
                span,
            }),
            index,
            ty,
        }))
    }

    /// The half both field spellings share: the compound operator and the
    /// value, checked against the field's own type.
    fn field_assignment(
        &mut self,
        target: AssignTarget,
        a: &Assign,
        span: Span,
    ) -> Checked<TypedBlockItem> {
        let AssignTarget::Field { ty, .. } = target else {
            return Err(TypeError::InvalidAssignTarget { span });
        };
        let op = self.compound_op(a.op, ty, span)?;
        let value = self.expr(&a.value, Some(ty))?;
        Ok(TypedBlockItem::Assign {
            target,
            op,
            value,
            span: a.span,
        })
    }

    /// A loop-local binding that names storage from outside the loop. Only a
    /// reference type can: a scalar binding is a copy, and writing it cannot
    /// reach anything another iteration sees.
    fn taint_if_aliased(&mut self, name: &str, ty: Type, value: &Expr) {
        let Some(floor) = self.parallel_loop().map(|c| c.floor) else {
            return;
        };
        if !ty.is_reference() && !matches!(ty, Type::Array(_)) {
            return;
        }
        if !self.reads_shared(value, floor) {
            return;
        }
        if let Some(ctx) = self.loop_ctx.iter_mut().rev().find(|c| c.parallel) {
            ctx.tainted.insert(name.to_owned());
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
        // Only `Mismatch` is rewritten. `LiteralNotApplicable` carries the
        // type the slot REQUIRED, not the one the operand had, so reusing its
        // payload here reported "this one is Boolean" about the literal `1`.
        // Its own message already names the literal correctly.
        let inner = self
            .expr(operand, Some(Type::Boolean))
            .map_err(|e| match e {
                TypeError::Mismatch { span, found, .. } => TypeError::LogicalOperandNotBoolean {
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
                    TypeError::Mismatch { span, found, .. } => {
                        TypeError::LogicalOperandNotBoolean {
                            span,
                            op: name,
                            found,
                        }
                    }
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

    /// `a^b`, and the one place two numeric operands are allowed to disagree.
    ///
    /// 1.0 declares `^` on every base-exponent pair -- an integer raised to a
    /// real is a real, a real raised to an integer is a real -- and
    /// `ProjectFortress/tests/expTest.fss` is the corpus asserting all four.
    /// Requiring agreement here would have been consistent with `+` and wrong
    /// about the operator this milestone exists to add.
    ///
    /// The exponent takes no hint from context: in `x: RR64 = 2^10` the base
    /// is pinned by the binding and the exponent is an ordinary ZZ32 literal.
    fn power(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let hint = expected.filter(|t| t.is_numeric());
        let base = self.expr(lhs, hint)?;
        let exponent = self.expr(rhs, None)?;
        for operand in [&base, &exponent] {
            if !operand.ty.is_numeric() {
                return Err(TypeError::Mismatch {
                    span: operand.span,
                    found: operand.ty,
                    required: Type::ZZ64,
                });
            }
        }
        // A real anywhere makes the result real; two integers keep the base's
        // width, because there is no implicit widening in this language.
        let ty = if base.ty == Type::RR64 || exponent.ty == Type::RR64 {
            Type::RR64
        } else {
            base.ty
        };
        let target = Target::Pow {
            base: base.ty,
            exponent: exponent.ty,
        };
        self.require(ty, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target,
                args: vec![base, exponent],
            },
            ty,
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
        if op == BinOp::Pow {
            return self.power(lhs, rhs, span, expected);
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
            BinOp::Div => {
                // The runtime guard catches every divisor that reaches it, but
                // a literal zero never does: LLVM folds the division to
                // `poison` while the module is being built, and the program
                // prints a value nothing computed.
                if left.ty.is_integer() && right.kind == TypedExprKind::IntConst(0) {
                    return Err(TypeError::DivisionByZero { span });
                }
                (
                    Target::Arith {
                        op: ArithOp::Div,
                        ty: left.ty,
                    },
                    left.ty,
                )
            }
            // No source syntax constructs these -- `a MAX b` is a word
            // operator this parser does not read -- so an infix one cannot
            // arise. A diagnostic rather than a panic, because malformed input
            // is never a crash here.
            BinOp::Max | BinOp::Min => {
                return Err(TypeError::CompoundOperatorUnsupported {
                    span,
                    op: op_name(op),
                })
            }
            // Routed above: `^` is the one operator whose operands may
            // differ in type, so it never reaches the agreement check.
            BinOp::Pow => {
                return Err(TypeError::MixedNumericOperands {
                    span,
                    left: left.ty,
                    right: right.ty,
                })
            }
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
                // The same guard `println` has, and for the same reason it
                // gives: there is one `to_string` shim per scalar and none for
                // anything else, so saying so HERE is a diagnostic and leaving
                // it to codegen is `no runtime symbol to_string_Shape` at exit
                // 70 -- a compiler internal error raised by ordinary source.
                // `Elem::of` is the scalar test; it is `None` for a trait, an
                // object, an array and Void.
                if Elem::of(from).is_none() {
                    return Err(TypeError::NotConcatenable {
                        span: t.span,
                        found: from,
                    });
                }
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
            self.refuse_keyword_argument(args)?;
            // The RECEIVER is an argument too, and the one this compiler used
            // to be able to trust: before a field store existed, a method
            // could not write anything its receiver owned. `b.bump()` where
            // `bump` writes `self.n` is the same race as `bump(b)`, and the
            // method body is checked in the method's own context, so the
            // loop's lexical rules never see the write.
            self.refuse_shared_receiver(base)?;
            self.refuse_shared_array_argument(args)?;
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
            "println" => self.println(args, span, expected, true),
            "print" => self.println(args, span, expected, false),
            "ignore" => self.ignore(args, span, expected),
            "assert" => self.assert(args, span, expected),
            "array" => self.array_new(args, span, expected),
            "length" => self.array_length(args, span, expected),
            _ if self.registry.is_object(name) => {
                self.refuse_shared_array_argument(args)?;
                self.refuse_keyword_argument(args)?;
                self.construct(name, args, span, *callee_span, expected)
            }
            _ => {
                self.refuse_shared_array_argument(args)?;
                self.refuse_keyword_argument(args)?;
                self.user_call(name, args, span, *callee_span, expected)
            }
        }
    }

    /// M4's boundary is one comparison over an assignment WRITTEN IN THE BODY.
    /// An array is captured by pointer, so handing one to a callee moves the
    /// store somewhere `assign` never looks and the guard never runs -- the
    /// loop rules go quiet rather than refusing. Refused at every call site
    /// until a whole-program reachability pass exists; the compiler is already
    /// whole-program for M3c's dispatch, so the machinery is not far away.
    ///
    /// Bare `Var` arguments only, and that composes: `f(g(a))` is refused at
    /// `g(a)`, because the inner call is checked as an argument expression and
    /// arrives here itself. `f(a[i])` passes an element and is left alone.
    ///
    /// Inside `atomic` the lock serialises the callee's writes too, so the
    /// refusal lifts with the rest of the boundary.
    /// The receiver half of the same rule. A method reaches its receiver's
    /// storage by construction -- that is what a receiver IS -- so the question
    /// is only whether the receiver names something shared between iterations
    /// and whether anything under it can be written.
    fn refuse_shared_receiver(&mut self, base: &Expr) -> Checked<()> {
        if self.atomic_depth > 0 {
            return Ok(());
        }
        let Some(floor) = self.parallel_loop().map(|c| c.floor) else {
            return Ok(());
        };
        let Expr::Var { name, span } = base else {
            return Ok(());
        };
        if !self.shared_in_loop(name, floor) {
            return Ok(());
        }
        let Some(ty) = self.lookup(name).map(|l| l.ty) else {
            return Ok(());
        };
        if matches!(ty, Type::Array(_)) {
            return Err(TypeError::ParallelSharedArrayArgument {
                span: *span,
                name: name.clone(),
            });
        }
        if let Some(path) = self.registry.reaches_mutable(ty) {
            return Err(TypeError::ParallelSharedObjectArgument {
                span: *span,
                name: name.clone(),
                path: if path.is_empty() {
                    name.clone()
                } else {
                    format!("{name}.{path}")
                },
            });
        }
        Ok(())
    }

    fn refuse_shared_array_argument(&mut self, args: &[Expr]) -> Checked<()> {
        if self.atomic_depth > 0 {
            return Ok(());
        }
        let Some(floor) = self.parallel_loop().map(|c| c.floor) else {
            return Ok(());
        };
        for arg in args {
            let Expr::Var { name, span } = arg else {
                continue;
            };
            let Some(ty) = self.lookup(name).map(|l| l.ty) else {
                continue;
            };
            if !self.shared_in_loop(name, floor) {
                continue;
            }
            if matches!(ty, Type::Array(_)) {
                return Err(TypeError::ParallelSharedArrayArgument {
                    span: *span,
                    name: name.clone(),
                });
            }
            // An object is only safe to hand over while nothing inside it can
            // be written. That was every object until field mutation landed,
            // which is why this question is asked of the registry now rather
            // than assumed.
            if let Some(path) = self.registry.reaches_mutable(ty) {
                return Err(TypeError::ParallelSharedObjectArgument {
                    span: *span,
                    name: name.clone(),
                    path: if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{name}.{path}")
                    },
                });
            }
        }
        Ok(())
    }

    /// A bare `Ident = e` argument. 1.0 spells a keyword argument exactly that
    /// way and reserves the parenthesised form for an equality test, but
    /// `primary`'s LParen arm returns the inner expression with no wrapper, so
    /// `f(x = 2)` and `f((x = 2))` are the same tree and the distinction cannot
    /// be recovered. It was resolved silently in favour of the test.
    ///
    /// Guarded HERE and not at the top of `call`, so the seven builtins and the
    /// MPI ops keep the reading they already have: none of them has a named
    /// parameter, so `assert(count = 1000)` is unambiguous -- and it is legal,
    /// working Fortress that a blanket guard would regress.
    ///
    /// A `Field` left side and a CHAIN are both outside the predicate on
    /// purpose. A keyword argument names a bare parameter, so `f(p.n = 3)`
    /// cannot be one, and `f(a = b = c)` is an `Expr::Block` by the time it
    /// arrives -- `desugar_chain` has already rewritten it.
    fn refuse_keyword_argument(&self, args: &[Expr]) -> Checked<()> {
        for arg in args {
            let Expr::Infix {
                op: BinOp::Eq, lhs, ..
            } = arg
            else {
                continue;
            };
            if let Expr::Var { name, span } = &**lhs {
                return Err(TypeError::KeywordArgumentUnsupported {
                    span: *span,
                    name: name.clone(),
                });
            }
        }
        Ok(())
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
        // The source comes from the argument and the target from context, so
        // every pair `is_widening_of` recognises is expressible. Both used to
        // be hardcoded to the one integer widening, which made the advice a
        // dead end: `x: RR64 = widen(n)` repeated `write \`widen(...)\`` one
        // type up, and no expression anywhere in the language reached an RR64
        // from an integer.
        let inner = self.expr(arg, None)?;
        let from = inner.ty;
        let to = match expected {
            Some(want) if want.is_widening_of(from) => want,
            // With no context the target is FORCED from ZZ64 -- RR64 is the
            // only widening of it -- and chosen narrowest from ZZ32, which is
            // what M1 did and what the acceptance program depends on.
            _ => match from {
                Type::ZZ32 => Type::ZZ64,
                Type::ZZ64 => Type::RR64,
                _ => return Err(TypeError::NotWidenable { span, found: from }),
            },
        };
        // An `expected` that is not a widening of the argument lands here and
        // is reported as the ordinary mismatch it is.
        self.require(to, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::Widen { from, to },
                args: vec![inner],
            },
            ty: to,
            span,
        })
    }

    /// `println` and `print`, which differ by one character in the shim they
    /// reach and by nothing else.
    fn println(
        &mut self,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
        newline: bool,
    ) -> Checked<TypedExpr> {
        let name = if newline { "println" } else { "print" };
        let [arg] = args else {
            return Err(TypeError::ArityMismatch {
                span,
                name: name.to_owned(),
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
                target: if newline {
                    Target::Println { ty }
                } else {
                    Target::Print { ty }
                },
                args: vec![inner],
            },
            ty: Type::Void,
            span,
        })
    }

    /// `ignore(e)`: evaluate `e` for its effects and discard its value.
    ///
    /// A block whose only item is the expression and whose tail is absent is
    /// exactly that, so this needs no target and no shim -- the discard is what
    /// a block statement already does.
    fn ignore(&mut self, args: &[Expr], span: Span, expected: Option<Type>) -> Checked<TypedExpr> {
        let [arg] = args else {
            return Err(TypeError::ArityMismatch {
                span,
                name: "ignore".to_owned(),
                expected: 1,
                found: args.len(),
            });
        };
        let inner = self.expr(arg, None)?;
        self.require(Type::Void, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Block {
                items: vec![TypedBlockItem::Expr(inner)],
                tail: None,
            },
            ty: Type::Void,
            span,
        })
    }

    /// `assert`, in the four shapes the corpus writes:
    ///
    /// ```text
    /// assert(flag)                 assert(flag, message)
    /// assert(actual, expected)     assert(actual, expected, message)
    /// ```
    ///
    /// The two two-argument forms are told apart by the SECOND argument's
    /// type. A message is a String, so a Boolean first argument followed by a
    /// String is the flag-and-message form and anything else is the equality
    /// form. `assert(s1, s2)` on two Strings is the equality form, and String
    /// equality is not implemented, so it is refused by name rather than
    /// quietly read as a message.
    ///
    /// It becomes an `if`, a call to the halt shim, and nothing else. The
    /// comparison is the `Target::Compare` the language already has, so an
    /// assert is exactly as strong as `=` is -- and no stronger.
    fn assert(&mut self, args: &[Expr], span: Span, expected: Option<Type>) -> Checked<TypedExpr> {
        let (condition, message) = match args {
            [flag] => (self.assert_flag(flag, span)?, None),
            [flag, second] => {
                let first = self.expr(flag, None)?;
                if first.ty == Type::Boolean {
                    // Told apart by the second argument's TYPE, not by whether
                    // it is a string LITERAL. `tst(s: String, a: Boolean) =
                    // assert(a, s)` is the legacy library's own idiom --
                    // `tests/intPrim.fss:16` -- and asking for a literal sent
                    // it into the equality form, where the message was
                    // reported as a Boolean that was not one.
                    //
                    // It takes no hint, because there is nothing to hint at
                    // yet: which form this is has not been decided.
                    let right = self.expr(second, None)?;
                    if right.ty == Type::String {
                        (first, Some(right))
                    } else {
                        (self.assert_equal(first, right, span)?, None)
                    }
                } else {
                    // Not a flag, so this is the equality form and the second
                    // operand takes the first's type -- which is what pins a
                    // bare literal in `assert(x, 17)`.
                    let right = self.expr(second, Some(first.ty))?;
                    (self.assert_equal(first, right, span)?, None)
                }
            }
            [actual, wanted, message] => {
                let left = self.expr(actual, None)?;
                let right = self.expr(wanted, Some(left.ty))?;
                let message = self.expr(message, Some(Type::String))?;
                (self.assert_equal(left, right, span)?, Some(message))
            }
            _ => {
                return Err(TypeError::ArityMismatch {
                    span,
                    name: "assert".to_owned(),
                    expected: 3,
                    found: args.len(),
                })
            }
        };
        let message = message.unwrap_or(TypedExpr {
            kind: TypedExprKind::StrConst("assertion failed".to_owned()),
            ty: Type::String,
            span,
        });
        self.require(Type::Void, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::If {
                cond: Box::new(condition),
                then_branch: Box::new(TypedExpr {
                    kind: TypedExprKind::Unit,
                    ty: Type::Void,
                    span,
                }),
                else_branch: Some(Box::new(TypedExpr {
                    kind: TypedExprKind::Apply {
                        target: Target::AssertFailed,
                        args: vec![message],
                    },
                    ty: Type::Void,
                    span,
                })),
            },
            ty: Type::Void,
            span,
        })
    }

    fn assert_flag(&mut self, flag: &Expr, span: Span) -> Checked<TypedExpr> {
        let typed = self.expr(flag, Some(Type::Boolean))?;
        if typed.ty != Type::Boolean {
            return Err(TypeError::LogicalOperandNotBoolean {
                span,
                op: "assert",
                found: typed.ty,
            });
        }
        Ok(typed)
    }

    fn assert_equal(
        &mut self,
        left: TypedExpr,
        right: TypedExpr,
        span: Span,
    ) -> Checked<TypedExpr> {
        if left.ty != right.ty {
            return Err(TypeError::MixedNumericOperands {
                span,
                left: left.ty,
                right: right.ty,
            });
        }
        if !left.ty.is_numeric() && left.ty != Type::Boolean {
            return Err(TypeError::NotComparable {
                span,
                found: left.ty,
            });
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target: Target::Compare {
                    op: CompareOp::Eq,
                    ty: left.ty,
                },
                args: vec![left, right],
            },
            ty: Type::Boolean,
            span,
        })
    }

    /// `case` is a DESUGARING and not a node: the subject is bound once and the
    /// arms become an `if`/`elif` chain over `subject = guard`. Building the
    /// chain out of AST rather than typed nodes is what makes every comparison
    /// rule -- which types `=` is defined on, what it means on an object --
    /// exactly the rules `infix` already enforces, with the diagnostics it
    /// already writes.
    ///
    /// THE SUBJECT IS EVALUATED ONCE. `case f(x) of ...` with the subject
    /// inlined into every guard runs `f` once per arm, which is the defect
    /// M3f's chained comparison already paid for once.
    fn case_expr(
        &mut self,
        subject: &Expr,
        arms: &[CaseArm],
        else_arm: Option<&Expr>,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        if arms.is_empty() {
            return Err(TypeError::CaseHasNoArms { span });
        }
        let name = format!("$case{}", self.cases);
        self.cases = self.cases.saturating_add(1);
        let subject_typed = self.expr(subject, None)?;
        let subject_ty = subject_typed.ty;
        let subject_ref = Expr::Var {
            name: name.clone(),
            span: subject.span(),
        };

        self.scopes.push(HashMap::new());
        self.declare(name.clone(), subject_ty, false);
        let built = self.case_chain(&subject_ref, arms, else_arm, span, expected);
        self.scopes.pop();
        let (chain, ty) = built?;

        Ok(TypedExpr {
            kind: TypedExprKind::Block {
                items: vec![TypedBlockItem::Binding {
                    name,
                    ty: subject_ty,
                    value: subject_typed,
                    mutable: false,
                    span: subject.span(),
                }],
                tail: Some(Box::new(chain)),
            },
            ty,
            span,
        })
    }

    /// The arms, folded into `if`/`elif` from the bottom up. Each guard is
    /// compared through `infix`, so which types `=` is defined on -- and what
    /// it means on an object -- are exactly the rules that were already there,
    /// with the diagnostics that were already written.
    fn case_chain(
        &mut self,
        subject_ref: &Expr,
        arms: &[CaseArm],
        else_arm: Option<&Expr>,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<(TypedExpr, Type)> {
        let mut conditions = Vec::with_capacity(arms.len());
        let mut bodies = Vec::with_capacity(arms.len());
        let mut result = expected;
        for arm in arms {
            let test = Expr::Infix {
                op: BinOp::Eq,
                fixity: fortress_ast::Fixity::Loose,
                lhs: Box::new(subject_ref.clone()),
                rhs: Box::new(arm.guard.clone()),
                span: arm.guard.span(),
            };
            conditions.push(self.expr(&test, Some(Type::Boolean))?);
            let body = self.expr(&arm.body, result)?;
            result = result.or(Some(body.ty));
            bodies.push(body);
        }
        let otherwise = match else_arm {
            Some(e) => Some(self.expr(e, result)?),
            None => None,
        };
        let ty = result.unwrap_or(Type::Void);
        for body in bodies.iter().chain(otherwise.as_ref()) {
            if body.ty != ty {
                return Err(TypeError::BranchTypeMismatch {
                    span: body.span,
                    then_type: ty,
                    else_type: body.ty,
                });
            }
        }

        // THE FALLTHROUGH. 1.0 throws MatchFailure when nothing matches
        // (case-expr.tex); this subset has no exceptions, so a `case` used for
        // its effect HALTS with a diagnostic rather than doing nothing -- the
        // same answer `assert` and the dispatch tree give. A `case` whose VALUE
        // is used cannot halt into a value, so that one is refused instead.
        let mut chain = match otherwise {
            Some(e) => e,
            None if ty == Type::Void => TypedExpr {
                kind: TypedExprKind::Apply {
                    target: Target::CaseFailed,
                    args: Vec::new(),
                },
                ty: Type::Void,
                span,
            },
            None => return Err(TypeError::CaseNeedsElse { span }),
        };
        for (cond, body) in conditions.into_iter().zip(bodies).rev() {
            chain = TypedExpr {
                kind: TypedExprKind::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(body),
                    else_branch: Some(Box::new(chain)),
                },
                ty,
                span,
            };
        }
        Ok((chain, ty))
    }

    /// `typecase`, and it is a real node rather than a desugaring: there is no
    /// expression in this language that reads a tag, and inventing one to
    /// desugar through would be a second way to do what M3c's dispatch tree
    /// already does.
    ///
    /// FIRST ARM WINS. A tag claimed by an earlier arm is removed from every
    /// later one, so the switch has one entry per tag; an arm left with no tag
    /// at all is refused, because it is dead code the reader believes in.
    fn typecase_expr(
        &mut self,
        subject: &Expr,
        arms: &[TypeCaseArm],
        else_arm: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let subject = self.expr(subject, None)?;
        if !subject.ty.is_reference() {
            return Err(TypeError::TypeCaseSubjectNotReference {
                span: subject.span,
                found: subject.ty,
            });
        }

        let mut claimed: BTreeSet<u32> = BTreeSet::new();
        let mut typed_arms: Vec<TypedTypeCaseArm> = Vec::new();
        let mut result: Option<Type> = expected;
        for arm in arms {
            let ty = self.registry.resolve(&arm.ty)?;
            if !self.registry.is_subtype(ty, subject.ty) {
                return Err(TypeError::TypeCaseArmUnrelated {
                    span: arm.span,
                    subject: subject.ty,
                    arm: ty,
                });
            }
            let tags: Vec<u32> = self
                .concrete_tags(ty)
                .into_iter()
                .filter(|tag| !claimed.contains(tag))
                .collect();
            if tags.is_empty() {
                return Err(TypeError::TypeCaseArmDead {
                    span: arm.span,
                    arm: ty,
                });
            }
            claimed.extend(tags.iter().copied());

            self.scopes.push(HashMap::new());
            if let Some(binder) = &arm.binder {
                self.declare(binder.clone(), ty, false);
            }
            let body = self.expr(&arm.body, result);
            self.scopes.pop();
            let body = body?;
            let body_ty = body.ty;
            typed_arms.push(TypedTypeCaseArm {
                tags,
                binder: arm.binder.clone(),
                ty,
                body,
            });
            result = result.or(Some(body_ty));
        }

        let else_branch = self.expr(else_arm, result)?;
        let ty = result.unwrap_or(else_branch.ty);
        if else_branch.ty != ty {
            return Err(TypeError::BranchTypeMismatch {
                span,
                then_type: ty,
                else_type: else_branch.ty,
            });
        }
        for arm in &typed_arms {
            if arm.body.ty != ty {
                return Err(TypeError::BranchTypeMismatch {
                    span: arm.body.span,
                    then_type: ty,
                    else_type: arm.body.ty,
                });
            }
        }
        Ok(TypedExpr {
            kind: TypedExprKind::TypeCase {
                subject: Box::new(subject),
                arms: typed_arms,
                else_branch: Box::new(else_branch),
            },
            ty,
            span,
        })
    }

    /// Every concrete tag a value of this type can carry. A trait is the closed
    /// set below it -- whole program, so the set is complete -- and an object is
    /// itself.
    fn concrete_tags(&self, ty: Type) -> Vec<u32> {
        match ty {
            Type::Object(name) => self.registry.tag_of(name).into_iter().collect(),
            Type::Trait(name) => self
                .registry
                .concretes_below(name)
                .into_iter()
                .filter_map(|concrete| self.registry.tag_of(concrete))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `for x <- a do ... end` over an ARRAY, desugared onto the indexed loop
    /// that already exists:
    ///
    /// ```text
    /// for $k <- 0 # length(a) do
    ///     x = a[$k]
    ///     <body, unchanged>
    /// end
    /// ```
    ///
    /// It is here rather than in the parser because the binder's type is the
    /// ARRAY'S ELEMENT TYPE, which the parser cannot know -- the same reason
    /// `BigReduction` is a node. `Elem` has five scalar kinds and nothing else,
    /// so the binder is always a scalar and nothing downstream needs a line for
    /// it: a reduction in the body is recognised by the same three-step
    /// ordering, `a[$k]` is an ordinary bounds-checked element read, and
    /// `length` is one of the builtins the shared-array guard leaves alone.
    ///
    /// THE SOURCE IS EVALUATED ONCE, into a binding the body cannot see the
    /// name of. `for x <- makeArray() do ... end` must not build an array per
    /// iteration.
    fn for_in(
        &mut self,
        binder: &str,
        source: &Expr,
        sequential: bool,
        body: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let typed_source = self.expr(source, None)?;
        let Type::Array(_) = typed_source.ty else {
            return Err(TypeError::NotAnArray {
                span: source.span(),
                found: typed_source.ty,
            });
        };
        let array = format!("$in{}", self.cases);
        let index = format!("$at{}", self.cases);
        self.cases = self.cases.saturating_add(1);

        let array_ref = || Expr::Var {
            name: array.clone(),
            span,
        };
        let desugared = Expr::Block {
            items: vec![
                BlockItem::Binding(fortress_ast::Binding {
                    name: array.clone(),
                    ty: None,
                    value: source.clone(),
                    mutable: false,
                    span,
                }),
                BlockItem::Expr(Expr::For {
                    binder: index.clone(),
                    lo: Box::new(Expr::IntLit {
                        digits: "0".to_owned(),
                        span,
                    }),
                    hi: Box::new(Expr::Call {
                        callee: Box::new(Expr::Var {
                            name: "length".to_owned(),
                            span,
                        }),
                        args: vec![array_ref()],
                        span,
                    }),
                    inclusive: false,
                    sequential,
                    body: Box::new(Expr::Block {
                        items: vec![
                            BlockItem::Binding(fortress_ast::Binding {
                                name: binder.to_owned(),
                                ty: None,
                                value: Expr::Index {
                                    base: Box::new(array_ref()),
                                    index: Box::new(Expr::Var { name: index, span }),
                                    span,
                                },
                                mutable: false,
                                span,
                            }),
                            BlockItem::Expr(body.clone()),
                        ],
                        span,
                    }),
                    span,
                }),
            ],
            span,
        };
        self.expr(&desugared, expected)
    }

    /// `do A also do B end`, SERIALISED, and that is a deviation with a licence
    /// rather than a shortcut.
    ///
    /// `also.tex:17-21` makes each block an implicit thread of one group.
    /// `parallelism.tex:88-90` permits an implementation to serialise any group
    /// of implicit threads -- and `also.tex:24-27` requires every block, and the
    /// group, to have type `()`, so there is no value to combine and nothing
    /// else the group could have meant. Running them in order is a legal
    /// schedule.
    ///
    /// WHY NOT THE PARALLEL LOWERING, measured rather than assumed. Desugaring
    /// to a two-iteration `for` buys nothing and costs the corpus: the runtime
    /// runs any range below `FORTRESS_PARALLEL_MIN` (4096) inline, so a
    /// two-block group never distributes; and the loop rules would then refuse
    /// nearly every real site, because an `also` block assigns enclosing locals
    /// non-atomically as a matter of routine -- `AlsoDo.fss:22`, `atomic5.fss:28`,
    /// `Expr.Do.treeSum.fss:27`. Real parallelism needs a task with a handle,
    /// which the one-broadcast-one-join pool does not have.
    fn also_do(
        &mut self,
        blocks: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        self.require(Type::Void, expected, span)?;
        let mut items = Vec::with_capacity(blocks.len());
        for block in blocks {
            // Checked with NO expectation, then required to be Void here: with
            // `Some(Void)` pushed down, `do 3 also do 5 end` fails on the
            // literal and reports a generic mismatch, where the rule is about
            // the BLOCK. The legacy implementation names the block too --
            // XXX10a.test expects "do-also expression has type IntLiteral, but
            // it must have () type".
            let typed = self.expr(block, None)?;
            if typed.ty != Type::Void {
                return Err(TypeError::AlsoBlockNotVoid {
                    span: typed.span,
                    found: typed.ty,
                });
            }
            items.push(TypedBlockItem::Expr(typed));
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Block { items, tail: None },
            ty: Type::Void,
            span,
        })
    }

    /// `SUM[i <- lo:hi] e`, lowered onto the M5 accumulator and nothing else.
    ///
    /// `reductions.tex:60-77` desugars it to
    /// `do var r = identity; for i <- lo:hi do r OP= e end; r end`, which is
    /// EXACTLY the shape M5's recogniser already turns into a per-worker
    /// private accumulator. So this needs no `Reduction` trait, no generator
    /// protocol and no closure -- which is why the gap analysis calls it the
    /// one separable part of the generator chain.
    ///
    /// THE ACCUMULATOR'S TYPE IS THE BODY'S, and that is why the desugaring is
    /// here rather than in the parser: `SUM[i <- 1:10] i` accumulates ZZ64,
    /// because a loop binder is ZZ64, and the parser cannot know that. The
    /// context supplies it when there is one -- `total: ZZ32 = SUM[...]` -- and
    /// otherwise the body is walked SPECULATIVELY for its type alone.
    #[allow(clippy::too_many_arguments)]
    fn big_reduction(
        &mut self,
        op: BinOp,
        binder: &str,
        lo: &Expr,
        hi: &Expr,
        inclusive: bool,
        sequential: bool,
        body: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let ty = match expected {
            Some(t) if Self::reducible(t) => t,
            _ => self.probe_type(body, binder, span)?,
        };
        if !Self::reducible(ty) {
            return Err(TypeError::Mismatch {
                span,
                found: ty,
                required: Type::ZZ64,
            });
        }
        let name = format!("$big{}", self.cases);
        self.cases = self.cases.saturating_add(1);

        // THE IDENTITY IS A TYPED CONSTANT AND NOT A LITERAL, because MAX and
        // MIN need the type's own extremum and no literal in this language
        // spells `i64::MIN` or an infinity. Everything else about the
        // desugaring is AST and goes through the ordinary checker; only this
        // one value is built directly.
        let identity = TypedExpr {
            kind: match (op, ty) {
                (BinOp::Max, Type::RR64) => TypedExprKind::FloatConst(f64::NEG_INFINITY),
                (BinOp::Min, Type::RR64) => TypedExprKind::FloatConst(f64::INFINITY),
                (_, Type::RR64) => {
                    TypedExprKind::FloatConst(if op == BinOp::Mul { 1.0 } else { 0.0 })
                }
                (BinOp::Max, Type::ZZ32) => TypedExprKind::IntConst(i128::from(i32::MIN)),
                (BinOp::Min, Type::ZZ32) => TypedExprKind::IntConst(i128::from(i32::MAX)),
                (BinOp::Max, _) => TypedExprKind::IntConst(i128::from(i64::MIN)),
                (BinOp::Min, _) => TypedExprKind::IntConst(i128::from(i64::MAX)),
                (BinOp::Mul, _) => TypedExprKind::IntConst(1),
                _ => TypedExprKind::IntConst(0),
            },
            ty,
            span,
        };

        let loop_ast = Expr::For {
            binder: binder.to_owned(),
            lo: Box::new(lo.clone()),
            hi: Box::new(hi.clone()),
            inclusive,
            sequential,
            body: Box::new(Expr::Block {
                items: vec![BlockItem::Assign(Assign {
                    target: Expr::Var {
                        name: name.clone(),
                        span,
                    },
                    op: Some(op),
                    value: body.clone(),
                    span,
                })],
                span,
            }),
            span,
        };

        // The accumulator is declared HERE rather than by checking a synthetic
        // binding, so that its initial value can be the constant above. The
        // loop and the final read are ordinary checking.
        self.scopes.push(HashMap::new());
        self.declare(name.clone(), ty, true);
        let lowered = self.expr(&loop_ast, Some(Type::Void));
        self.scopes.pop();
        let lowered = lowered?;

        Ok(TypedExpr {
            kind: TypedExprKind::Block {
                items: vec![
                    TypedBlockItem::Binding {
                        name: name.clone(),
                        ty,
                        value: identity,
                        mutable: true,
                        span,
                    },
                    TypedBlockItem::Expr(lowered),
                ],
                tail: Some(Box::new(TypedExpr {
                    kind: TypedExprKind::Var(name),
                    ty,
                    span,
                })),
            },
            ty,
            span,
        })
    }

    /// The type of an expression, walked for that alone.
    ///
    /// The walk has side effects -- it memoises dispatch tables, numbers
    /// outlined loop bodies, and records captures into an enclosing loop
    /// context -- so the two that would be WRONG to keep are saved and put
    /// back. A memoised dispatch table is not one of them: the real walk
    /// computes the same table from the same types, and `or_insert_with` keeps
    /// the first.
    fn probe_type(&mut self, body: &Expr, binder: &str, span: Span) -> Checked<Type> {
        let loops = self.loops;
        let saved = self.loop_ctx.clone();
        let mut scope = HashMap::new();
        scope.insert(
            binder.to_owned(),
            Local {
                ty: Type::ZZ64,
                mutable: false,
            },
        );
        self.scopes.push(scope);
        let probed = self.expr(body, None);
        self.scopes.pop();
        self.loops = loops;
        self.loop_ctx = saved;
        let _ = span;
        Ok(probed?.ty)
    }

    /// `label L ... end L`. The label's type is fixed by the FIRST `exit` that
    /// carries a value, and everything else -- later exits, the body's own
    /// fallthrough -- is checked against it. It cannot be taken from the body
    /// instead: the body has not been walked yet when the first exit is.
    fn label_expr(
        &mut self,
        name: &str,
        body: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        if self.labels.iter().any(|l| l.name == name) {
            return Err(TypeError::LabelAlreadyOpen {
                span,
                name: name.to_owned(),
            });
        }
        self.labels.push(LabelCtx {
            name: name.to_owned(),
            ty: expected,
            atomic_depth: self.atomic_depth,
            loop_depth: self.loop_ctx.len(),
        });
        let body = self.expr(body, expected);
        let ctx = self.labels.pop();
        let body = body?;
        let ty = ctx.and_then(|c| c.ty).unwrap_or(body.ty);
        // A label whose exits carry a value and whose body can also fall out of
        // the bottom needs a value on that edge too. 1.0 has no answer for it
        // either; refusing beats inventing a zero and printing it.
        if body.ty != ty {
            return Err(TypeError::LabelFallsThrough {
                span,
                name: name.to_owned(),
                expected: ty,
                found: body.ty,
            });
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Label {
                name: name.to_owned(),
                body: Box::new(body),
            },
            ty,
            span,
        })
    }

    /// `exit L with e`, and the two crossings that have to be refused.
    ///
    /// AN `atomic` BOUNDARY: the branch would skip `fortress_atomic_leave` and
    /// leave one process-wide RECURSIVE mutex held for the rest of the process.
    /// `atomic.tex:59-70`'s rollback rule has two arms and this is the one
    /// `label`/`exit` re-opens; until there is an answer, the crossing is a
    /// diagnostic rather than a deadlock.
    ///
    /// A LOOP BOUNDARY: every `for` body is OUTLINED into its own function --
    /// `seq(...)` included, because one lowering serves both -- so a branch out
    /// of it is a jump between functions, which is exactly the unwinding this
    /// construct was chosen for not needing.
    fn exit_expr(
        &mut self,
        name: Option<&str>,
        value: Option<&Expr>,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
        let Some(index) = (match name {
            Some(wanted) => self.labels.iter().rposition(|l| l.name == wanted),
            None => self.labels.len().checked_sub(1),
        }) else {
            return Err(TypeError::UnknownLabel {
                span,
                name: name.unwrap_or("").to_owned(),
            });
        };
        let (label, atomic_depth, loop_depth, declared) = {
            let ctx = self.labels.get(index).ok_or(TypeError::UnknownLabel {
                span,
                name: name.unwrap_or("").to_owned(),
            })?;
            (ctx.name.clone(), ctx.atomic_depth, ctx.loop_depth, ctx.ty)
        };
        if self.atomic_depth > atomic_depth {
            return Err(TypeError::ExitCrossesAtomic { span, name: label });
        }
        if self.loop_ctx.len() > loop_depth {
            return Err(TypeError::ExitCrossesLoop { span, name: label });
        }

        let value = match value {
            Some(e) => Some(Box::new(self.expr(e, declared)?)),
            None => None,
        };
        let carried = value.as_ref().map_or(Type::Void, |v| v.ty);
        if let Some(want) = declared {
            if carried != want {
                return Err(TypeError::ExitTypeMismatch {
                    span,
                    name: label,
                    expected: want,
                    found: carried,
                });
            }
        } else if let Some(ctx) = self.labels.get_mut(index) {
            ctx.ty = Some(carried);
        }
        Ok(TypedExpr {
            kind: TypedExprKind::Exit { name: label, value },
            // BOTTOM, spelled with the context instead of with a type. Control
            // never comes back from an `exit`, so it fits wherever it is
            // written: as the tail of a function body it is that function's
            // return type, and in statement position -- inside `if c then exit
            // L with i end`, which is the shape the corpus writes -- it is
            // Void, so the `if` needs no `else`.
            //
            // A `Never` variant on `Type` would say this properly. It would
            // also touch every exhaustive match in the workspace, which is
            // SPIKE-COMPOSITE-TYPE's decision and not this one's. The cost of
            // spelling it this way: `if c then exit L with 1 else 2 end` in a
            // position with NO expected type refuses, because the `then` side
            // takes Void from its context and the `else` side is then checked
            // against it. Give the expression a type -- a declared return type,
            // an annotated binding -- and it goes through.
            ty: expected.unwrap_or(Type::Void),
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
                    self.taint_if_aliased(&b.name, ty, &b.value);
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
                    if index != last {
                        refuse_field_assignment(e)?;
                    }
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

/// `o.f = v` in statement position. The parser cannot refuse this: `try_binding`
/// only recognises a bare `Ident` before the `=`, so `b.x = 7` falls through to
/// the expression path and becomes an ordinary `Eq` comparison whose value is
/// then thrown away. Statement position exists only here, in the checker.
///
/// Scoped to a FIELD target on purpose. `blocks.tex:49-63` invalidates every
/// unparenthesised statement-position equality, but the wider rule costs two
/// corpus files that compile today (`Compiled5.ac.fss`, `Compiled9.r.fss`) and
/// the widest -- every non-final item must have type `()` -- costs at least
/// three; both need the compile floor re-ratcheted deliberately. A field target
/// costs nothing: no corpus file writes one, because a mutable field is refused
/// at its declaration.
///
/// `atomic` is looked through because `atomic b.x = 7` is parsed as an `Atomic`
/// wrapping the `Infix` directly rather than as a block.
fn refuse_field_assignment(e: &Expr) -> Checked<()> {
    let inner = match e {
        Expr::Atomic { body, .. } => body,
        other => other,
    };
    if let Expr::Infix {
        op: BinOp::Eq, lhs, ..
    } = inner
    {
        if let Expr::Field { name, span, .. } = &**lhs {
            return Err(TypeError::FieldAssignmentUnsupported {
                span: *span,
                name: name.clone(),
            });
        }
    }
    Ok(())
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

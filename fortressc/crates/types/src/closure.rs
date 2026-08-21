//! SPIKE-CLOSURE-REPRESENTATION, branch (b): a function used as a value is
//! lowered to a generated object with an `apply` method, and the call on it
//! enters M3c's whole-program dispatch like any other dotted method call.
//!
//! THE BRANCH THIS ANSWERS. The gap analysis gives two: (a) a fat pointer,
//! which needs `Type` to grow a boxed or interned composite variant and stop
//! being `Copy`, touching every pass; and (b) a generated object, which reuses
//! machinery that works but mints a tag per closure site. (b) is implemented
//! here on the smallest case it can be implemented on -- a NAMED function
//! passed as an argument, no `fn` syntax at all -- because that is what
//! answers the representation question without any syntax work riding on it.
//!
//! WHERE IT RUNS, and this is the load-bearing part. Tags and
//! `registry.concrete` freeze in `Checker::new`, and expansion is an
//! AST-to-AST pass that runs to a fixpoint before that. This pass sits BETWEEN
//! them: it sees a component that is already ground, so it never meets a
//! static parameter, and everything it appends is in place before the registry
//! is built. `check` calls the three in order, so the order cannot be got
//! wrong.
//!
//! WHAT IT DOES NOT DO, all deliberate and all named:
//!   * `fn` syntax, so no anonymous closure and no captured environment. A
//!     generated object here has ZERO constructor parameters -- but it is
//!     `Some(vec![])` and not `None`, so it is constructed at each use rather
//!     than being a singleton, because captures-as-constructor-parameters is
//!     the shape the real feature takes and the difference would be invisible
//!     until the day it mattered.
//!   * Arrows whose domain is a tuple, or an arrow inside a generic body. Both
//!     keep the diagnostic they already have.
//!   * The synthetic objects are NOT counted against `MAX_INSTANTIATIONS`,
//!     which counts `instances` plus `stamps` in `mono`. One object per
//!     (function, arrow) pair is bounded by the source, but a follow-up should
//!     fold them into the same total.

use std::collections::{BTreeMap, BTreeSet};

use fortress_ast::{
    Assign, Binding, BlockItem, CaseArm, Component, Decl, Expr, FnDecl, Member, MethodDecl,
    Modifiers, ObjectDecl, Param, Span, TypeCaseArm, TypeRef,
};

use crate::error::TypeError;

/// The trait a `A -> B` parameter becomes, and the method every generated
/// object implements. `$` cannot be lexed, so neither name can collide with
/// anything a source file wrote -- the same injectivity argument
/// `mangle_static` uses.
const APPLY: &str = "apply";
/// Every minted trait starts with this, which is how a rewritten parameter type
/// is told from a user's own type name without carrying the map around.
const TRAIT_PREFIX: &str = "Arrow$";

pub(crate) fn lower(component: &Component) -> Result<Component, TypeError> {
    let mut pass = Pass {
        functions: BTreeMap::new(),
        traits: BTreeMap::new(),
        objects: BTreeMap::new(),
        known: BUILTIN_TYPES.iter().map(|s| (*s).to_owned()).collect(),
    };
    for decl in &component.decls {
        match decl {
            Decl::Function(f) => pass
                .functions
                .entry(f.name.clone())
                .or_default()
                .push(f.clone()),
            Decl::Trait(t) => {
                pass.known.insert(t.name.clone());
            }
            Decl::Object(o) => {
                pass.known.insert(o.name.clone());
            }
        }
    }
    if !pass.uses_arrows(component) {
        return Ok(component.clone());
    }
    pass.run(component)
}

/// One arrow signature, keyed by what the source wrote. Two spellings of the
/// same type are the same key because `TypeRef::written` is canonical for the
/// ground types this pass can see.
type ArrowKey = (String, String);

/// The scalar type names the checker knows without a declaration, plus the
/// array constructor. A minted trait must not name anything else that the
/// component does not declare: an abstract trait member's parameter types are
/// resolved by nothing today, so an unknown name inside one would be accepted
/// in silence -- and on master the same source was refused, because the arrow
/// itself was.
const BUILTIN_TYPES: [&str; 6] = ["ZZ32", "ZZ64", "RR64", "Boolean", "String", "Array"];

struct Pass {
    /// Every top-level function, by name. A name may have several declarations:
    /// that is an overload set, and a value use of one is ambiguous unless the
    /// arrow the context asks for picks exactly one.
    functions: BTreeMap<String, Vec<FnDecl>>,
    /// Arrow signature to the trait minted for it. Ordered, because tags follow
    /// declaration order and declaration order has to be a fact about the
    /// source rather than about a hash.
    traits: BTreeMap<ArrowKey, ArrowTrait>,
    /// `(function name, arrow)` to the object minted for it.
    objects: BTreeMap<(String, ArrowKey), ObjectDecl>,
    /// Every type name this component declares, plus the builtins.
    known: BTreeSet<String>,
}

struct ArrowTrait {
    name: String,
    from: TypeRef,
    to: TypeRef,
}

fn sanitize(written: &str) -> String {
    written
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn arrow_key(from: &TypeRef, to: &TypeRef) -> ArrowKey {
    (from.written(), to.written())
}

impl Pass {
    /// A cheap precheck. A component with no arrow anywhere is returned
    /// untouched and emits byte-identical IR, which is what keeps the corpus
    /// number and every existing gate exactly where they were.
    fn uses_arrows(&self, component: &Component) -> bool {
        component.decls.iter().any(|d| match d {
            Decl::Function(f) => {
                f.params.iter().any(|p| is_arrow(&p.ty))
                    || f.return_type.as_ref().is_some_and(is_arrow)
            }
            Decl::Trait(t) => t.members.iter().any(member_uses_arrow),
            Decl::Object(o) => {
                o.params.iter().flatten().any(|p| is_arrow(&p.ty))
                    || o.members.iter().any(member_uses_arrow)
            }
        })
    }

    fn run(&mut self, component: &Component) -> Result<Component, TypeError> {
        // Pass one: every arrow that appears in a signature mints its trait.
        // Signatures only -- a local binding takes its type from its
        // initializer, and the initializer is a function name whose arrow comes
        // from the parameter it is being handed to.
        let mut decls = component.decls.clone();
        for decl in &mut decls {
            match decl {
                Decl::Function(f) => {
                    for p in &mut f.params {
                        self.rewrite_type(&mut p.ty);
                    }
                    if let Some(t) = &mut f.return_type {
                        self.rewrite_type(t);
                    }
                }
                Decl::Trait(t) => {
                    for m in &mut t.members {
                        self.rewrite_member_types(m);
                    }
                }
                Decl::Object(o) => {
                    for p in o.params.iter_mut().flatten() {
                        self.rewrite_type(&mut p.ty);
                    }
                    for m in &mut o.members {
                        self.rewrite_member_types(m);
                    }
                }
            }
        }

        // The signature map has to be REBUILT from the rewritten declarations.
        // It was collected before pass one, so its parameter types are still
        // `TypeRef::Arrow`, and pass two asks "is this parameter one of the
        // minted traits" -- a question the stale copy answers `no` to for every
        // parameter, silently, which is exactly what the first draft did.
        self.functions.clear();
        for decl in &decls {
            if let Decl::Function(f) = decl {
                self.functions
                    .entry(f.name.clone())
                    .or_default()
                    .push(f.clone());
            }
        }

        // Pass two: bodies. A function name in a slot that wants an arrow
        // becomes a construction of the object minted for it, and a call on an
        // arrow-typed name becomes a dotted `apply`.
        for decl in &mut decls {
            match decl {
                Decl::Function(f) => {
                    let mut scope = Scope::default();
                    for p in &f.params {
                        scope.declare(&p.name, &p.ty);
                    }
                    if let Some(body) = &mut f.body {
                        self.rewrite_expr(body, &mut scope)?;
                    }
                }
                Decl::Trait(t) => {
                    for m in &mut t.members {
                        self.rewrite_member_body(m)?;
                    }
                }
                Decl::Object(o) => {
                    for m in &mut o.members {
                        self.rewrite_member_body(m)?;
                    }
                }
            }
        }

        // Ordered append, so a tag is a fact about the source. `traits` and
        // `objects` are both BTreeMaps for exactly this reason: with a HashMap
        // the emitted object file would depend on iteration order.
        for arrow in self.traits.values() {
            decls.push(Decl::Trait(fortress_ast::TraitDecl {
                modifiers: Modifiers::default(),
                name: arrow.name.clone(),
                static_params: Vec::new(),
                extends: Vec::new(),
                comprises: Vec::new(),
                excludes: Vec::new(),
                members: vec![Member::Method(MethodDecl {
                    modifiers: Modifiers::default(),
                    name: APPLY.to_owned(),
                    static_params: Vec::new(),
                    params: vec![Param {
                        name: "x$0".to_owned(),
                        ty: arrow.from.clone(),
                        span: Span::new(0, 0),
                    }],
                    return_type: Some(arrow.to.clone()),
                    // No body: an abstract method, which is what keeps it out
                    // of the dispatch table's winners and makes every concrete
                    // implementor the only candidate for its own tag.
                    body: None,
                    accessor: false,
                    span: Span::new(0, 0),
                })],
                span: Span::new(0, 0),
            }));
        }
        for object in self.objects.values() {
            decls.push(Decl::Object(object.clone()));
        }

        Ok(Component {
            decls,
            ..component.clone()
        })
    }

    fn rewrite_member_types(&mut self, m: &mut Member) {
        match m {
            Member::Field(f) => self.rewrite_type(&mut f.ty),
            Member::Method(m) => {
                for p in &mut m.params {
                    self.rewrite_type(&mut p.ty);
                }
                if let Some(t) = &mut m.return_type {
                    self.rewrite_type(t);
                }
            }
        }
    }

    fn rewrite_member_body(&mut self, m: &mut Member) -> Result<(), TypeError> {
        let Member::Method(method) = m else {
            return Ok(());
        };
        let mut scope = Scope::default();
        for p in &method.params {
            scope.declare(&p.name, &p.ty);
        }
        if let Some(body) = &mut method.body {
            self.rewrite_expr(body, &mut scope)?;
        }
        Ok(())
    }

    /// Innermost first, so `(A -> B) -> C` mints the trait for `A -> B` before
    /// the one that names it and the composition falls out with no special
    /// case.
    fn rewrite_type(&mut self, t: &mut TypeRef) {
        match t {
            TypeRef::Arrow { from, to, span } => {
                self.rewrite_type(from);
                self.rewrite_type(to);
                // NOT EVERY ARROW IS LIFTABLE, and one that is not must be left
                // exactly as it was so that `Registry::resolve` gives it the
                // refusal it already had. Minting a trait whose `apply` takes a
                // tuple turned `(ZZ32, ZZ32) -> ZZ32` from a diagnostic on
                // master into a silent exit 0 -- the worst class this project
                // recognises, introduced by the pass meant to add a feature.
                if !self.liftable(from) || !self.liftable(to) {
                    return;
                }
                let key = arrow_key(from, to);
                let name = self
                    .traits
                    .entry(key)
                    .or_insert_with(|| ArrowTrait {
                        name: format!(
                            "{TRAIT_PREFIX}{}${}",
                            sanitize(&from.written()),
                            sanitize(&to.written())
                        ),
                        from: (**from).clone(),
                        to: (**to).clone(),
                    })
                    .name
                    .clone();
                *t = TypeRef::Named {
                    name,
                    args: Vec::new(),
                    span: *span,
                };
            }
            TypeRef::Named { args, .. } => {
                for a in args {
                    self.rewrite_type(a);
                }
            }
            TypeRef::Tuple { elems, .. } => {
                for e in elems {
                    self.rewrite_type(e);
                }
            }
            TypeRef::Unit { .. } => {}
        }
    }

    /// The object for `name` seen at arrow `key`. Minted once per pair: two
    /// call sites handing the same function to the same arrow share a tag.
    fn object_for(&mut self, name: &str, key: &ArrowKey, span: Span) -> Result<String, TypeError> {
        let arrow = self.traits.get(key).ok_or_else(|| TypeError::UnknownName {
            span,
            name: name.to_owned(),
        })?;
        let trait_name = arrow.name.clone();
        let from = arrow.from.clone();
        let to = arrow.to.clone();
        let object_name = format!("{name}$fn${trait_name}");
        if self.objects.contains_key(&(name.to_owned(), key.clone())) {
            return Ok(object_name);
        }

        // The function has to EXIST and its signature has to be the arrow's,
        // because nothing downstream will check it: after this pass the object
        // is an ordinary implementor and its `apply` body is an ordinary call.
        let candidates = self
            .functions
            .get(name)
            .ok_or_else(|| TypeError::UnknownName {
                span,
                name: name.to_owned(),
            })?;
        let matching: Vec<&FnDecl> = candidates
            .iter()
            .filter(|f| {
                f.params.len() == 1
                    && f.params
                        .first()
                        .is_some_and(|p| p.ty.written() == from.written())
                    && f.return_type
                        .as_ref()
                        .is_some_and(|r| r.written() == to.written())
            })
            .collect();
        if matching.len() != 1 {
            return Err(TypeError::FunctionValueUnresolved {
                span,
                name: name.to_owned(),
                arrow: format!("{} -> {}", from.written(), to.written()),
                found: matching.len(),
            });
        }

        let body = Expr::Call {
            callee: Box::new(Expr::Var {
                name: name.to_owned(),
                span,
            }),
            args: vec![Expr::Var {
                name: "x$0".to_owned(),
                span,
            }],
            span,
        };
        let object = ObjectDecl {
            modifiers: Modifiers::default(),
            name: object_name.clone(),
            static_params: Vec::new(),
            // `Some(vec![])` and NOT `None`: `None` is a singleton, built once
            // between `fortress_runtime_init` and `run`. A closure is built
            // where it is written, and its captures will be these parameters.
            params: Some(Vec::new()),
            extends: vec![TypeRef::Named {
                name: trait_name,
                args: Vec::new(),
                span,
            }],
            comprises: Vec::new(),
            excludes: Vec::new(),
            members: vec![Member::Method(MethodDecl {
                modifiers: Modifiers::default(),
                name: APPLY.to_owned(),
                static_params: Vec::new(),
                // No `self` parameter: a `self` parameter marks a FUNCTIONAL
                // method, which lifts into the top-level overload set and is a
                // different namespace entirely. `apply` has to be dotted.
                params: vec![Param {
                    name: "x$0".to_owned(),
                    ty: from,
                    span,
                }],
                return_type: Some(to),
                body: Some(body),
                accessor: false,
                span,
            })],
            span,
        };
        self.objects.insert((name.to_owned(), key.clone()), object);
        Ok(object_name)
    }

    fn rewrite_exprs(&mut self, items: &mut [Expr], scope: &mut Scope) -> Result<(), TypeError> {
        for item in items {
            self.rewrite_expr(item, scope)?;
        }
        Ok(())
    }

    fn rewrite_expr(&mut self, e: &mut Expr, scope: &mut Scope) -> Result<(), TypeError> {
        match e {
            Expr::Unit { .. }
            | Expr::IntLit { .. }
            | Expr::FloatLit { .. }
            | Expr::StrLit { .. }
            | Expr::BoolLit { .. }
            | Expr::Var { .. } => Ok(()),

            Expr::Call { callee, args, span } => {
                // `f(x)` where `f` is an arrow-typed NAME in scope: the call is
                // a dotted `apply`, which is where M3c's dispatch takes over.
                // A local wins over a top-level function, which is the
                // shadowing rule `juxtshadow.fss` already covers for
                // juxtaposition.
                if self.arrow_of(callee, scope).is_some() {
                    let mut receiver = callee.clone();
                    self.rewrite_expr(&mut receiver, scope)?;
                    self.rewrite_exprs(args, scope)?;
                    let dot = receiver.span();
                    // Into the box that is already there: the callee slot is
                    // a `Box<Expr>` and reusing it saves an allocation per
                    // rewritten call.
                    **callee = Expr::Field {
                        base: receiver,
                        name: APPLY.to_owned(),
                        span: dot,
                    };
                    return Ok(());
                }
                let wanted = match callee.as_ref() {
                    Expr::Var { name, .. } if scope.get(name).is_none() => {
                        self.arrow_parameters(name, args.len())
                    }
                    _ => Vec::new(),
                };
                self.rewrite_expr(callee, scope)?;
                for (index, arg) in args.iter_mut().enumerate() {
                    let slot = wanted.get(index).and_then(Option::as_ref);
                    if let (Some(key), Expr::Var { name, span: aspan }) = (slot, &*arg) {
                        if scope.get(name).is_none() && self.functions.contains_key(name) {
                            let object = self.object_for(name, key, *aspan)?;
                            *arg = Expr::Call {
                                callee: Box::new(Expr::Var {
                                    name: object,
                                    span: *aspan,
                                }),
                                args: Vec::new(),
                                span: *aspan,
                            };
                            continue;
                        }
                    }
                    self.rewrite_expr(arg, scope)?;
                }
                let _ = span;
                Ok(())
            }

            Expr::Tuple { items, .. } | Expr::Juxt { items, .. } | Expr::ArrayLit { items, .. } => {
                self.rewrite_exprs(items, scope)
            }
            Expr::Infix { lhs, rhs, .. } => {
                self.rewrite_expr(lhs, scope)?;
                self.rewrite_expr(rhs, scope)
            }
            Expr::Prefix { operand, .. } => self.rewrite_expr(operand, scope),
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.rewrite_expr(cond, scope)?;
                self.rewrite_expr(then_branch, scope)?;
                match else_branch {
                    Some(e) => self.rewrite_expr(e, scope),
                    None => Ok(()),
                }
            }
            Expr::Block { items, .. } => {
                scope.push();
                let result = self.rewrite_block(items, scope);
                scope.pop();
                result
            }
            Expr::Index { base, index, .. } => {
                self.rewrite_expr(base, scope)?;
                self.rewrite_expr(index, scope)
            }
            Expr::While { cond, body, .. } => {
                self.rewrite_expr(cond, scope)?;
                self.rewrite_expr(body, scope)
            }
            Expr::Field { base, .. } => self.rewrite_expr(base, scope),
            Expr::For {
                binder,
                lo,
                hi,
                body,
                ..
            } => {
                self.rewrite_expr(lo, scope)?;
                self.rewrite_expr(hi, scope)?;
                scope.push();
                scope.declare_opaque(binder);
                let result = self.rewrite_expr(body, scope);
                scope.pop();
                result
            }
            Expr::Instantiate { callee, .. } => self.rewrite_expr(callee, scope),
            Expr::Atomic { body, .. } => self.rewrite_expr(body, scope),
            Expr::Case {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.rewrite_expr(subject, scope)?;
                for arm in arms.iter_mut() {
                    let CaseArm { guard, body, .. } = arm;
                    self.rewrite_expr(guard, scope)?;
                    self.rewrite_expr(body, scope)?;
                }
                match else_arm {
                    Some(e) => self.rewrite_expr(e, scope),
                    None => Ok(()),
                }
            }
            Expr::TypeCase {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.rewrite_expr(subject, scope)?;
                for arm in arms.iter_mut() {
                    let TypeCaseArm { binder, body, .. } = arm;
                    scope.push();
                    if let Some(b) = binder {
                        scope.declare_opaque(b);
                    }
                    let result = self.rewrite_expr(body, scope);
                    scope.pop();
                    result?;
                }
                self.rewrite_expr(else_arm, scope)
            }
            Expr::Label { body, .. } => self.rewrite_expr(body, scope),
            Expr::Exit { value, .. } => match value {
                Some(e) => self.rewrite_expr(e, scope),
                None => Ok(()),
            },
        }
    }

    fn rewrite_block(
        &mut self,
        items: &mut [BlockItem],
        scope: &mut Scope,
    ) -> Result<(), TypeError> {
        for item in items {
            match item {
                BlockItem::Binding(b) => {
                    let Binding {
                        name, ty, value, ..
                    } = b;
                    if let Some(t) = ty {
                        self.rewrite_type(t);
                    }
                    self.rewrite_expr(value, scope)?;
                    match ty {
                        Some(t) => scope.declare(name, t),
                        None => scope.declare_opaque(name),
                    }
                }
                BlockItem::Assign(a) => {
                    let Assign { target, value, .. } = a;
                    self.rewrite_expr(target, scope)?;
                    self.rewrite_expr(value, scope)?;
                }
                BlockItem::Expr(e) => self.rewrite_expr(e, scope)?,
            }
        }
        Ok(())
    }

    /// Whether an arrow over this type can become a trait.
    ///
    /// A tuple and the unit type fail because `apply` would need a parameter
    /// this subset cannot store. A name the component does not declare fails
    /// for a subtler reason: an abstract trait member's parameter types are
    /// resolved by NOTHING today, so `Foo -> ZZ32` with no `Foo` would compile
    /// to exit 0 -- where master refused it, because the arrow itself was
    /// refused. An unliftable arrow is left exactly as it was and keeps that
    /// refusal.
    fn liftable(&self, t: &TypeRef) -> bool {
        match t {
            TypeRef::Named { name, args, .. } => {
                (self.known.contains(name) || name.starts_with(TRAIT_PREFIX))
                    && args.iter().all(|a| self.liftable(a))
            }
            TypeRef::Arrow { from, to, .. } => self.liftable(from) && self.liftable(to),
            TypeRef::Tuple { .. } | TypeRef::Unit { .. } => false,
        }
    }

    /// Whether an expression already HAS one of the minted trait types, so a
    /// call on it is an `apply` rather than a call to a function of that name.
    /// Two shapes only, and they are the two that arrow values travel by
    /// without any inference: a name in scope, and the result of a call to a
    /// function whose return type is an arrow. Anything else keeps the
    /// diagnostic it has -- this pass never guesses a type.
    fn arrow_of(&self, e: &Expr, scope: &Scope) -> Option<String> {
        match e {
            Expr::Var { name, .. } => match scope.get(name) {
                Some(Slot::Arrow(trait_name)) => Some(trait_name.clone()),
                _ => None,
            },
            Expr::Call { callee, args, .. } => {
                let Expr::Var { name, .. } = callee.as_ref() else {
                    return None;
                };
                if scope.get(name).is_some() {
                    return None;
                }
                let returns: BTreeSet<&str> = self
                    .functions
                    .get(name)?
                    .iter()
                    .filter(|f| f.params.len() == args.len())
                    .filter_map(|f| match f.return_type.as_ref() {
                        Some(TypeRef::Named { name, .. }) if name.starts_with(TRAIT_PREFIX) => {
                            Some(name.as_str())
                        }
                        _ => None,
                    })
                    .collect();
                // An overload set whose declarations disagree cannot pick one
                // here; leave it and let the checker report it.
                match returns.len() {
                    1 => returns.into_iter().next().map(ToOwned::to_owned),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Which parameters of `name` want an arrow, at this arity. Types have
    /// already been rewritten by pass one, so an arrow parameter is now a
    /// `Named` whose name is one of the minted traits -- which is why this
    /// looks the trait up by name rather than for `TypeRef::Arrow`.
    fn arrow_parameters(&self, name: &str, arity: usize) -> Vec<Option<ArrowKey>> {
        let by_name: BTreeMap<&str, &ArrowKey> = self
            .traits
            .iter()
            .map(|(key, arrow)| (arrow.name.as_str(), key))
            .collect();
        let mut out: Vec<Option<ArrowKey>> = vec![None; arity];
        let Some(candidates) = self.functions.get(name) else {
            return out;
        };
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for decl in candidates.iter().filter(|f| f.params.len() == arity) {
            for (index, p) in decl.params.iter().enumerate() {
                let TypeRef::Named { name: pname, .. } = &p.ty else {
                    continue;
                };
                if let Some(key) = by_name.get(pname.as_str()) {
                    // An overload set whose declarations disagree about which
                    // parameter is an arrow cannot pick one here, and guessing
                    // is what the dispatch machinery exists to avoid. Leave it
                    // alone and let the checker report the unresolved name.
                    if seen.insert(index) {
                        if let Some(slot) = out.get_mut(index) {
                            *slot = Some((*key).clone());
                        }
                    }
                }
            }
        }
        out
    }
}

fn is_arrow(t: &TypeRef) -> bool {
    match t {
        TypeRef::Arrow { .. } => true,
        TypeRef::Named { args, .. } => args.iter().any(is_arrow),
        TypeRef::Tuple { elems, .. } => elems.iter().any(is_arrow),
        TypeRef::Unit { .. } => false,
    }
}

fn member_uses_arrow(m: &Member) -> bool {
    match m {
        Member::Field(f) => is_arrow(&f.ty),
        Member::Method(m) => {
            m.params.iter().any(|p| is_arrow(&p.ty)) || m.return_type.as_ref().is_some_and(is_arrow)
        }
    }
}

/// What a name in a body is bound to. Only the arrow case carries anything: a
/// name bound to anything else exists here purely so that it SHADOWS a
/// top-level function of the same name, which is the rule that keeps
/// `juxtshadow.fss`'s premise true for this rewrite too.
enum Slot {
    /// The name of the minted trait this name is typed with.
    Arrow(String),
    Opaque,
}

#[derive(Default)]
struct Scope {
    frames: Vec<BTreeMap<String, Slot>>,
}

impl Scope {
    fn push(&mut self) {
        self.frames.push(BTreeMap::new());
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    fn declare(&mut self, name: &str, ty: &TypeRef) {
        let slot = match ty {
            TypeRef::Named { name: tname, .. } if tname.starts_with(TRAIT_PREFIX) => {
                Slot::Arrow(tname.clone())
            }
            _ => Slot::Opaque,
        };
        if self.frames.is_empty() {
            self.frames.push(BTreeMap::new());
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.insert(name.to_owned(), slot);
        }
    }

    fn declare_opaque(&mut self, name: &str) {
        if self.frames.is_empty() {
            self.frames.push(BTreeMap::new());
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.insert(name.to_owned(), Slot::Opaque);
        }
    }

    fn get(&self, name: &str) -> Option<&Slot> {
        self.frames.iter().rev().find_map(|f| f.get(name))
    }
}

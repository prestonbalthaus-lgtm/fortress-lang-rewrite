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
//!   * STALE UNTIL 2026-08-22, AND CORRECTED HERE. This list said "`fn`
//!     syntax, so no anonymous closure and no captured environment". BOTH
//!     LAND NOW and the comment predated the feature -- measured with the
//!     compiler, one construct at a time, while pricing the Generator
//!     protocol: an anonymous `fn` with no capture runs, and one WITH a
//!     capture runs and gets the capture right (`apply2(fn(x:ZZ32):ZZ32 => x k,
//!     4)` with `k = 3` prints 12, not 8). Reading this paragraph is what made
//!     that milestone look like it needed a BUILD. See
//!     docs/superpowers/specs/2026-08-22-generator-protocol-measured.md.
//!     What remains true is the SHAPE: captures are constructor parameters, so
//!     a generated object with none is still `Some(vec![])` and not `None` and
//!     is constructed at each use rather than being a singleton.
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
use crate::types::BUILTIN_TYPE_NAMES;

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
        known: BUILTIN_TYPE_NAMES.iter().map(|s| (*s).to_owned()).collect(),
        lambdas: 0,
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
    // Unconditional. A precheck for "does this component write an arrow" was
    // wrong twice over: a `fn` needs lowering with no arrow written anywhere in
    // the file, and a component that needs nothing is walked and returned
    // unchanged for a cost the sweep cannot measure.
    pass.run(component)
}

/// One arrow signature, keyed by what the source wrote. Two spellings of the
/// same type are the same key because `TypeRef::written` is canonical for the
/// ground types this pass can see.
type ArrowKey = (String, String);

struct Pass {
    /// Every top-level function, by name. A name may have several declarations:
    /// that is an overload set, and a value use of one is ambiguous unless the
    /// arrow the context asks for picks exactly one.
    functions: BTreeMap<String, Vec<FnDecl>>,
    /// Arrow signature to the trait minted for it. Ordered, because tags follow
    /// declaration order and declaration order has to be a fact about the
    /// source rather than about a hash.
    traits: BTreeMap<ArrowKey, ArrowTrait>,
    /// `(what it came from, arrow)` to the object minted for it. For a named
    /// function that is the function's name, so two sites sharing a function
    /// and an arrow share a tag; for a lambda it is the generated name, which
    /// is unique per site, because two identical lambdas at two sites are two
    /// closures.
    objects: BTreeMap<(String, ArrowKey), ObjectDecl>,
    /// Every type name this component declares, plus the builtins.
    known: BTreeSet<String>,
    /// Numbers the objects minted for anonymous functions. A lambda has no name
    /// to key on, and two identical ones at different sites are two closures.
    lambdas: usize,
}

struct ArrowTrait {
    name: String,
    from: TypeRef,
    to: TypeRef,
}

/// Whether a parameter type is the parser's placeholder for one that was not
/// written. Spelled here rather than imported: `types` does not depend on
/// `parser` and must not start.
fn is_infer(t: &TypeRef) -> bool {
    matches!(t, TypeRef::Named { name, .. } if name == "$infer")
}

/// The `apply` parameter list for an arrow with this domain: none at all when
/// the domain is `()`, and one otherwise.
fn apply_params(from: &TypeRef, name: &str, span: Span) -> Vec<Param> {
    if matches!(from, TypeRef::Unit { .. }) {
        return Vec::new();
    }
    vec![Param {
        name: name.to_owned(),
        ty: from.clone(),
        varargs: false,
        span,
    }]
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
                        self.rewrite_type(&mut p.ty)?;
                    }
                    if let Some(t) = &mut f.return_type {
                        self.rewrite_type(t)?;
                    }
                }
                Decl::Trait(t) => {
                    for m in &mut t.members {
                        self.rewrite_member_types(m)?;
                    }
                }
                Decl::Object(o) => {
                    for p in o.params.iter_mut().flatten() {
                        self.rewrite_type(&mut p.ty)?;
                    }
                    for m in &mut o.members {
                        self.rewrite_member_types(m)?;
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
                    let returns = f.return_type.clone();
                    if let Some(body) = &mut f.body {
                        self.rewrite_slotted(body, returns.as_ref(), &mut scope)?;
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
                comprises_open: false,
                excludes: Vec::new(),
                members: vec![Member::Method(MethodDecl {
                    modifiers: Modifiers::default(),
                    name: APPLY.to_owned(),
                    static_params: Vec::new(),
                    params: apply_params(&arrow.from, "x$0", Span::new(0, 0)),
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

    fn rewrite_member_types(&mut self, m: &mut Member) -> Result<(), TypeError> {
        match m {
            Member::Field(f) => self.rewrite_type(&mut f.ty)?,
            // A GENERIC METHOD IS NOT REWRITTEN, for the same reason
            // monomorphization files it instead of expanding it: its static
            // parameters are names, not types, so `body: E->R` inside
            // `generate[\R\]` on `trait Generator[\E\]` reports `unknown type
            // E` against a type that was never meant to exist. The stamp is
            // what gets walked, once a call site has said at what arguments,
            // and a genuinely unknown name is caught there.
            //
            // NARROWED TO GENERIC METHODS ON PURPOSE. Loosening it for a
            // non-generic member would reopen the hole `rewrite_type` closed:
            // an unliftable arrow in an abstract member is `TypeNotImplemented`,
            // which `excusable` skips, so `m(g: Foo -> ZZ32): ZZ32` compiled to
            // exit 0 in silence.
            Member::Method(m) if !m.static_params.is_empty() => {}
            Member::Method(m) => {
                for p in &mut m.params {
                    self.rewrite_type(&mut p.ty)?;
                }
                if let Some(t) = &mut m.return_type {
                    self.rewrite_type(t)?;
                }
            }
        }
        Ok(())
    }

    fn rewrite_member_body(&mut self, m: &mut Member) -> Result<(), TypeError> {
        let Member::Method(method) = m else {
            return Ok(());
        };
        let mut scope = Scope::default();
        for p in &method.params {
            scope.declare(&p.name, &p.ty);
        }
        let returns = method.return_type.clone();
        if let Some(body) = &mut method.body {
            self.rewrite_slotted(body, returns.as_ref(), &mut scope)?;
        }
        Ok(())
    }

    /// Innermost first, so `(A -> B) -> C` mints the trait for `A -> B` before
    /// the one that names it and the composition falls out with no special
    /// case.
    fn rewrite_type(&mut self, t: &mut TypeRef) -> Result<(), TypeError> {
        match t {
            // The ELEMENT of a shape may be an arrow -- `(A -> B)[5]` -- and it
            // is lifted like any other. The extents are static arguments and
            // hold no arrow.
            TypeRef::Shaped { base, .. } => {
                self.rewrite_type(base)?;
                return Ok(());
            }
            TypeRef::Arrow { from, to, span } => {
                self.rewrite_type(from)?;
                self.rewrite_type(to)?;
                // A NAME THAT DOES NOT EXIST IS REPORTED HERE, because leaving
                // the arrow unlifted hands it to a diagnostic that names the
                // wrong thing -- and inside an ABSTRACT member, to no
                // diagnostic at all: an unliftable arrow is `TypeNotImplemented`,
                // which `excusable` skips, so `m(g: Foo -> ZZ32): ZZ32` in a
                // trait compiled to exit 0 in silence.
                if let Some(unknown) = self.undeclared_in(from).or_else(|| self.undeclared_in(to)) {
                    return Err(TypeError::UnknownType {
                        span: *span,
                        name: unknown,
                    });
                }
                // NOT EVERY ARROW IS LIFTABLE, and one that is not must be left
                // exactly as it was so that `Registry::resolve` gives it the
                // refusal it already had. Minting a trait whose `apply` takes a
                // tuple turned `(ZZ32, ZZ32) -> ZZ32` from a diagnostic on
                // master into a silent exit 0 -- the worst class this project
                // recognises, introduced by the pass meant to add a feature.
                if !self.liftable_domain(from) || !self.liftable(to) {
                    return Ok(());
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
                    self.rewrite_type(a)?;
                }
            }
            TypeRef::Tuple { elems, .. } => {
                for e in elems {
                    self.rewrite_type(e)?;
                }
            }
            // Nothing to rewrite: a static value contains no type at all.
            TypeRef::Unit { .. } | TypeRef::Static { .. } => {}
        }
        Ok(())
    }

    /// The first name inside a type that this component does not declare. Only
    /// asked about the sides of an ARROW: everywhere else the checker resolves
    /// the type itself and reports it, and asking here as well would report the
    /// same thing twice with a worse span.
    fn undeclared_in(&self, t: &TypeRef) -> Option<String> {
        match t {
            TypeRef::Named { name, args, .. } => {
                if !self.known.contains(name) && !name.starts_with(TRAIT_PREFIX) {
                    return Some(name.clone());
                }
                args.iter().find_map(|a| self.undeclared_in(a))
            }
            TypeRef::Arrow { from, to, .. } => {
                self.undeclared_in(from).or_else(|| self.undeclared_in(to))
            }
            TypeRef::Tuple { elems, .. } => elems.iter().find_map(|e| self.undeclared_in(e)),
            TypeRef::Shaped { base, .. } => self.undeclared_in(base),
            // A static VALUE names no type, so there is no type name in it to
            // be undeclared. Whether the names INSIDE it resolve to value
            // parameters is expansion's question and it answers it by name.
            TypeRef::Static { .. } => None,
            TypeRef::Unit { .. } => None,
        }
    }

    /// An expression whose type is WRITTEN somewhere -- a declaration's return
    /// type, a binding's annotation -- and which may be a lambda that needs it.
    ///
    /// `make(k: ZZ32): () -> ZZ32 = fn () => k + 1` has no argument slot to
    /// take the arrow from, and refusing it would refuse the shape the corpus
    /// writes for a closure factory. Only the expression ITSELF is treated this
    /// way: a lambda deeper inside the body has no claim on the declaration's
    /// return type, and giving it one would type it by coincidence.
    fn rewrite_slotted(
        &mut self,
        e: &mut Expr,
        written: Option<&TypeRef>,
        scope: &mut Scope,
    ) -> Result<(), TypeError> {
        if matches!(e, Expr::Lambda { .. }) {
            let key = written.and_then(|t| match t {
                TypeRef::Named { name, .. } if name.starts_with(TRAIT_PREFIX) => self
                    .traits
                    .iter()
                    .find(|(_, a)| a.name == *name)
                    .map(|(k, _)| k.clone()),
                _ => None,
            });
            return self.lambda(e, key.as_ref(), scope);
        }
        self.rewrite_expr(e, scope)
    }

    /// `fn (x: T): R => e`, lowered to a generated object whose CONSTRUCTOR
    /// PARAMETERS are the names the body captures.
    ///
    /// That choice is what makes the body need no rewriting at all: a dotted
    /// method reads its receiver's fields by their own spelling, so `k` inside
    /// `apply` resolves to the field `k` exactly as it resolved to the
    /// enclosing local before. No environment struct, no fat pointer, and
    /// nothing in codegen that did not already exist.
    ///
    /// FOUR THINGS ARE REFUSED BY NAME rather than guessed at, and each is a
    /// boundary rather than an oversight:
    ///   * more than one parameter -- the arrow would be `(A, B) -> C`, a tuple
    ///     domain, which is unliftable until composite types are real;
    ///   * no return type and no arrow to take one from;
    ///   * a captured name with no written type, because a constructor
    ///     parameter needs one and inventing it is how a wrong type gets in;
    ///   * capturing `self`, because the generated object's own `apply` binds
    ///     `self` to the closure and the capture would be silently shadowed.
    fn lambda(
        &mut self,
        e: &mut Expr,
        wanted: Option<&ArrowKey>,
        scope: &mut Scope,
    ) -> Result<(), TypeError> {
        let Expr::Lambda {
            params,
            return_type,
            body,
            span,
        } = e
        else {
            return Ok(());
        };
        let span = *span;
        let mut param = match params.as_slice() {
            [] => None,
            [one] => Some(one.clone()),
            _ => {
                return Err(TypeError::LambdaUnsupported {
                    span,
                    form: "a lambda with more than one parameter",
                })
            }
        };
        if let Some(p) = param.as_mut() {
            self.rewrite_type(&mut p.ty)?;
        }

        // What the slot asks for, when it asks for anything. It is the ONLY
        // source for an unwritten parameter type and one of two for the return
        // type. There is no inference beyond it.
        let slot = wanted
            .and_then(|key| self.traits.get(key))
            .map(|a| (a.from.clone(), a.to.clone()));

        let written_from = match param.as_ref() {
            // No parameter list at all: the domain is `()` and `apply` takes
            // nothing. 169 of the corpus's 1064 `fn` uses are this shape.
            None => Some(TypeRef::Unit { span }),
            Some(p) if !is_infer(&p.ty) => Some(p.ty.clone()),
            Some(_) => None,
        };
        let from = match (written_from, slot.as_ref()) {
            (Some(t), _) => t,
            (None, Some((f, _))) => f.clone(),
            (None, None) => {
                return Err(TypeError::LambdaUnsupported {
                    span,
                    form: "a lambda whose parameter has no written type, in a position that \
                           does not supply one",
                })
            }
        };
        let to = match (return_type.as_ref(), slot.as_ref()) {
            (Some(r), _) => {
                let mut r = r.clone();
                self.rewrite_type(&mut r)?;
                r
            }
            (None, Some((_, t))) => t.clone(),
            (None, None) => {
                return Err(TypeError::LambdaUnsupported {
                    span,
                    form: "a lambda with no return type, in a position that does not supply one",
                })
            }
        };
        if let Some(p) = param.as_mut() {
            p.ty = from.clone();
        }
        if !self.liftable_domain(&from) || !self.liftable(&to) {
            return Err(TypeError::LambdaUnsupported {
                span,
                form: "a lambda over a type this subset cannot store",
            });
        }
        let mut arrow_ref = TypeRef::Arrow {
            from: Box::new(from.clone()),
            to: Box::new(to.clone()),
            span,
        };
        self.rewrite_type(&mut arrow_ref)?;
        let TypeRef::Named {
            name: trait_name, ..
        } = &arrow_ref
        else {
            return Err(TypeError::LambdaUnsupported {
                span,
                form: "a lambda over a type this subset cannot store",
            });
        };
        let trait_name = trait_name.clone();
        let key = arrow_key(&from, &to);

        // The body is lowered FIRST, in the lambda's own scope, so a nested
        // lambda has already become a construction by the time its captures are
        // counted as free names of this one.
        let mut lowered = (**body).clone();
        scope.push();
        if let Some(p) = param.as_ref() {
            scope.declare(&p.name, &p.ty);
        }
        let walked = self.rewrite_expr(&mut lowered, scope);
        scope.pop();
        walked?;

        let mut free: BTreeSet<String> = BTreeSet::new();
        let bound: BTreeSet<String> = param.iter().map(|p| p.name.clone()).collect();
        free_names(&lowered, &mut vec![bound], &mut free);
        let mut captures: Vec<Param> = Vec::new();
        for name in free {
            if name == "self" {
                return Err(TypeError::LambdaCaptureUntyped { span, name });
            }
            let Some(slot) = scope.get(&name) else {
                // Not a local: a top-level function, an object, or a builtin.
                // Those are reachable from inside `apply` unchanged.
                continue;
            };
            let Some(ty) = slot.ty.clone() else {
                return Err(TypeError::LambdaCaptureUntyped { span, name });
            };
            captures.push(Param {
                name,
                ty,
                varargs: false,
                span,
            });
        }

        let index = self.lambdas;
        self.lambdas = self.lambdas.saturating_add(1);
        let object_name = format!("fn${index}${trait_name}");
        let object = ObjectDecl {
            modifiers: Modifiers::default(),
            name: object_name.clone(),
            static_params: Vec::new(),
            params: Some(captures.clone()),
            extends: vec![TypeRef::Named {
                name: trait_name,
                args: Vec::new(),
                span,
            }],
            comprises: Vec::new(),
            comprises_open: false,
            excludes: Vec::new(),
            members: vec![Member::Method(MethodDecl {
                modifiers: Modifiers::default(),
                name: APPLY.to_owned(),
                static_params: Vec::new(),
                params: param
                    .as_ref()
                    .map(|p| {
                        vec![Param {
                            name: p.name.clone(),
                            ty: from.clone(),
                            varargs: false,
                            span,
                        }]
                    })
                    .unwrap_or_default(),
                return_type: Some(to),
                body: Some(lowered),
                accessor: false,
                span,
            })],
            span,
        };
        self.objects.insert((object_name.clone(), key), object);

        *e = Expr::Call {
            callee: Box::new(Expr::Var {
                name: object_name,
                span,
            }),
            args: captures
                .into_iter()
                .map(|c| Expr::Var { name: c.name, span })
                .collect(),
            span,
        };
        Ok(())
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
        let nullary = matches!(from, TypeRef::Unit { .. });
        let matching: Vec<&FnDecl> = candidates
            .iter()
            .filter(|f| {
                let domain = if nullary {
                    f.params.is_empty()
                } else {
                    f.params.len() == 1
                        && f.params
                            .first()
                            .is_some_and(|p| p.ty.written() == from.written())
                };
                domain
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
            args: if nullary {
                Vec::new()
            } else {
                vec![Expr::Var {
                    name: "x$0".to_owned(),
                    span,
                }]
            },
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
            comprises_open: false,
            excludes: Vec::new(),
            members: vec![Member::Method(MethodDecl {
                modifiers: Modifiers::default(),
                name: APPLY.to_owned(),
                static_params: Vec::new(),
                // No `self` parameter: a `self` parameter marks a FUNCTIONAL
                // method, which lifts into the top-level overload set and is a
                // different namespace entirely. `apply` has to be dotted.
                params: apply_params(&from, "x$0", span),
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
            | Expr::CharLit { .. }
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
                    // A lambda in an argument slot takes its return type from
                    // the arrow the slot declares, which is the only place an
                    // unannotated one can come from.
                    if matches!(arg, Expr::Lambda { .. }) {
                        self.lambda(arg, slot, scope)?;
                        continue;
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
            Expr::Index { base, indices, .. } => {
                self.rewrite_expr(base, scope)?;
                for index in indices {
                    self.rewrite_expr(index, scope)?;
                }
                Ok(())
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
            // A spawned body is ordinary code: a named function passed as an
            // argument inside one is rewritten exactly as it would be outside.
            Expr::Spawn { body, .. } => self.rewrite_expr(body, scope),
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
            Expr::AlsoDo { blocks, .. } => self.rewrite_exprs(blocks, scope),
            Expr::ForIn {
                binder,
                source,
                body,
                ..
            } => {
                self.rewrite_expr(source, scope)?;
                scope.push();
                scope.declare_opaque(binder);
                let result = self.rewrite_expr(body, scope);
                scope.pop();
                result
            }
            // A lambda in a position that does not say what arrow it is. The
            // written return type is the only other source, and `lambda` is
            // where that is decided.
            Expr::Lambda { .. } => self.lambda(e, None, scope),
            Expr::BigReduction {
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
                        self.rewrite_type(t)?;
                    }
                    let written = ty.clone();
                    self.rewrite_slotted(value, written.as_ref(), scope)?;
                    match ty {
                        Some(t) => scope.declare(name, t),
                        None => scope.declare_opaque(name),
                    }
                }
                // Every name a tuple binder introduces is opaque here: its type
                // comes from the initializer and this pass only needs to know
                // the name is BOUND, so a later use resolves to the local
                // rather than to a top-level declaration.
                BlockItem::TupleBinding(b) => {
                    self.rewrite_expr(&mut b.value, scope)?;
                    for name in &b.names {
                        scope.declare_opaque(name);
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

    /// The domain of an arrow may be `()`, where the rest of a type position
    /// may not: a nullary closure's `apply` takes no parameter at all, so
    /// there is nothing unstorable about it. 169 of the corpus's 1064 `fn` uses
    /// are `fn () => e`. The CODOMAIN is still an ordinary type -- `apply` has
    /// to return something.
    fn liftable_domain(&self, t: &TypeRef) -> bool {
        matches!(t, TypeRef::Unit { .. }) || self.liftable(t)
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
            TypeRef::Arrow { from, to, .. } => self.liftable_domain(from) && self.liftable(to),
            // A static value is not a type and there is nothing to lift. A
            // shaped type is not an arrow either -- and it must answer false
            // rather than recursing into its element, or `(A -> B)[5]` would
            // be lifted to the minted trait and lose its shape.
            TypeRef::Tuple { .. }
            | TypeRef::Unit { .. }
            | TypeRef::Static { .. }
            | TypeRef::Shaped { .. } => false,
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
            Expr::Var { name, .. } => scope
                .get(name)
                .and_then(|s| s.arrow().map(ToOwned::to_owned)),
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

/// Every name an expression READS that is not bound inside it. `bound` is the
/// stack of names the lambda itself introduces; anything else that is a `Var`
/// is a free name, and the caller decides which of those are captures and which
/// are top-level declarations.
///
/// Conservative on purpose: a name that is only a callee (`f(x)`) is collected
/// too, and the caller drops it when the scope does not hold it.
fn free_names(e: &Expr, bound: &mut Vec<BTreeSet<String>>, out: &mut BTreeSet<String>) {
    let is_bound = |bound: &Vec<BTreeSet<String>>, n: &str| bound.iter().any(|f| f.contains(n));
    match e {
        Expr::Var { name, .. } => {
            if !is_bound(bound, name) {
                out.insert(name.clone());
            }
        }
        Expr::Unit { .. }
        | Expr::IntLit { .. }
        | Expr::FloatLit { .. }
        | Expr::StrLit { .. }
        | Expr::CharLit { .. }
        | Expr::BoolLit { .. } => {}
        Expr::Tuple { items, .. } | Expr::Juxt { items, .. } | Expr::ArrayLit { items, .. } => {
            for i in items {
                free_names(i, bound, out);
            }
        }
        Expr::Infix { lhs, rhs, .. } => {
            free_names(lhs, bound, out);
            free_names(rhs, bound, out);
        }
        Expr::Prefix { operand, .. } => free_names(operand, bound, out),
        Expr::Call { callee, args, .. } => {
            free_names(callee, bound, out);
            for a in args {
                free_names(a, bound, out);
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            free_names(cond, bound, out);
            free_names(then_branch, bound, out);
            if let Some(e) = else_branch {
                free_names(e, bound, out);
            }
        }
        Expr::Block { items, .. } => {
            bound.push(BTreeSet::new());
            for item in items {
                match item {
                    BlockItem::Binding(b) => {
                        free_names(&b.value, bound, out);
                        if let Some(frame) = bound.last_mut() {
                            frame.insert(b.name.clone());
                        }
                    }
                    BlockItem::TupleBinding(b) => {
                        free_names(&b.value, bound, out);
                        if let Some(frame) = bound.last_mut() {
                            for name in &b.names {
                                frame.insert(name.clone());
                            }
                        }
                    }
                    BlockItem::Assign(a) => {
                        free_names(&a.target, bound, out);
                        free_names(&a.value, bound, out);
                    }
                    BlockItem::Expr(e) => free_names(e, bound, out),
                }
            }
            bound.pop();
        }
        Expr::Index { base, indices, .. } => {
            free_names(base, bound, out);
            for index in indices {
                free_names(index, bound, out);
            }
        }
        Expr::While { cond, body, .. } => {
            free_names(cond, bound, out);
            free_names(body, bound, out);
        }
        Expr::Field { base, .. } => free_names(base, bound, out),
        Expr::For {
            binder,
            lo,
            hi,
            body,
            ..
        } => {
            free_names(lo, bound, out);
            free_names(hi, bound, out);
            bound.push([binder.clone()].into_iter().collect());
            free_names(body, bound, out);
            bound.pop();
        }
        Expr::Instantiate { callee, .. } => free_names(callee, bound, out),
        Expr::Atomic { body, .. } => free_names(body, bound, out),
        // EVERY FREE NAME OF A SPAWNED BODY IS A CAPTURE, which is the whole
        // reason this arm is not a no-op: the body becomes an outlined
        // function and anything it reads from the enclosing scope has to
        // travel in the environment.
        Expr::Spawn { body, .. } => free_names(body, bound, out),
        Expr::Case {
            subject,
            arms,
            else_arm,
            ..
        } => {
            free_names(subject, bound, out);
            for a in arms {
                free_names(&a.guard, bound, out);
                free_names(&a.body, bound, out);
            }
            if let Some(e) = else_arm {
                free_names(e, bound, out);
            }
        }
        Expr::TypeCase {
            subject,
            arms,
            else_arm,
            ..
        } => {
            free_names(subject, bound, out);
            for a in arms {
                bound.push(a.binder.iter().cloned().collect());
                free_names(&a.body, bound, out);
                bound.pop();
            }
            free_names(else_arm, bound, out);
        }
        Expr::Label { body, .. } => free_names(body, bound, out),
        Expr::AlsoDo { blocks, .. } => {
            for b in blocks {
                free_names(b, bound, out);
            }
        }
        Expr::ForIn {
            binder,
            source,
            body,
            ..
        } => {
            free_names(source, bound, out);
            bound.push([binder.clone()].into_iter().collect());
            free_names(body, bound, out);
            bound.pop();
        }
        Expr::BigReduction {
            binder,
            lo,
            hi,
            body,
            ..
        } => {
            free_names(lo, bound, out);
            free_names(hi, bound, out);
            bound.push([binder.clone()].into_iter().collect());
            free_names(body, bound, out);
            bound.pop();
        }
        Expr::Exit { value, .. } => {
            if let Some(e) = value {
                free_names(e, bound, out);
            }
        }
        // A nested lambda has already been lowered into a construction by the
        // time this runs, so this arm is only reached if one was left; treat
        // its parameters as bound and its body as read.
        Expr::Lambda { params, body, .. } => {
            bound.push(params.iter().map(|p| p.name.clone()).collect());
            free_names(body, bound, out);
            bound.pop();
        }
    }
}

/// What a name in a body is bound to.
///
/// A name is here for two reasons and both matter. It SHADOWS a top-level
/// function of the same name -- the rule that keeps `juxtshadow.fss`'s premise
/// true for this rewrite. And, when a lambda closes over it, its WRITTEN TYPE
/// is what the generated object's constructor parameter is declared with; a
/// name with no written type cannot be captured at all, and is refused by name
/// rather than guessed at.
struct Slot {
    ty: Option<TypeRef>,
}

impl Slot {
    fn arrow(&self) -> Option<&str> {
        match &self.ty {
            Some(TypeRef::Named { name, .. }) if name.starts_with(TRAIT_PREFIX) => Some(name),
            _ => None,
        }
    }
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
        self.insert(
            name,
            Slot {
                ty: Some(ty.clone()),
            },
        );
    }

    /// A name that is bound and whose type is not written: an unannotated
    /// binding, a loop binder, a typecase binder with no type of its own.
    fn declare_opaque(&mut self, name: &str) {
        self.insert(name, Slot { ty: None });
    }

    fn insert(&mut self, name: &str, slot: Slot) {
        if self.frames.is_empty() {
            self.frames.push(BTreeMap::new());
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.insert(name.to_owned(), slot);
        }
    }

    fn get(&self, name: &str) -> Option<&Slot> {
        self.frames.iter().rev().find_map(|f| f.get(name))
    }
}

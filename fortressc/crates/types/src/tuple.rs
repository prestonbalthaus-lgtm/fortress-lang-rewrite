//! Arity flattening, and the non-materialising calling convention with it.
//!
//! `overloading.tex:125` -- "Recall that a functional has a single parameter,
//! which may be a tuple". So `f(x: (A,B))` and `f(a: A, b: B)` ARE ONE
//! DECLARATION, and the honest way to have the first is to lower it into the
//! second. That is the whole pass: a tuple-typed name becomes SEVERAL names,
//! `x$0` and `x$1`, and a tuple is never built, stored, returned or passed.
//! There is nothing to box because nothing is ever whole.
//!
//! ```text
//! f(x: (A,B)): R = do (a,b) = x; ... end       f(x$0: A, x$1: B): R = do
//! t = (p, q)                                     a = x$0
//! f(t)                                           b = x$1 ... end
//! f((p, q))                                    t$0 = p
//!                                              t$1 = q
//!                                              f(t$0, t$1)  and  f(p, q)
//! ```
//!
//! IT RUNS BEFORE EXPANSION, like `comprehension`, and for the same reason: it
//! changes ARITIES, and every signature the registry and the dispatch tables are
//! built from has to be the flattened one.
//!
//! WHAT IS REFUSED BY NAME, each because it needs the half of the convention
//! this milestone does not build:
//!
//! * A TUPLE RESULT. `tuple_free`'s existing refusal, untouched: returning one
//!   needs the CALLEE to hand back several values, which is an LLVM aggregate
//!   return and a milestone of its own -- the argument direction is what a
//!   calling convention is.
//! * A MUTABLE tuple local. `t := (a,b)` would have to split into two stores
//!   and `t := f()` could not split at all.
//! * A tuple local whose initialiser is neither a written tuple nor another
//!   flattened name -- the same wall, seen from the binding side.
//! * A NESTED tuple type. Measured at zero corpus files, and flattening it
//!   recursively would make an arity depend on a type's shape two levels down.
//! * A flattened name used as anything but a whole argument or the right-hand
//!   side of a destructuring. `t.something()`, `println(t)` and
//!   `typecase (x,y)` all want a value, and there is none.

use std::collections::BTreeMap;

use fortress_ast::{Assign, Binding, BlockItem, Component, Decl, Expr, Member, Param, TypeRef};

use crate::error::TypeError;

pub fn lower(component: &Component) -> Result<Component, TypeError> {
    let mut out = component.clone();
    let mut pass = Pass {
        scopes: Vec::new(),
        arities: BTreeMap::new(),
    };
    // THE COMPONENT-LEVEL VALUES FIRST, AND IN A FRAME OF THEIR OWN. A tuple
    // written at component level is split into one value per part, and every
    // body below has to know that before it is walked -- `TupleCastGeneric`
    // declares `tu:(O,O)` at the top and passes it from inside `run`.
    pass.push();
    let mut split: Vec<Decl> = Vec::with_capacity(out.decls.len());
    for decl in out.decls.drain(..) {
        match decl {
            Decl::Value(v) => split.extend(pass.value(v)?),
            other => split.push(other),
        }
    }
    out.decls = split;
    // SIGNATURES FIRST, BODIES SECOND. Splicing a whole tuple into a call is
    // only right where the callee HAS a declaration of that arity, and the
    // arities are not known until every parameter list has been flattened.
    for decl in &mut out.decls {
        pass.signature(decl)?;
    }
    for decl in &mut out.decls {
        pass.body(decl)?;
    }
    pass.pop();
    Ok(out)
}

/// One part of a flattened name: the name it now goes by. The TYPE is not
/// carried, and that is the point -- every part is an ordinary binding or an
/// ordinary parameter by the time this pass is done, and the checker types it
/// the way it types any other.
#[derive(Clone)]
struct Part {
    name: String,
}

struct Pass {
    scopes: Vec<BTreeMap<String, Vec<Part>>>,
    /// Every arity each top-level name is declared at, AFTER flattening. A
    /// tuple argument is spliced only where it makes a call reach one of them;
    /// otherwise the tuple is written back out and the checker refuses it for
    /// what it is, rather than for an arity nobody wrote.
    arities: BTreeMap<String, Vec<usize>>,
}

impl Pass {
    fn push(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn record(&mut self, name: &str, parts: Vec<Part>) {
        if let Some(frame) = self.scopes.last_mut() {
            frame.insert(name.to_owned(), parts);
        }
    }

    /// A name shadowed by an ordinary binding stops being flattened, which is
    /// why this walks the frames from the top and a plain binding CLEARS.
    fn parts(&self, name: &str) -> Option<&Vec<Part>> {
        self.scopes.iter().rev().find_map(|f| f.get(name))
    }

    fn clear(&mut self, name: &str) {
        if let Some(frame) = self.scopes.last_mut() {
            frame.remove(name);
        }
    }

    /// Flatten every parameter list and record what each name is declared at.
    fn signature(&mut self, decl: &mut Decl) -> Result<(), TypeError> {
        match decl {
            Decl::Function(f) => {
                self.flatten_params(&mut f.params)?;
                self.declared(&f.name, f.params.len());
            }
            Decl::Value(_) => {}
            Decl::Trait(t) => {
                for m in &mut t.members {
                    self.member_signature(m)?;
                }
            }
            Decl::Object(o) => {
                if let Some(params) = &o.params {
                    // AN OBJECT'S VALUE PARAMETERS ARE ITS FIELDS, so flattening
                    // one would silently change a layout and a constructor
                    // arity that dispatch has already been told about.
                    //
                    // A MERGED ONE IS LEFT ALONE. An api's object is not
                    // lowered here and has no constructor, which the checker
                    // already says in those words -- refusing it now would take
                    // `UnstorableApi`'s `Pair` away from the file that exists
                    // to prove exactly that.
                    for p in params {
                        if matches!(p.ty, TypeRef::Tuple { .. }) && !o.merged {
                            return Err(TypeError::TupleFieldNotFlattened { span: p.span });
                        }
                    }
                    self.declared(&o.name, params.len());
                }
                for m in &mut o.members {
                    self.member_signature(m)?;
                }
            }
        }
        Ok(())
    }

    fn member_signature(&mut self, member: &mut Member) -> Result<(), TypeError> {
        match member {
            Member::Method(m) => {
                self.flatten_params(&mut m.params)?;
                self.declared(&m.name, m.params.len());
            }
            Member::Field(f) => {
                if matches!(f.ty, TypeRef::Tuple { .. }) {
                    return Err(TypeError::TupleFieldNotFlattened { span: f.span });
                }
            }
            Member::Coercion { .. } => {}
        }
        Ok(())
    }

    fn declared(&mut self, name: &str, arity: usize) {
        let seen = self.arities.entry(name.to_owned()).or_default();
        if !seen.contains(&arity) {
            seen.push(arity);
        }
    }

    fn body(&mut self, decl: &mut Decl) -> Result<(), TypeError> {
        match decl {
            Decl::Function(f) => {
                self.push();
                self.bind_parts(&f.params);
                let walked = match &mut f.body {
                    Some(body) => self.expr(body),
                    None => Ok(()),
                };
                self.pop();
                walked?;
            }
            // Already split and recorded, and its initializer walked with it.
            Decl::Value(_) => {}
            Decl::Trait(t) => {
                for m in &mut t.members {
                    self.member(m)?;
                }
            }
            Decl::Object(o) => {
                for m in &mut o.members {
                    self.member(m)?;
                }
            }
        }
        Ok(())
    }

    fn member(&mut self, member: &mut Member) -> Result<(), TypeError> {
        match member {
            Member::Method(m) => {
                self.push();
                self.bind_parts(&m.params);
                let walked = match &mut m.body {
                    Some(body) => self.expr(body),
                    None => Ok(()),
                };
                self.pop();
                walked?;
            }
            Member::Field(f) => {
                if let Some(init) = &mut f.init {
                    self.push();
                    let walked = self.expr(init);
                    self.pop();
                    walked?;
                }
            }
            Member::Coercion { .. } => {}
        }
        Ok(())
    }

    /// Re-derive which names an already-flattened parameter list stands for.
    /// `x$0` beside `x$1` IS `x`, and the flattening is the only thing that
    /// mints that spelling -- `$` is unwritable in a source name.
    fn bind_parts(&mut self, params: &[Param]) {
        let mut groups: BTreeMap<String, Vec<Part>> = BTreeMap::new();
        for p in params {
            let Some((base, index)) = p.name.rsplit_once('$') else {
                continue;
            };
            if index.parse::<usize>().is_err() {
                continue;
            }
            groups.entry(base.to_owned()).or_default().push(Part {
                name: p.name.clone(),
            });
        }
        for (name, parts) in groups {
            self.record(&name, parts);
        }
    }

    /// Replace every tuple-typed parameter with one parameter per element, in
    /// place, and report what was replaced.
    fn flatten_params(&mut self, params: &mut Vec<Param>) -> Result<(), TypeError> {
        if !params.iter().any(|p| matches!(p.ty, TypeRef::Tuple { .. })) {
            return Ok(());
        }
        let mut flat: Vec<Param> = Vec::with_capacity(params.len());
        for p in params.iter() {
            let TypeRef::Tuple { elems, span } = &p.ty else {
                flat.push(p.clone());
                continue;
            };
            for (index, elem) in elems.iter().enumerate() {
                if matches!(elem, TypeRef::Tuple { .. }) {
                    return Err(TypeError::TupleNested { span: *span });
                }
                flat.push(Param {
                    name: part_name(&p.name, index),
                    ty: elem.clone(),
                    varargs: false,
                    mutable: false,
                    span: p.span,
                });
            }
        }
        *params = flat;
        Ok(())
    }

    fn expr(&mut self, e: &mut Expr) -> Result<(), TypeError> {
        match e {
            Expr::Var { name, span } => {
                if self.parts(name).is_some() {
                    return Err(TypeError::TupleNameNotWhole {
                        span: *span,
                        name: name.clone(),
                    });
                }
                Ok(())
            }
            Expr::Call { callee, args, span } => {
                self.expr(callee)?;
                let _ = span;
                let mut spliced: Vec<Expr> = Vec::with_capacity(args.len());
                let mut whole: Vec<Expr> = Vec::with_capacity(args.len());
                let mut spread = false;
                for arg in args.iter_mut() {
                    match self.argument(arg)? {
                        Some(parts) => {
                            spread = true;
                            whole.push(Expr::Tuple {
                                items: parts.clone(),
                                span: arg.span(),
                            });
                            spliced.extend(parts);
                        }
                        None => {
                            self.expr(arg)?;
                            whole.push(arg.clone());
                            spliced.push(arg.clone());
                        }
                    }
                }
                // SPLICE ONLY WHERE IT REACHES A DECLARATION. `o(x: Any)` beside
                // a four-element tuple wants a tuple VALUE, and spreading it
                // anyway reported `o takes 1 argument(s), found 4` -- an arity
                // nobody wrote. Written back out as a tuple, the checker
                // refuses it for what it is, or names the callee it cannot find.
                *args = if spread && self.reaches(callee, spliced.len()) {
                    spliced
                } else {
                    whole
                };
                Ok(())
            }
            Expr::Block { items, .. } => {
                self.push();
                let walked = self.block(items);
                self.pop();
                walked
            }
            other => self.children(other),
        }
    }

    /// A whole tuple in argument position, spread. `None` when the argument is
    /// an ordinary one.
    fn argument(&mut self, arg: &mut Expr) -> Result<Option<Vec<Expr>>, TypeError> {
        match arg {
            Expr::Var { name, span } => {
                let span = *span;
                let Some(parts) = self.parts(name).cloned() else {
                    return Ok(None);
                };
                Ok(Some(
                    parts
                        .iter()
                        .map(|p| Expr::Var {
                            name: p.name.clone(),
                            span,
                        })
                        .collect(),
                ))
            }
            // `f((a, b))` is `f(a, b)`, which is the same declaration by
            // `overloading.tex:125`. Written out, it never becomes a value.
            Expr::Tuple { items, .. } => {
                for item in items.iter_mut() {
                    self.expr(item)?;
                }
                Ok(Some(items.clone()))
            }
            _ => Ok(None),
        }
    }

    /// Whether the callee has a declaration of this arity. A callee that is not
    /// a plain name -- a dotted method, an arrow-typed local -- is taken on
    /// trust, because its arity is a type fact this pass does not have. A name
    /// with NO declaration in this component is not: `println` is a builtin of
    /// arity one, and trusting it spread a two-element tuple across it.
    fn reaches(&self, callee: &Expr, arity: usize) -> bool {
        let name = match callee {
            Expr::Var { name, .. } => name,
            Expr::Instantiate { callee, .. } => match &**callee {
                Expr::Var { name, .. } => name,
                _ => return true,
            },
            _ => return true,
        };
        let Some(seen) = self.arities.get(name) else {
            return false;
        };
        seen.contains(&arity)
    }

    fn block(&mut self, items: &mut Vec<BlockItem>) -> Result<(), TypeError> {
        let mut out: Vec<BlockItem> = Vec::with_capacity(items.len());
        for item in items.iter_mut() {
            match item {
                BlockItem::Binding(b) => {
                    if let Some(split) = self.split_binding(b)? {
                        out.extend(split);
                        continue;
                    }
                    self.expr(&mut b.value)?;
                    self.clear(&b.name);
                    out.push(item.clone());
                }
                BlockItem::TupleBinding(b) => {
                    // `(a, b) = x` where `x` is flattened: one ordinary binding
                    // per name, which is where the convention pays off -- the
                    // parts are already separate values.
                    if let Expr::Var { name, span } = &b.value {
                        if let Some(parts) = self.parts(name).cloned() {
                            if parts.len() != b.names.len() {
                                return Err(TypeError::TupleArityMismatch {
                                    span: b.span,
                                    names: b.names.len(),
                                    values: parts.len(),
                                });
                            }
                            for (target, part) in b.names.iter().zip(&parts) {
                                out.push(BlockItem::Binding(Binding {
                                    name: target.clone(),
                                    ty: None,
                                    value: Expr::Var {
                                        name: part.name.clone(),
                                        span: *span,
                                    },
                                    mutable: false,
                                    span: b.span,
                                }));
                                self.clear(target);
                            }
                            continue;
                        }
                    }
                    self.expr(&mut b.value)?;
                    for name in &b.names {
                        self.clear(name);
                    }
                    out.push(item.clone());
                }
                BlockItem::Assign(a) => {
                    self.assign(a)?;
                    out.push(item.clone());
                }
                BlockItem::Expr(x) => {
                    self.expr(x)?;
                    out.push(item.clone());
                }
            }
        }
        *items = out;
        Ok(())
    }

    /// `t: (A,B) = (p, q)` and `t = (p, q)`, split into one binding per part.
    /// `None` when the binding is an ordinary one.
    fn split_binding(&mut self, b: &mut Binding) -> Result<Option<Vec<BlockItem>>, TypeError> {
        let written: Option<Vec<TypeRef>> = match &b.ty {
            Some(TypeRef::Tuple { elems, .. }) => Some(elems.clone()),
            _ => None,
        };
        // A written tuple type, a tuple written out, or another flattened name:
        // any of the three has parts to bind, and `s = t` reading like `s: (A,B)
        // = t` is one hole fewer for a reader to find.
        let value_has_parts = match &b.value {
            Expr::Tuple { .. } => true,
            Expr::Var { name, .. } => self.parts(name).is_some(),
            _ => false,
        };
        if written.is_none() && !value_has_parts {
            return Ok(None);
        }
        if b.mutable {
            return Err(TypeError::TupleLocalMutable { span: b.span });
        }
        let mut elements: Vec<Expr> = match &mut b.value {
            Expr::Tuple { items, .. } => {
                for item in items.iter_mut() {
                    self.expr(item)?;
                }
                items.clone()
            }
            Expr::Var { name, span } => {
                let span = *span;
                let Some(parts) = self.parts(name).cloned() else {
                    return Err(TypeError::TupleLocalUnsplittable { span: b.span });
                };
                parts
                    .iter()
                    .map(|p| Expr::Var {
                        name: p.name.clone(),
                        span,
                    })
                    .collect()
            }
            _ => return Err(TypeError::TupleLocalUnsplittable { span: b.span }),
        };
        if let Some(types) = &written {
            if types.len() != elements.len() {
                return Err(TypeError::TupleArityMismatch {
                    span: b.span,
                    names: types.len(),
                    values: elements.len(),
                });
            }
            if types.iter().any(|t| matches!(t, TypeRef::Tuple { .. })) {
                return Err(TypeError::TupleNested { span: b.span });
            }
        }
        let mut parts = Vec::with_capacity(elements.len());
        let mut out = Vec::with_capacity(elements.len());
        for (index, value) in elements.drain(..).enumerate() {
            let name = part_name(&b.name, index);
            let ty = written.as_ref().and_then(|types| types.get(index).cloned());
            out.push(BlockItem::Binding(Binding {
                name: name.clone(),
                ty: ty.clone(),
                value,
                mutable: false,
                span: b.span,
            }));
            parts.push(Part { name });
        }
        self.record(&b.name, parts);
        Ok(Some(out))
    }

    /// A component-level value, split the way a block binding is. `Binding`
    /// and `ValueDecl` are different nodes carrying the same four fields, so
    /// the split is shared by going through one.
    fn value(&mut self, v: fortress_ast::ValueDecl) -> Result<Vec<Decl>, TypeError> {
        let mut binding = Binding {
            name: v.name.clone(),
            ty: v.ty.clone(),
            value: match v.init.clone() {
                Some(e) => e,
                // A declaration with no initializer is an api's obligation and
                // has nothing to split.
                None => return Ok(vec![Decl::Value(v)]),
            },
            mutable: v.mutable,
            span: v.span,
        };
        let Some(parts) = self.split_binding(&mut binding)? else {
            let mut v = v;
            if let Some(init) = &mut v.init {
                self.expr(init)?;
            }
            return Ok(vec![Decl::Value(v)]);
        };
        Ok(parts
            .into_iter()
            .filter_map(|item| match item {
                BlockItem::Binding(b) => Some(Decl::Value(fortress_ast::ValueDecl {
                    modifiers: v.modifiers,
                    name: b.name,
                    ty: b.ty,
                    init: Some(b.value),
                    mutable: b.mutable,
                    span: b.span,
                })),
                _ => None,
            })
            .collect())
    }

    fn assign(&mut self, a: &mut Assign) -> Result<(), TypeError> {
        if let Expr::Var { name, span } = &a.target {
            if self.parts(name).is_some() {
                return Err(TypeError::TupleNameNotWhole {
                    span: *span,
                    name: name.clone(),
                });
            }
        }
        self.expr(&mut a.target)?;
        self.expr(&mut a.value)
    }

    /// EXHAUSTIVE ON PURPOSE, the same as `comprehension`'s: a catch-all would
    /// leave a flattened name unvisited inside the next `Expr` variant, and the
    /// program would compile against `x$0` instead of being refused.
    fn children(&mut self, e: &mut Expr) -> Result<(), TypeError> {
        match e {
            Expr::Unit { .. }
            | Expr::IntLit { .. }
            | Expr::FloatLit { .. }
            | Expr::StrLit { .. }
            | Expr::CharLit { .. }
            | Expr::BoolLit { .. }
            | Expr::Exit { value: None, .. } => {}
            // Handled by `expr`; unreachable here.
            Expr::Var { .. } | Expr::Call { .. } | Expr::Block { .. } => {}
            Expr::Tuple { items, .. } | Expr::Juxt { items, .. } | Expr::ArrayLit { items, .. } => {
                for item in items {
                    self.expr(item)?;
                }
            }
            Expr::Infix { lhs, rhs, .. } => {
                self.expr(lhs)?;
                self.expr(rhs)?;
            }
            Expr::Prefix { operand: inner, .. }
            | Expr::Throw { value: inner, .. }
            | Expr::Field { base: inner, .. }
            | Expr::Atomic { body: inner, .. }
            | Expr::Spawn { body: inner, .. }
            | Expr::Label { body: inner, .. }
            | Expr::Lambda { body: inner, .. }
            | Expr::Instantiate { callee: inner, .. }
            | Expr::Exit {
                value: Some(inner), ..
            } => self.expr(inner)?,
            Expr::Index { base, indices, .. } => {
                self.expr(base)?;
                for index in indices {
                    self.expr(index)?;
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.expr(cond)?;
                self.expr(then_branch)?;
                if let Some(otherwise) = else_branch {
                    self.expr(otherwise)?;
                }
            }
            Expr::While { cond, body, .. } => {
                self.expr(cond)?;
                self.expr(body)?;
            }
            Expr::BindingCondition {
                binders,
                source,
                body,
                otherwise,
                ..
            } => {
                self.expr(source)?;
                self.push();
                for b in binders.iter() {
                    self.clear(b);
                }
                let walked = self.expr(body);
                self.pop();
                walked?;
                if let Some(o) = otherwise {
                    self.expr(o)?;
                }
            }
            Expr::ObjectExpr { members, .. } => {
                for member in members {
                    self.member(member)?;
                }
            }
            Expr::Comprehension { body, gens, .. } => {
                self.expr(body)?;
                for g in gens {
                    self.expr(&mut g.init)?;
                    if let Some(h) = &mut g.hi {
                        self.expr(h)?;
                    }
                }
            }
            Expr::Try {
                body,
                arms,
                finally,
                ..
            } => {
                self.expr(body)?;
                for arm in arms {
                    self.expr(&mut arm.body)?;
                }
                if let Some(f) = finally {
                    self.expr(f)?;
                }
            }
            Expr::For {
                binder,
                lo,
                hi,
                body,
                ..
            } => {
                self.expr(lo)?;
                self.expr(hi)?;
                self.push();
                self.clear(binder);
                let walked = self.expr(body);
                self.pop();
                walked?;
            }
            Expr::BigReduction { lo, hi, body, .. } => {
                self.expr(lo)?;
                self.expr(hi)?;
                self.expr(body)?;
            }
            Expr::ForIn {
                binder,
                source,
                body,
                ..
            }
            | Expr::SeqIterate {
                binder,
                source,
                body,
                ..
            } => {
                self.expr(source)?;
                self.push();
                self.clear(binder);
                let walked = self.expr(body);
                self.pop();
                walked?;
            }
            Expr::AlsoDo { blocks, .. } => {
                for block in blocks {
                    self.expr(block)?;
                }
            }
            Expr::Case {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.expr(subject)?;
                for arm in arms {
                    self.expr(&mut arm.guard)?;
                    self.expr(&mut arm.body)?;
                }
                if let Some(otherwise) = else_arm {
                    self.expr(otherwise)?;
                }
            }
            Expr::TypeCase {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.expr(subject)?;
                for arm in arms {
                    self.expr(&mut arm.body)?;
                }
                self.expr(else_arm)?;
            }
        }
        Ok(())
    }
}

/// `x$0`, `x$1`. Unwritable, so a part can never collide with a source name.
fn part_name(base: &str, index: usize) -> String {
    format!("{base}${index}")
}

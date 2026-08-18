//! Monomorphization: an AST-to-AST expansion that runs *before* the checker.
//!
//! The phase split is the whole point. `dispatch_target` builds a table from
//! `registry.concretes_below` and memoises it during body checking, while
//! `registry.concrete` freezes in `Checker::new`. If instantiation could append
//! a concrete type mid-check, a later instantiation landing under a trait an
//! earlier table already switched on would leave that table with no arm for it,
//! and the missing tag would reach `fortress_dispatch_failed` at run time on a
//! program the checker approved. So expansion closes the world first and hands
//! the checker a component containing no generic declarations at all.
//!
//! Static arguments are written, never inferred. That is what makes instantiation
//! demand a syntactic property of the source, which is what lets this run before
//! anything is typed.

use std::collections::{BTreeMap, VecDeque};

use fortress_ast::{
    Assign, BlockItem, BoundObligation, Component, Decl, Expr, FieldDecl, FnDecl, Member,
    MethodDecl, ObjectDecl, Param, Span, StaticParam, TraitDecl, TypeRef,
};

use crate::error::TypeError;

/// The total ceiling, per component. Depth and type-size limits are both
/// insufficient on their own: an *acyclic* call graph of k+1 declarations, each
/// handing its callee two different wrapper types, produces 2^(k+1)-1 distinct
/// instantiations with every type small. Only a total count catches that.
pub const MAX_INSTANTIATIONS: usize = 4096;

/// Type constructors the language provides rather than the program. These keep
/// their arguments through substitution; everything else with arguments is a
/// user generic and gets mangled away.
const BUILTIN_CONSTRUCTORS: [&str; 1] = ["Array"];

type Subst = BTreeMap<String, TypeRef>;

/// Ordered, and that is load bearing rather than incidental. Instantiations are
/// emitted in this map's order, tags follow declaration order, and switch arms
/// follow tags -- so a hash map here would make the emitted object depend on
/// iteration order instead of on the source text.
type Instances = BTreeMap<String, Instance>;

/// One instantiation still to be produced.
struct Job {
    origin: String,
    args: Vec<TypeRef>,
    mangled: String,
    span: Span,
}

struct Instance {
    origin: String,
    decl: Decl,
}

pub fn expand(component: &Component) -> Result<Component, TypeError> {
    Expander::new(component)?.run(component)
}

struct Expander<'a> {
    generics: BTreeMap<String, &'a Decl>,
    /// Mangled name to the instantiation, ordered so emission is a pure
    /// function of the source rather than of worklist discovery order.
    instances: Instances,
    queue: VecDeque<Job>,
    obligations: Vec<BoundObligation>,
    demand: Vec<Job>,
}

impl<'a> Expander<'a> {
    fn new(component: &'a Component) -> Result<Self, TypeError> {
        let mut generics: BTreeMap<String, &'a Decl> = BTreeMap::new();
        for decl in &component.decls {
            if static_params(decl).is_empty() {
                continue;
            }
            generics.insert(decl_name(decl).to_owned(), decl);
        }
        check_uniformity(component)?;
        Ok(Self {
            generics,
            instances: Instances::new(),
            queue: VecDeque::new(),
            obligations: Vec::new(),
            demand: Vec::new(),
        })
    }

    fn run(mut self, component: &Component) -> Result<Component, TypeError> {
        // Pass one: rewrite the ground declarations. Their static-argument
        // lists are the seed set, and they are syntactic, so no type
        // information is needed to find them.
        let empty = Subst::new();
        let mut ground: Vec<(usize, Decl)> = Vec::new();
        for (index, decl) in component.decls.iter().enumerate() {
            if !static_params(decl).is_empty() {
                continue;
            }
            let rewritten = self.decl(decl, &empty, None)?;
            ground.push((index, rewritten));
        }
        self.drain()?;

        // Pass two: the worklist, to a fixpoint.
        while let Some(job) = self.queue.pop_front() {
            if self.instances.contains_key(&job.mangled) {
                continue;
            }
            if self.instances.len() >= MAX_INSTANTIATIONS {
                return Err(TypeError::TooManyInstantiations {
                    span: job.span,
                    name: job.origin,
                    limit: MAX_INSTANTIATIONS,
                });
            }
            let Some(template) = self.generics.get(&job.origin).copied() else {
                return Err(TypeError::UnknownType {
                    span: job.span,
                    name: job.origin,
                });
            };
            let params = static_params(template);
            if params.len() != job.args.len() {
                return Err(TypeError::StaticArgumentCountMismatch {
                    span: job.span,
                    name: job.origin.clone(),
                    expected: params.len(),
                    found: job.args.len(),
                });
            }
            let mut subst = Subst::new();
            for (param, arg) in params.iter().zip(&job.args) {
                subst.insert(param.name.clone(), arg.clone());
            }
            // Reserve the key before substituting, so a declaration that
            // mentions itself -- which every F-bound does -- is a memo hit on
            // the first lookup instead of an infinite descent.
            self.instances.insert(
                job.mangled.clone(),
                Instance {
                    origin: job.origin.clone(),
                    decl: template.clone(),
                },
            );
            self.record_bounds(params, &subst, job.span)?;
            let built = self.decl(template, &subst, Some(&job.mangled))?;
            self.drain()?;
            if let Some(slot) = self.instances.get_mut(&job.mangled) {
                slot.decl = built;
            }
        }

        // Emission. Each instantiation is emitted at its template's position in
        // the declaration list, ordered by mangled name, so declaration order
        // stays a pure function of the source text -- which is what keeps tags,
        // and therefore switch arms, deterministic.
        let mut decls: Vec<Decl> = Vec::new();
        let mut ground = ground.into_iter().peekable();
        for (index, decl) in component.decls.iter().enumerate() {
            if static_params(decl).is_empty() {
                if let Some((_, d)) = ground.next_if(|(i, _)| *i == index) {
                    decls.push(d);
                }
                continue;
            }
            let name = decl_name(decl);
            for instance in self.instances.values() {
                if instance.origin == name {
                    decls.push(instance.decl.clone());
                }
            }
        }

        Ok(Component {
            name: component.name.clone(),
            exports: component.exports.clone(),
            imports: component.imports.clone(),
            decls,
            bounds: self.obligations,
            is_api: component.is_api,
            span: component.span,
        })
    }

    fn drain(&mut self) -> Result<(), TypeError> {
        for job in self.demand.drain(..) {
            self.queue.push_back(job);
        }
        Ok(())
    }

    /// A bound cannot be discharged here: subtyping needs the registry, and the
    /// registry is built from the component this pass produces. The obligation
    /// crosses the phase boundary instead, and `check` settles it.
    fn record_bounds(
        &mut self,
        params: &[StaticParam],
        subst: &Subst,
        span: Span,
    ) -> Result<(), TypeError> {
        for param in params {
            let Some(subject) = subst.get(&param.name) else {
                continue;
            };
            for bound in &param.bounds {
                let bound = self.ty(bound, subst)?;
                self.obligations.push(BoundObligation {
                    subject: subject.clone(),
                    bound,
                    parameter: param.name.clone(),
                    span,
                });
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- types

    fn ty(&mut self, t: &TypeRef, subst: &Subst) -> Result<TypeRef, TypeError> {
        if t.args.is_empty() {
            if let Some(replacement) = subst.get(&t.name) {
                return Ok(replacement.clone());
            }
            if self.generics.contains_key(&t.name) {
                return Err(TypeError::StaticArgumentsRequired {
                    span: t.span,
                    name: t.name.clone(),
                });
            }
            return Ok(t.clone());
        }

        let mut args = Vec::with_capacity(t.args.len());
        for a in &t.args {
            args.push(self.ty(a, subst)?);
        }
        if BUILTIN_CONSTRUCTORS.contains(&t.name.as_str()) {
            return Ok(TypeRef {
                name: t.name.clone(),
                args,
                span: t.span,
            });
        }
        if !self.generics.contains_key(&t.name) {
            return Err(TypeError::UnknownType {
                span: t.span,
                name: t.name.clone(),
            });
        }
        let mangled = mangle_static(&t.name, &args);
        self.request(&t.name, args, &mangled, t.span);
        Ok(TypeRef {
            name: mangled,
            args: Vec::new(),
            span: t.span,
        })
    }

    fn request(&mut self, origin: &str, args: Vec<TypeRef>, mangled: &str, span: Span) {
        if self.instances.contains_key(mangled) {
            return;
        }
        self.demand.push(Job {
            origin: origin.to_owned(),
            args,
            mangled: mangled.to_owned(),
            span,
        });
    }

    fn types(&mut self, list: &[TypeRef], subst: &Subst) -> Result<Vec<TypeRef>, TypeError> {
        let mut out = Vec::with_capacity(list.len());
        for t in list {
            out.push(self.ty(t, subst)?);
        }
        Ok(out)
    }

    fn params(&mut self, list: &[Param], subst: &Subst) -> Result<Vec<Param>, TypeError> {
        let mut out = Vec::with_capacity(list.len());
        for p in list {
            out.push(Param {
                name: p.name.clone(),
                ty: self.ty(&p.ty, subst)?,
                span: p.span,
            });
        }
        Ok(out)
    }

    // ------------------------------------------------------ declarations

    fn decl(
        &mut self,
        decl: &Decl,
        subst: &Subst,
        rename: Option<&str>,
    ) -> Result<Decl, TypeError> {
        Ok(match decl {
            Decl::Function(f) => Decl::Function(FnDecl {
                name: rename.unwrap_or(&f.name).to_owned(),
                static_params: Vec::new(),
                params: self.params(&f.params, subst)?,
                return_type: match &f.return_type {
                    Some(t) => Some(self.ty(t, subst)?),
                    None => None,
                },
                body: match &f.body {
                    Some(b) => Some(self.expr(b, subst)?),
                    None => None,
                },
                span: f.span,
            }),
            Decl::Trait(t) => Decl::Trait(TraitDecl {
                name: rename.unwrap_or(&t.name).to_owned(),
                static_params: Vec::new(),
                extends: self.types(&t.extends, subst)?,
                comprises: self.types(&t.comprises, subst)?,
                excludes: self.types(&t.excludes, subst)?,
                members: self.members(&t.members, subst)?,
                span: t.span,
            }),
            Decl::Object(o) => Decl::Object(ObjectDecl {
                name: rename.unwrap_or(&o.name).to_owned(),
                static_params: Vec::new(),
                params: match &o.params {
                    Some(p) => Some(self.params(p, subst)?),
                    None => None,
                },
                extends: self.types(&o.extends, subst)?,
                members: self.members(&o.members, subst)?,
                span: o.span,
            }),
        })
    }

    /// Method bodies are not walked. Dotted methods are parsed and never
    /// checked, so a body there can neither be compiled nor create demand; the
    /// signature is substituted so the shape of the declaration stays honest.
    fn members(&mut self, members: &[Member], subst: &Subst) -> Result<Vec<Member>, TypeError> {
        let mut out = Vec::with_capacity(members.len());
        for m in members {
            out.push(match m {
                Member::Field(f) => Member::Field(FieldDecl {
                    name: f.name.clone(),
                    ty: self.ty(&f.ty, subst)?,
                    init: match &f.init {
                        Some(e) => Some(self.expr(e, subst)?),
                        None => None,
                    },
                    mutable: f.mutable,
                    span: f.span,
                }),
                Member::Method(m) => Member::Method(MethodDecl {
                    name: m.name.clone(),
                    static_params: m.static_params.clone(),
                    params: if m.static_params.is_empty() {
                        self.params(&m.params, subst)?
                    } else {
                        m.params.clone()
                    },
                    return_type: m.return_type.clone(),
                    body: m.body.clone(),
                    span: m.span,
                }),
            });
        }
        Ok(out)
    }

    // ------------------------------------------------------- expressions

    fn expr(&mut self, e: &Expr, subst: &Subst) -> Result<Expr, TypeError> {
        Ok(match e {
            Expr::IntLit { .. }
            | Expr::FloatLit { .. }
            | Expr::StrLit { .. }
            | Expr::BoolLit { .. } => e.clone(),

            Expr::Var { name, span } => {
                if self.generics.contains_key(name) {
                    return Err(TypeError::StaticArgumentsRequired {
                        span: *span,
                        name: name.clone(),
                    });
                }
                e.clone()
            }

            Expr::Instantiate { callee, args, span } => {
                let Expr::Var { name, .. } = callee.as_ref() else {
                    return Err(TypeError::StaticArgumentsRequired {
                        span: *span,
                        name: "<expression>".to_owned(),
                    });
                };
                if !self.generics.contains_key(name) {
                    return Err(TypeError::NotGeneric {
                        span: *span,
                        name: name.clone(),
                    });
                }
                let args = self.types(args, subst)?;
                let mangled = mangle_static(name, &args);
                self.request(name, args, &mangled, *span);
                Expr::Var {
                    name: mangled,
                    span: *span,
                }
            }

            Expr::Juxt { items, span } => Expr::Juxt {
                items: self.exprs(items, subst)?,
                span: *span,
            },
            Expr::Infix {
                op,
                fixity,
                lhs,
                rhs,
                span,
            } => Expr::Infix {
                op: *op,
                fixity: *fixity,
                lhs: Box::new(self.expr(lhs, subst)?),
                rhs: Box::new(self.expr(rhs, subst)?),
                span: *span,
            },
            Expr::Prefix { op, operand, span } => Expr::Prefix {
                op: *op,
                operand: Box::new(self.expr(operand, subst)?),
                span: *span,
            },
            Expr::Call { callee, args, span } => Expr::Call {
                callee: Box::new(self.expr(callee, subst)?),
                args: self.exprs(args, subst)?,
                span: *span,
            },
            Expr::If {
                cond,
                then_branch,
                else_branch,
                span,
            } => Expr::If {
                cond: Box::new(self.expr(cond, subst)?),
                then_branch: Box::new(self.expr(then_branch, subst)?),
                else_branch: match else_branch {
                    Some(b) => Some(Box::new(self.expr(b, subst)?)),
                    None => None,
                },
                span: *span,
            },
            Expr::Block { items, span } => Expr::Block {
                items: self.block(items, subst)?,
                span: *span,
            },
            Expr::ArrayLit { items, span } => Expr::ArrayLit {
                items: self.exprs(items, subst)?,
                span: *span,
            },
            Expr::Index { base, index, span } => Expr::Index {
                base: Box::new(self.expr(base, subst)?),
                index: Box::new(self.expr(index, subst)?),
                span: *span,
            },
            Expr::While { cond, body, span } => Expr::While {
                cond: Box::new(self.expr(cond, subst)?),
                body: Box::new(self.expr(body, subst)?),
                span: *span,
            },
            Expr::Field { base, name, span } => Expr::Field {
                base: Box::new(self.expr(base, subst)?),
                name: name.clone(),
                span: *span,
            },
        })
    }

    fn exprs(&mut self, list: &[Expr], subst: &Subst) -> Result<Vec<Expr>, TypeError> {
        let mut out = Vec::with_capacity(list.len());
        for e in list {
            out.push(self.expr(e, subst)?);
        }
        Ok(out)
    }

    fn block(&mut self, items: &[BlockItem], subst: &Subst) -> Result<Vec<BlockItem>, TypeError> {
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            out.push(match item {
                BlockItem::Binding(b) => BlockItem::Binding(fortress_ast::Binding {
                    name: b.name.clone(),
                    ty: match &b.ty {
                        Some(t) => Some(self.ty(t, subst)?),
                        None => None,
                    },
                    value: self.expr(&b.value, subst)?,
                    mutable: b.mutable,
                    span: b.span,
                }),
                BlockItem::Assign(a) => BlockItem::Assign(Assign {
                    target: self.expr(&a.target, subst)?,
                    value: self.expr(&a.value, subst)?,
                    span: a.span,
                }),
                BlockItem::Expr(e) => BlockItem::Expr(self.expr(e, subst)?),
            });
        }
        Ok(out)
    }
}

/// Injective by construction: the terminator is what distinguishes
/// `Foo[\Bar[\X\]\]` from `Foo[\Bar, X\]`, and `$` cannot appear in an
/// identifier, so no source name can collide with a mangled one.
#[must_use]
pub fn mangle_static(name: &str, args: &[TypeRef]) -> String {
    if args.is_empty() {
        return name.to_owned();
    }
    let mut out = String::from(name);
    for a in args {
        out.push('$');
        out.push_str(&mangle_static(&a.name, &a.args));
    }
    out.push_str("$e");
    out
}

/// Specification 1.0 `basic/overloading.tex:100-108`: two declarations of one
/// functional name may not differ in their static parameters, nor may one have
/// them and another not. Enforcing it is what makes an overload set uniformly
/// generic or uniformly ground, which is what makes monomorphizing one produce a
/// fresh disjoint set rather than adding a member to an existing one -- and
/// therefore what makes the dispatch tables built after this pass correct.
fn check_uniformity(component: &Component) -> Result<(), TypeError> {
    let mut seen: BTreeMap<&str, (&[StaticParam], Span)> = BTreeMap::new();
    for decl in &component.decls {
        let Decl::Function(f) = decl else { continue };
        let params = f.static_params.as_slice();
        match seen.get(f.name.as_str()) {
            None => {
                seen.insert(&f.name, (params, f.span));
            }
            Some((first, first_span)) => {
                let same = first.len() == params.len()
                    && first
                        .iter()
                        .zip(params)
                        .all(|(a, b)| a.bounds.len() == b.bounds.len());
                if !same {
                    return Err(TypeError::OverloadSetStaticParamsDiffer {
                        span: f.span,
                        name: f.name.clone(),
                        first: *first_span,
                    });
                }
            }
        }
    }
    Ok(())
}

fn static_params(decl: &Decl) -> &[StaticParam] {
    match decl {
        Decl::Function(f) => &f.static_params,
        Decl::Trait(t) => &t.static_params,
        Decl::Object(o) => &o.static_params,
    }
}

fn decl_name(decl: &Decl) -> &str {
    match decl {
        Decl::Function(f) => &f.name,
        Decl::Trait(t) => &t.name,
        Decl::Object(o) => &o.name,
    }
}

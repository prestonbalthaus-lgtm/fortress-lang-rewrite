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

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use fortress_ast::{
    Assign, BlockItem, BoundObligation, CaseArm, Component, CutMember, Decl, Expr, ExtentRange,
    FieldDecl, FnDecl, Member, MethodDecl, ObjectDecl, Param, Span, StaticExpr, StaticOp,
    StaticParam, TraitDecl, TypeCaseArm, TypeRef,
};

use crate::error::TypeError;
use crate::types::BUILTIN_TYPE_NAMES;

/// The total ceiling, per component. Depth and type-size limits are both
/// insufficient on their own: an *acyclic* call graph of k+1 declarations, each
/// handing its callee two different wrapper types, produces 2^(k+1)-1 distinct
/// instantiations with every type small. Only a total count catches that.
pub const MAX_INSTANTIATIONS: usize = 4096;

/// Type constructors the language provides rather than the program. These keep
/// their arguments through substitution; everything else with arguments is a
/// user generic and gets mangled away.
use crate::types::BUILTIN_TYPE_CONSTRUCTORS as BUILTIN_CONSTRUCTORS;

type Subst = BTreeMap<String, TypeRef>;

/// Ordered, and that is load bearing rather than incidental. Instantiations are
/// emitted in this map's order, tags follow declaration order, and switch arms
/// follow tags -- so a hash map here would make the emitted object depend on
/// iteration order instead of on the source text.
///
/// Keyed by mangled name *and member index*, because a generic overload set
/// instantiates to a ground overload set: every member is a distinct
/// declaration that happens to share one mangled name. Keying by the name alone
/// kept exactly one of them.
type Instances = BTreeMap<(String, usize), Instance>;

/// One instantiation still to be produced.
struct Job {
    origin: String,
    args: Vec<TypeRef>,
    mangled: String,
    span: Span,
}

/// Which emitted declaration a method stamp belongs to. A stamp goes into the
/// member list of exactly one declaration, and after expansion that is either a
/// ground declaration or one instantiation of a generic one.
///
/// Ordered, for the same reason `Instances` is: emission has to be a pure
/// function of the source text or tags, and therefore switch arms, move.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum OwnerKey {
    Ground(usize),
    Instance(String, usize),
}

/// A generic method as written, plus the substitution its owner was built
/// under. The two are kept apart until a stamp needs them, because walking the
/// body with the owner's substitution alone would meet the method's own static
/// parameters unbound and mangle a request for a type that does not exist.
#[derive(Clone)]
struct MethodTemplate {
    owner_name: String,
    decl: MethodDecl,
    subst: Subst,
}

/// One `m[\Args\]` written at a call site. The receiver is deliberately not
/// part of it: this pass has no types, so demand is by name and arity only.
#[derive(Clone)]
struct MethodRequest {
    name: String,
    args: Vec<TypeRef>,
    value_arity: usize,
    mangled: String,
    span: Span,
}

struct Instance {
    origin: String,
    /// Which member of the origin's overload set this came from. Emission uses
    /// it to place each instance under its own source declaration exactly once.
    member: usize,
    decl: Decl,
}

pub fn expand(component: &Component, uniformity: Uniformity) -> Result<Component, TypeError> {
    check_template_headers(component)?;
    Expander::new(component, uniformity)?.run(component)
}

/// Whether the static-parameter uniformity rule below is enforced against this
/// component's own declarations.
///
/// `ExemptLegacyLibrary` exists because the SHIPPED 1.0 LIBRARY BREAKS A RULE
/// 1.0 STATES. `Library/FortressLibrary.fsi:758` declares `__cond[\E,R\]`
/// beside `__cond[\E\]`, and `Library/QuickSort.fsi` is refused for the same
/// class. The rule is not being relaxed: it is suspended for the legacy
/// library's own source, which this project treats as a MEASUREMENT INPUT
/// rather than as code it owns (DEV-14).
///
/// IT IS DELIBERATELY NOT GENERAL, and the cost of making it general is
/// measured: suspending the rule for every component gains three corpus files
/// and ONE OF THE THREE IS A MUST-FAIL TEST --
/// `ProjectFortress/compiler_tests/Compiled6.ak.fss`, whose `XXX6ak.test`
/// expects two errors. A new must-fail acceptance is a regression, not a
/// baseline, so the general version cannot be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uniformity {
    Enforced,
    ExemptLegacyLibrary,
}

struct Expander<'a> {
    /// Every member, in declaration order. A generic overload set has more than
    /// one, and each needs its own body instantiated.
    generics: BTreeMap<String, Vec<&'a Decl>>,
    /// Mangled name to the instantiation, ordered so emission is a pure
    /// function of the source rather than of worklist discovery order.
    instances: Instances,
    queue: VecDeque<Job>,
    obligations: Vec<BoundObligation>,
    demand: Vec<Job>,
    /// Names of functional methods that take static parameters. They are not
    /// lifted, and this pass is the only one that sees `f[\ZZ32\](o, x)`
    /// before the checker, so it reports the mechanism or nothing does.
    generic_functional: BTreeSet<String>,
    /// Names of generic *dotted* methods, from the source. Needed at rewrite
    /// time, before any template has necessarily been registered.
    generic_methods: BTreeSet<String>,
    /// Which declaration `members` is currently expanding, so a generic method
    /// it meets can be filed under the owner it will be emitted into.
    owner: Option<OwnerKey>,
    /// The owner's name in the EMITTED component -- the source name for a
    /// ground declaration, the mangled one for an instantiation. It is what a
    /// speculative bound obligation names, so the checker can find the stamp.
    owner_name: String,
    /// The origin name and static-parameter names of the instantiation being
    /// expanded, so a member signature can be asked whether it demands that
    /// same declaration at a strictly larger type.
    current_owner: Option<(String, Vec<String>)>,
    /// See `Component::cuts`.
    cuts: Vec<CutMember>,
    templates: BTreeMap<(OwnerKey, usize), MethodTemplate>,
    /// Keyed by mangled name and value arity: one written `m[\ZZ32\](x)` is
    /// one request no matter how many times it appears.
    method_demand: BTreeMap<(String, usize), MethodRequest>,
    stamps: BTreeMap<(OwnerKey, usize, String), MethodDecl>,
}

impl<'a> Expander<'a> {
    fn new(component: &'a Component, uniformity: Uniformity) -> Result<Self, TypeError> {
        let mut generics: BTreeMap<String, Vec<&'a Decl>> = BTreeMap::new();
        for decl in &component.decls {
            if static_params(decl).is_empty() {
                continue;
            }
            generics
                .entry(decl_name(decl).to_owned())
                .or_default()
                .push(decl);
        }
        if uniformity == Uniformity::Enforced {
            check_uniformity(component)?;
        }
        let mut generic_functional = BTreeSet::new();
        let mut generic_methods = BTreeSet::new();
        for decl in &component.decls {
            for member in members_of(decl) {
                let Member::Method(m) = member else { continue };
                if m.accessor || m.static_params.is_empty() {
                    continue;
                }
                if m.params.iter().any(|p| p.name == "self") {
                    generic_functional.insert(m.name.clone());
                } else {
                    generic_methods.insert(m.name.clone());
                }
            }
        }
        Ok(Self {
            generics,
            instances: Instances::new(),
            queue: VecDeque::new(),
            obligations: Vec::new(),
            demand: Vec::new(),
            generic_functional,
            generic_methods,
            owner: None,
            owner_name: String::new(),
            current_owner: None,
            cuts: Vec::new(),
            templates: BTreeMap::new(),
            method_demand: BTreeMap::new(),
            stamps: BTreeMap::new(),
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
            self.owner = Some(OwnerKey::Ground(index));
            self.owner_name = decl_name(decl).to_owned();
            let rewritten = self.decl(decl, &empty, None)?;
            ground.push((index, rewritten));
        }
        self.owner = None;
        self.drain()?;

        // Pass two: types and method stamps to a JOINT fixpoint. They are not
        // two passes one after the other: a stamped body can demand a type
        // instantiation, and a type instantiation registers method templates a
        // stamp still has to be made from.
        loop {
            self.expand_types()?;
            if !self.stamp_methods()? {
                break;
            }
        }

        Ok(self.finish(component, ground))
    }

    /// One batch of stamps: every registered template crossed with every
    /// request whose name and arities it matches, minus what is already
    /// stamped. Returns whether anything was made, which is what the joint
    /// fixpoint loops on.
    ///
    /// The cross product IS the over-approximation. Expansion has no types, so
    /// it cannot know which type `o` in `o.m[\String\]()` is; it stamps every
    /// type that declares a generic `m` of matching arity and lets M3c's
    /// dispatch pick the winner by receiver. Stamps nothing reaches are dead
    /// code, and `MAX_INSTANTIATIONS` is what bounds the guessing.
    fn stamp_methods(&mut self) -> Result<bool, TypeError> {
        let mut work: Vec<((OwnerKey, usize), MethodRequest)> = Vec::new();
        for (tkey, template) in &self.templates {
            for request in self.method_demand.values() {
                if template.decl.name != request.name
                    || template.decl.static_params.len() != request.args.len()
                    || template.decl.params.len() != request.value_arity
                {
                    continue;
                }
                let key = (tkey.0.clone(), tkey.1, request.mangled.clone());
                if self.stamps.contains_key(&key) {
                    continue;
                }
                work.push((tkey.clone(), request.clone()));
            }
        }
        if work.is_empty() {
            return Ok(false);
        }
        for (tkey, request) in work {
            let Some(template) = self.templates.get(&tkey).cloned() else {
                continue;
            };
            if self.total() >= MAX_INSTANTIATIONS {
                return Err(TypeError::TooManyInstantiations {
                    span: request.span,
                    name: request.name,
                    limit: MAX_INSTANTIATIONS,
                });
            }
            // The owner's substitution and the method's own, composed and
            // applied in one walk. The method's parameters win, which is what
            // makes `trait T[\S\] ... g[\S\]()` shadow rather than collide.
            let mut subst = template.subst.clone();
            bind_static(&template.decl.static_params, &request.args, &mut subst)?;
            let key = (tkey.0.clone(), tkey.1, request.mangled.clone());
            // Reserved before the body is walked, so a generic method that
            // calls itself at its own arguments is a memo hit rather than an
            // infinite descent.
            self.stamps.insert(key.clone(), template.decl.clone());
            self.record_bounds(
                &template.decl.static_params,
                &subst,
                request.span,
                Some((template.owner_name.clone(), request.mangled.clone())),
                None,
            )?;
            let built = MethodDecl {
                // Copied, never re-defaulted: a monomorphized stamp that
                // silently lost `abstract` would be a lie the AST tells.
                modifiers: template.decl.modifiers,
                name: request.mangled.clone(),
                static_params: Vec::new(),
                params: self.params(&template.decl.params, &subst)?,
                return_type: match &template.decl.return_type {
                    Some(t) => Some(self.ty(t, &subst)?),
                    None => None,
                },
                body: match &template.decl.body {
                    Some(b) => Some(self.expr(b, &subst)?),
                    None => None,
                },
                accessor: template.decl.accessor,
                span: template.decl.span,
            };
            self.drain()?;
            self.stamps.insert(key, built);
        }
        Ok(true)
    }

    /// One ceiling for the whole component, counting type instantiations and
    /// method stamps together. Two separate ceilings would each be reachable
    /// while the total was twice what was intended.
    fn total(&self) -> usize {
        self.instances.len().saturating_add(self.stamps.len())
    }

    fn expand_types(&mut self) -> Result<(), TypeError> {
        while let Some(job) = self.queue.pop_front() {
            if self.instances.contains_key(&(job.mangled.clone(), 0)) {
                continue;
            }
            if self.total() >= MAX_INSTANTIATIONS {
                return Err(TypeError::TooManyInstantiations {
                    span: job.span,
                    name: job.origin,
                    limit: MAX_INSTANTIATIONS,
                });
            }
            let Some(templates) = self.generics.get(&job.origin).cloned() else {
                return Err(TypeError::UnknownType {
                    span: job.span,
                    name: job.origin,
                });
            };
            // Every member of the set instantiates at these arguments.
            // `check_uniformity` has already established that they agree on how
            // many static parameters they take, but not on what those are
            // *called*, so each member substitutes under its own names.
            for (member, template) in templates.iter().enumerate() {
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
                bind_static(params, &job.args, &mut subst)?;
                let key = (job.mangled.clone(), member);
                // Reserve the key before substituting, so a declaration that
                // mentions itself -- which every F-bound does -- is a memo hit
                // on the first lookup instead of an infinite descent.
                self.instances.insert(
                    key.clone(),
                    Instance {
                        origin: job.origin.clone(),
                        member,
                        decl: (*template).clone(),
                    },
                );
                // EVERY member of the set instantiates at these arguments, so
                // every member's bound is recorded -- and only one of them can
                // be the one the call meant. Carry the member's own span so a
                // bound that does not hold prunes that member instead of
                // refusing the program.
                let member_span =
                    (templates.len() > 1).then(|| (job.mangled.clone(), decl_span(template)));
                self.record_bounds(params, &subst, job.span, None, member_span)?;
                self.owner = Some(OwnerKey::Instance(job.mangled.clone(), member));
                self.owner_name.clone_from(&job.mangled);
                self.current_owner = Some((
                    job.origin.clone(),
                    params.iter().map(|p| p.name.clone()).collect(),
                ));
                let built = self.decl(template, &subst, Some(&job.mangled))?;
                self.current_owner = None;
                self.owner = None;
                self.drain()?;
                if let Some(slot) = self.instances.get_mut(&key) {
                    slot.decl = built;
                }
            }
        }
        Ok(())
    }

    /// Emission. Each instantiation is emitted at its template's position in
    /// the declaration list, ordered by mangled name, so declaration order
    /// stays a pure function of the source text -- which is what keeps tags,
    /// and therefore switch arms, deterministic. Stamps are appended to the
    /// member list of the declaration they were filed under, in the same
    /// ordered-map order and for the same reason.
    fn emit(&self, component: &Component, ground: Vec<(usize, Decl)>) -> Vec<Decl> {
        let mut emitted: Vec<(OwnerKey, Decl)> = Vec::new();
        let mut seen_members: BTreeMap<&str, usize> = BTreeMap::new();
        let mut ground = ground.into_iter().peekable();
        for (index, decl) in component.decls.iter().enumerate() {
            if static_params(decl).is_empty() {
                if let Some((_, d)) = ground.next_if(|(i, _)| *i == index) {
                    emitted.push((OwnerKey::Ground(index), d));
                }
                continue;
            }
            // Each source declaration emits only the instances built from
            // *it*. Matching on the name alone pushed every member's instance
            // once per member, so a two-member set emitted each body twice and
            // the checker reported it as a duplicate definition.
            let name = decl_name(decl);
            let member = *seen_members.entry(name).or_insert(0);
            seen_members.insert(name, member + 1);
            for ((mangled, slot), instance) in &self.instances {
                if instance.origin == name && instance.member == member {
                    emitted.push((
                        OwnerKey::Instance(mangled.clone(), *slot),
                        instance.decl.clone(),
                    ));
                }
            }
        }

        let mut decls = Vec::with_capacity(emitted.len());
        for (key, mut decl) in emitted {
            let extra: Vec<Member> = self
                .stamps
                .iter()
                .filter(|((owner, _, _), _)| *owner == key)
                .map(|(_, m)| Member::Method(m.clone()))
                .collect();
            if !extra.is_empty() {
                match &mut decl {
                    Decl::Trait(t) => t.members.extend(extra),
                    Decl::Object(o) => o.members.extend(extra),
                    Decl::Function(_) => {}
                }
            }
            decls.push(decl);
        }
        decls
    }

    fn finish(self, component: &Component, ground: Vec<(usize, Decl)>) -> Component {
        let decls = self.emit(component, ground);
        Component {
            name: component.name.clone(),
            exports: component.exports.clone(),
            imports: component.imports.clone(),
            decls,
            bounds: self.obligations,
            cuts: self.cuts,
            is_api: component.is_api,
            dims: component.dims.clone(),
            units: component.units.clone(),
            span: component.span,
        }
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
        speculative: Option<(String, String)>,
        overload_member: Option<(String, Span)>,
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
                    speculative: speculative.clone(),
                    overload_member: overload_member.clone(),
                    span,
                });
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- types

    fn ty(&mut self, t: &TypeRef, subst: &Subst) -> Result<TypeRef, TypeError> {
        // The three non-nominal forms substitute structurally. None of them can
        // ever be an instantiation request, so none of them reaches `request`.
        let (name, args, span) = match t {
            TypeRef::Named { name, args, span } => (name, args, *span),
            TypeRef::Unit { .. } => return Ok(t.clone()),
            // A shape substitutes structurally, ELEMENT AND EXTENTS BOTH. The
            // extents are where `ZZ32[n]` inside `f[\nat n\]` gets its number,
            // and they are ordinary static arguments, so they go through this
            // same function and are evaluated at the substitution like every
            // other one.
            TypeRef::Shaped {
                base,
                spelling,
                extents,
                span,
            } => {
                let mut substituted = Vec::with_capacity(extents.len());
                for extent in extents {
                    substituted.push(ExtentRange {
                        lower: match &extent.lower {
                            Some(l) => Some(self.ty(l, subst)?),
                            None => None,
                        },
                        upper: match &extent.upper {
                            Some(u) => Some(self.ty(u, subst)?),
                            None => None,
                        },
                        form: extent.form,
                        span: extent.span,
                    });
                }
                return Ok(TypeRef::Shaped {
                    base: Box::new(self.ty(base, subst)?),
                    spelling: *spelling,
                    extents: substituted,
                    span: *span,
                });
            }
            // A STATIC VALUE IS EVALUATED HERE, at the substitution, and never
            // later. `[\ 2 + 3 \]` and `[\ 5 \]` must be ONE stamp against
            // MAX_INSTANTIATIONS, and they are only one if the value and not
            // the expression is what gets mangled.
            TypeRef::Static { expr, span } => {
                return Ok(TypeRef::Static {
                    expr: eval_static(expr, subst, *span)?,
                    span: *span,
                })
            }
            TypeRef::Tuple { elems, span } => {
                let mut out = Vec::with_capacity(elems.len());
                for e in elems {
                    out.push(self.ty(e, subst)?);
                }
                return Ok(TypeRef::Tuple {
                    elems: out,
                    span: *span,
                });
            }
            TypeRef::Arrow { from, to, span } => {
                return Ok(TypeRef::Arrow {
                    from: Box::new(self.ty(from, subst)?),
                    to: Box::new(self.ty(to, subst)?),
                    span: *span,
                })
            }
        };

        if args.is_empty() {
            if let Some(replacement) = subst.get(name) {
                return Ok(replacement.clone());
            }
            if self.generics.contains_key(name) {
                return Err(TypeError::StaticArgumentsRequired {
                    span,
                    name: name.clone(),
                });
            }
            return Ok(t.clone());
        }

        let mut expanded = Vec::with_capacity(args.len());
        for a in args {
            expanded.push(self.ty(a, subst)?);
        }
        // A DECLARED GENERIC OF THE SAME NAME WINS, the same order
        // `resolve_name` takes for the builtin SCALARS. Without the second
        // condition `Array` and `Thread` are a reserved namespace that a
        // component cannot shadow -- and 1.0 declares one:
        // `ProjectFortress/LibraryBuiltin/FortressBuiltin.fsi:209` writes
        // `object Thread[\T\](fcn:()->T)`. Expansion would hand the reference
        // straight through and the registry would build the builtin instead of
        // stamping the declaration.
        if BUILTIN_CONSTRUCTORS.contains(&name.as_str()) && !self.generics.contains_key(name) {
            return Ok(TypeRef::Named {
                name: name.clone(),
                args: expanded,
                span,
            });
        }
        if !self.generics.contains_key(name) {
            return Err(TypeError::UnknownType {
                span,
                name: name.clone(),
            });
        }
        let mangled = mangle_static(name, &expanded);
        self.request(name, expanded, &mangled, span);
        Ok(TypeRef::Named {
            name: mangled,
            args: Vec::new(),
            span,
        })
    }

    fn request(&mut self, origin: &str, args: Vec<TypeRef>, mangled: &str, span: Span) {
        // Member 0 is reserved first, so its presence means the whole set has
        // already been requested at these arguments.
        if self.instances.contains_key(&(mangled.to_owned(), 0)) {
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
                varargs: p.varargs,
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
                modifiers: f.modifiers,
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
                value_binding: f.value_binding,
                span: f.span,
            }),
            Decl::Trait(t) => Decl::Trait(TraitDecl {
                modifiers: t.modifiers,
                name: rename.unwrap_or(&t.name).to_owned(),
                static_params: Vec::new(),
                extends: self.types(&t.extends, subst)?,
                comprises: self.types(&t.comprises, subst)?,
                comprises_open: t.comprises_open,
                excludes: self.types(&t.excludes, subst)?,
                members: self.members(&t.members, subst)?,
                span: t.span,
            }),
            Decl::Object(o) => Decl::Object(ObjectDecl {
                modifiers: o.modifiers,
                name: rename.unwrap_or(&o.name).to_owned(),
                static_params: Vec::new(),
                params: match &o.params {
                    Some(p) => Some(self.params(p, subst)?),
                    None => None,
                },
                extends: self.types(&o.extends, subst)?,
                comprises: self.types(&o.comprises, subst)?,
                comprises_open: o.comprises_open,
                excludes: self.types(&o.excludes, subst)?,
                members: self.members(&o.members, subst)?,
                span: o.span,
            }),
        })
    }

    /// A ground method is substituted whole -- parameters, return type and
    /// body. M3i checks method bodies, so leaving the return type alone made
    /// `get(): T = v` inside `Cell[\T\]` refuse with `unknown type T`, and
    /// leaving the body alone swallowed every instantiation request written
    /// inside one.
    ///
    /// A generic method is left exactly as written. Its body may name its own
    /// static parameters, and walking `Cell[\S\]` with `S` unbound would
    /// mangle a request for a type that does not exist.
    /// Does this member's signature demand its OWN owner at static arguments
    /// that properly contain the owner's own static parameters? That is the one
    /// shape a monomorphizing compiler cannot stamp: every round adds a layer
    /// and the chain has no fixpoint. `Library/FortressLibrary.fsi:1138` is the
    /// live witness -- `trait Indexed[\E,I\]` declares
    /// `getter indexValuePairs(): Indexed[\(I,E),I\]`.
    ///
    /// Deliberately NOT a depth or budget test. `reverse(): Indexed[\E,I\]`
    /// names the owner at its own arguments and is a memo hit; `map[\R\](...):
    /// Indexed[\R,I\]` names it at a FRESH parameter and converges. Only a
    /// COMPOSITE argument that mentions an owner parameter grows.
    fn signature_grows_its_owner(&self, m: &MethodDecl) -> bool {
        let Some((origin, params)) = &self.current_owner else {
            return false;
        };
        let mut grows = false;
        for t in m.params.iter().map(|p| &p.ty).chain(m.return_type.iter()) {
            walk_type(t, &mut |node| {
                if let TypeRef::Named { name, args, .. } = node {
                    if name == origin && args.iter().any(|a| properly_contains_param(a, params)) {
                        grows = true;
                    }
                }
            });
        }
        grows
    }

    fn members(&mut self, members: &[Member], subst: &Subst) -> Result<Vec<Member>, TypeError> {
        let mut out = Vec::with_capacity(members.len());
        for (index, m) in members.iter().enumerate() {
            let built = match m {
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
                // FILED, NOT EXPANDED -- the same treatment the generic arm
                // below gives, and for the same reason. Walking this signature
                // demands the owner at a larger type, which demands it larger
                // again. A DECLARATION IS NOT A DEMAND; a call is.
                Member::Method(m)
                    if m.static_params.is_empty() && self.signature_grows_its_owner(m) =>
                {
                    let origin = self
                        .current_owner
                        .as_ref()
                        .map(|(o, _)| o.clone())
                        .unwrap_or_default();
                    self.cuts.push(CutMember {
                        owner: self.owner_name.clone(),
                        origin,
                        member: m.name.clone(),
                        span: m.span,
                    });
                    continue;
                }
                Member::Method(m) if m.static_params.is_empty() => Member::Method(MethodDecl {
                    modifiers: m.modifiers,
                    name: m.name.clone(),
                    static_params: Vec::new(),
                    params: self.params(&m.params, subst)?,
                    return_type: match &m.return_type {
                        Some(t) => Some(self.ty(t, subst)?),
                        None => None,
                    },
                    body: match &m.body {
                        Some(b) => Some(self.expr(b, subst)?),
                        None => None,
                    },
                    accessor: m.accessor,
                    span: m.span,
                }),
                Member::Method(m) => {
                    // A generic method is filed, not expanded. It stays in the
                    // member list as written -- the checker skips it -- and a
                    // stamp of it is appended later, once some call site has
                    // said at what arguments.
                    if let Some(owner) = self.owner.clone() {
                        if !m.accessor && !m.params.iter().any(|p| p.name == "self") {
                            self.templates.insert(
                                (owner, index),
                                MethodTemplate {
                                    owner_name: self.owner_name.clone(),
                                    decl: m.clone(),
                                    subst: subst.clone(),
                                },
                            );
                        }
                    }
                    Member::Method(m.clone())
                }
            };
            out.push(built);
        }
        Ok(out)
    }

    // ------------------------------------------------------- expressions

    fn expr(&mut self, e: &Expr, subst: &Subst) -> Result<Expr, TypeError> {
        Ok(match e {
            Expr::Unit { .. }
            | Expr::IntLit { .. }
            | Expr::FloatLit { .. }
            | Expr::StrLit { .. }
            | Expr::CharLit { .. }
            | Expr::BoolLit { .. } => e.clone(),

            Expr::Var { name, span } => {
                // A VALUE STATIC PARAMETER READ AS A VALUE. The substitution
                // already holds the number, so this is the whole mechanism --
                // the checker downstream sees an ordinary literal and needed no
                // change at all.
                if let Some(literal) = value_parameter_literal(name, subst, *span) {
                    return Ok(literal);
                }
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
                    // `o.m[\String\]()` is a generic *method* call. Its static
                    // arguments are written; what is missing is dotted method
                    // dispatch, and expansion runs before the checker, so this
                    // pass reports it or nothing does. Saying "write its static
                    // arguments" about a site that has written them sent nine
                    // corpus files into the wrong blocker bucket.
                    if let Expr::Field { name, .. } = callee.as_ref() {
                        return Err(TypeError::DottedMethodUnsupported {
                            span: *span,
                            name: name.clone(),
                        });
                    }
                    return Err(TypeError::StaticArgumentsRequired {
                        span: *span,
                        name: "<expression>".to_owned(),
                    });
                };
                if !self.generics.contains_key(name) {
                    if self.generic_functional.contains(name) {
                        return Err(TypeError::GenericFunctionalMethodUnsupported {
                            span: *span,
                            name: name.clone(),
                        });
                    }
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
            Expr::Call { callee, args, span } => {
                // `o.m[\String\](y)`, and the unqualified `f[\S\](y)` that
                // means `self.f[\S\](y)`. The value arity is only knowable
                // here, at the application, which is why this is not left to
                // the `Instantiate` arm below.
                if let Expr::Instantiate {
                    callee: inner,
                    args: type_args,
                    span: inner_span,
                } = callee.as_ref()
                {
                    if let Some(rewritten) =
                        self.method_instantiation(inner, type_args, args.len(), *inner_span, subst)?
                    {
                        return Ok(Expr::Call {
                            callee: Box::new(rewritten),
                            args: self.exprs(args, subst)?,
                            span: *span,
                        });
                    }
                }
                Expr::Call {
                    callee: Box::new(self.expr(callee, subst)?),
                    args: self.exprs(args, subst)?,
                    span: *span,
                }
            }
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
            Expr::ArrayLit {
                items,
                extents,
                span,
            } => Expr::ArrayLit {
                items: self.exprs(items, subst)?,
                extents: extents.clone(),
                span: *span,
            },
            // Refused by the checker, but expansion runs first and its elements
            // can still name a static parameter.
            Expr::Tuple { items, span } => Expr::Tuple {
                items: self.exprs(items, subst)?,
                span: *span,
            },
            Expr::Index {
                base,
                indices,
                span,
            } => Expr::Index {
                base: Box::new(self.expr(base, subst)?),
                indices: indices
                    .iter()
                    .map(|i| self.expr(i, subst))
                    .collect::<Result<Vec<_>, _>>()?,
                span: *span,
            },
            Expr::While { cond, body, span } => Expr::While {
                cond: Box::new(self.expr(cond, subst)?),
                body: Box::new(self.expr(body, subst)?),
                span: *span,
            },
            Expr::For {
                binder,
                lo,
                hi,
                inclusive,
                sequential,
                body,
                span,
            } => Expr::For {
                binder: binder.clone(),
                lo: Box::new(self.expr(lo, subst)?),
                hi: Box::new(self.expr(hi, subst)?),
                inclusive: *inclusive,
                sequential: *sequential,
                body: Box::new(self.expr(body, subst)?),
                span: *span,
            },
            Expr::Field { base, name, span } => Expr::Field {
                base: Box::new(self.expr(base, subst)?),
                name: name.clone(),
                span: *span,
            },
            Expr::Atomic { body, span } => Expr::Atomic {
                body: Box::new(self.expr(body, subst)?),
                span: *span,
            },
            // Substitution walks INTO a spawned body. It is ordinary code and a
            // static parameter mentioned inside one has to be replaced like any
            // other -- returning `e.clone()` here would leave the parameter
            // standing in an expansion that is supposed to be ground.
            Expr::Spawn { body, span } => Expr::Spawn {
                body: Box::new(self.expr(body, subst)?),
                span: *span,
            },

            Expr::Case {
                subject,
                arms,
                else_arm,
                span,
            } => Expr::Case {
                subject: Box::new(self.expr(subject, subst)?),
                arms: arms
                    .iter()
                    .map(|a| {
                        Ok(CaseArm {
                            guard: self.expr(&a.guard, subst)?,
                            body: self.expr(&a.body, subst)?,
                            span: a.span,
                        })
                    })
                    .collect::<Result<_, TypeError>>()?,
                else_arm: match else_arm {
                    Some(e) => Some(Box::new(self.expr(e, subst)?)),
                    None => None,
                },
                span: *span,
            },
            // The arm TYPE substitutes like any other written type, which is
            // what lets `typecase x of T => ...` appear inside a generic body.
            Expr::TypeCase {
                subject,
                arms,
                else_arm,
                span,
            } => Expr::TypeCase {
                subject: Box::new(self.expr(subject, subst)?),
                arms: arms
                    .iter()
                    .map(|a| {
                        Ok(TypeCaseArm {
                            binder: a.binder.clone(),
                            ty: self.ty(&a.ty, subst)?,
                            body: self.expr(&a.body, subst)?,
                            span: a.span,
                        })
                    })
                    .collect::<Result<_, TypeError>>()?,
                else_arm: Box::new(self.expr(else_arm, subst)?),
                span: *span,
            },
            Expr::Label { name, body, span } => Expr::Label {
                name: name.clone(),
                body: Box::new(self.expr(body, subst)?),
                span: *span,
            },
            Expr::ForIn {
                binder,
                source,
                sequential,
                body,
                span,
            } => Expr::ForIn {
                binder: binder.clone(),
                source: Box::new(self.expr(source, subst)?),
                sequential: *sequential,
                body: Box::new(self.expr(body, subst)?),
                span: *span,
            },
            Expr::AlsoDo { blocks, span } => Expr::AlsoDo {
                blocks: self.exprs(blocks, subst)?,
                span: *span,
            },
            Expr::BigReduction {
                op,
                binder,
                lo,
                hi,
                inclusive,
                sequential,
                body,
                span,
            } => Expr::BigReduction {
                op: *op,
                binder: binder.clone(),
                lo: Box::new(self.expr(lo, subst)?),
                hi: Box::new(self.expr(hi, subst)?),
                inclusive: *inclusive,
                sequential: *sequential,
                body: Box::new(self.expr(body, subst)?),
                span: *span,
            },
            Expr::Lambda {
                params,
                return_type,
                body,
                span,
            } => Expr::Lambda {
                params: self.params(params, subst)?,
                return_type: match return_type {
                    Some(t) => Some(self.ty(t, subst)?),
                    None => None,
                },
                body: Box::new(self.expr(body, subst)?),
                span: *span,
            },
            Expr::Exit { name, value, span } => Expr::Exit {
                name: name.clone(),
                value: match value {
                    Some(e) => Some(Box::new(self.expr(e, subst)?)),
                    None => None,
                },
                span: *span,
            },
        })
    }

    /// A written `m[\Args\]` in callee position, if this is a generic METHOD
    /// call. `None` means it is something else -- a top-level generic function,
    /// or nothing this pass recognises -- and the ordinary path handles it.
    ///
    /// The rewrite is the whole mechanism: the call keeps its shape and only
    /// the name changes, so the checker meets an ordinary dotted call on a
    /// ground name and M3c's dispatch decides the receiver.
    fn method_instantiation(
        &mut self,
        callee: &Expr,
        type_args: &[TypeRef],
        value_arity: usize,
        span: Span,
        subst: &Subst,
    ) -> Result<Option<Expr>, TypeError> {
        let name = match callee {
            Expr::Field { name, .. } | Expr::Var { name, .. } => name.clone(),
            _ => return Ok(None),
        };
        // A top-level generic function wins: it is a real name in this
        // namespace, and the existing path already instantiates it.
        if matches!(callee, Expr::Var { .. }) && self.generics.contains_key(&name) {
            return Ok(None);
        }
        if !self.generic_methods.contains(&name) {
            if self.generic_functional.contains(&name) {
                return Err(TypeError::GenericFunctionalMethodUnsupported { span, name });
            }
            return Ok(None);
        }
        let args = self.types(type_args, subst)?;
        let mangled = mangle_static(&name, &args);
        self.method_demand
            .entry((mangled.clone(), value_arity))
            .or_insert_with(|| MethodRequest {
                name: name.clone(),
                args,
                value_arity,
                mangled: mangled.clone(),
                span,
            });
        Ok(Some(match callee {
            Expr::Field { base, span, .. } => Expr::Field {
                base: Box::new(self.expr(base, subst)?),
                name: mangled,
                span: *span,
            },
            _ => Expr::Var {
                name: mangled,
                span,
            },
        }))
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
                // Substitution walks INTO the initializer: a static parameter
                // mentioned there has to be replaced like any other.
                BlockItem::TupleBinding(b) => BlockItem::TupleBinding(fortress_ast::TupleBinding {
                    names: b.names.clone(),
                    value: self.expr(&b.value, subst)?,
                    span: b.span,
                }),
                BlockItem::Assign(a) => BlockItem::Assign(Assign {
                    target: self.expr(&a.target, subst)?,
                    op: a.op,
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
        out.push_str(&mangle_type(a));
    }
    out.push_str("$e");
    out
}

/// Bind one instantiation's arguments to its parameters, CHECKING THE KIND.
/// Two things are decided here and nowhere else: that a value parameter got a
/// value, and that a type parameter did not.
///
/// A BARE NAME REACHING A VALUE PARAMETER HAS ALREADY FAILED. `ty` substitutes
/// an enclosing parameter before the argument gets here, so a name that is
/// still a `Named` is one that resolved to nothing -- which is precisely what
/// "must be known at compile time" refuses. That is why the classification can
/// live here, after substitution, and demand can stay syntactic.
fn bind_static(params: &[StaticParam], args: &[TypeRef], out: &mut Subst) -> Result<(), TypeError> {
    for (param, arg) in params.iter().zip(args) {
        // SUB-PHASE 4d PARSES A `unit`/`dim` PARAMETER AND STOPS THERE.
        // Substituting one means deciding what a dimensioned value looks like
        // at run time, and there is no boxing in this backend to decide it
        // with. Refusing HERE and not at the declaration is deliberate:
        // `ProjectFortress/tests/dimensionUnitDecl.fss` DECLARES
        // `trait Float1[\unit U absorbs unit, nat e, nat s\]` and never
        // instantiates it, so refusing the declaration would cost a file for a
        // capability nothing asks for.
        if param.kind.is_dimensional() {
            return Err(TypeError::DimensionalParameterInstantiated {
                span: arg.span(),
                param: param.name.clone(),
                kind: param.kind.spelling(),
            });
        }
        let bound = match (param.kind.is_value(), arg) {
            (true, TypeRef::Static { .. }) => arg.clone(),
            (true, TypeRef::Named { name, args, span }) if args.is_empty() => {
                return Err(TypeError::StaticArgumentNotConstant {
                    span: *span,
                    name: name.clone(),
                })
            }
            (true, other) => {
                return Err(TypeError::TypeWhereStaticValueRequired {
                    span: other.span(),
                    param: param.name.clone(),
                    kind: param.kind.spelling(),
                    written: other.written(),
                })
            }
            (false, TypeRef::Static { expr, span }) => {
                return Err(TypeError::StaticValueWhereTypeRequired {
                    span: *span,
                    written: expr.written(),
                })
            }
            (false, _) => arg.clone(),
        };
        out.insert(param.name.clone(), bound);
    }
    Ok(())
}

/// A value parameter READ AS A VALUE in the body it parameterises:
/// `ImmutableArray1[\T, nat b0, nat s0\]` uses `s0` as a size. The
/// substitution already holds the number, so this is a rewrite and not an
/// evaluation, and what the checker sees afterwards is an ordinary literal.
fn value_parameter_literal(name: &str, subst: &Subst, span: Span) -> Option<Expr> {
    match subst.get(name) {
        Some(TypeRef::Static {
            expr: StaticExpr::Int(v),
            ..
        }) => Some(Expr::IntLit {
            digits: v.to_string(),
            span,
        }),
        Some(TypeRef::Static {
            expr: StaticExpr::Bool(v),
            ..
        }) => Some(Expr::BoolLit { value: *v, span }),
        _ => None,
    }
}

/// D7 §3.1's "statically evaluable". Substitutes every name from the
/// enclosing substitution and folds the arithmetic, so what comes out is a
/// literal or a diagnostic and never a half-folded expression.
///
/// EVALUATING AT THE SUBSTITUTION IS WHAT MAKES THE BUDGET HONEST: `[\2 + 3\]`
/// and `[\5\]` are ONE stamp against MAX_INSTANTIATIONS only because the
/// VALUE is what gets mangled, and the value only exists here.
fn eval_static(expr: &StaticExpr, subst: &Subst, span: Span) -> Result<StaticExpr, TypeError> {
    match expr {
        StaticExpr::Int(_) | StaticExpr::Bool(_) => Ok(expr.clone()),
        StaticExpr::Ref(name) => match subst.get(name) {
            // A value parameter substituted with a value: the ordinary case.
            Some(TypeRef::Static { expr, .. }) => eval_static(expr, subst, span),
            // A value parameter substituted with a TYPE. The caller wrote a
            // type where a value was declared, and the mismatch is reported
            // where the kinds are known rather than here.
            Some(other) => Err(TypeError::TypeWhereStaticValueRequired {
                span,
                param: name.clone(),
                kind: "nat",
                written: other.written(),
            }),
            // Not in the substitution at all. At the point a stamp is cut every
            // enclosing parameter is bound, so this is a name that names
            // nothing -- which is exactly what "must be known at compile time"
            // refuses.
            None => Err(TypeError::StaticArgumentNotConstant {
                span,
                name: name.clone(),
            }),
        },
        StaticExpr::Bin { op, left, right } => {
            let (l, r) = (
                eval_static(left, subst, span)?,
                eval_static(right, subst, span)?,
            );
            match (l, r) {
                (StaticExpr::Int(a), StaticExpr::Int(b)) => {
                    // WRAPPING, for the same reason ZZ32/ZZ64 arithmetic wraps
                    // at run time -- see the 2026-08-21 arithmetic ruling. A
                    // static expression that trapped would make the SUCCESS of
                    // a compile depend on an intermediate nobody wrote down.
                    let v = match op {
                        StaticOp::Add => a.wrapping_add(b),
                        StaticOp::Sub => a.wrapping_sub(b),
                        StaticOp::Mul => a.wrapping_mul(b),
                    };
                    Ok(StaticExpr::Int(v))
                }
                _ => Err(TypeError::StaticExpressionForm {
                    span,
                    form: "arithmetic on a Boolean static argument",
                }),
            }
        }
    }
}

/// `$` cannot appear in a source identifier, so the three non-nominal forms
/// take a `$`-led name and no user type can collide with one.
fn mangle_type(t: &TypeRef) -> String {
    match t {
        TypeRef::Named { name, args, .. } => mangle_static(name, args),
        TypeRef::Unit { .. } => "$unit".to_owned(),
        // TWO SPELLINGS OF ONE TYPE MANGLE DIFFERENTLY, and that is a stated
        // cost rather than an oversight: `ZZ32[5]` and `Array[\ZZ32\]` resolve
        // to the same `Type`, so writing both as static arguments to one
        // generic stamps it twice. Two specialisations of one ground type is
        // wasteful and never wrong -- the symbols differ, so nothing collides.
        // Zero corpus files write either as a static argument.
        TypeRef::Shaped { base, extents, .. } => {
            let inner: Vec<String> = extents
                .iter()
                .map(|e| e.plain_size().map_or_else(|| "$r".to_owned(), mangle_type))
                .collect();
            format!("$arr{}{}", mangle_type(base), inner.concat())
        }
        // The VALUE, not the expression it was written as. A negative is `n`
        // rather than `-`, because `-` is not legal in a symbol.
        TypeRef::Static { expr, .. } => match expr {
            StaticExpr::Int(v) if *v < 0 => format!("$n{}", v.unsigned_abs()),
            StaticExpr::Int(v) => format!("${v}"),
            StaticExpr::Bool(v) => format!("${v}"),
            // Unreachable through `ty`, which evaluates before it mangles; a
            // name here would silently make two stamps of one instantiation.
            other => format!("$unevaluated${}", other.written()),
        },
        TypeRef::Tuple { elems, .. } => {
            let mut out = String::from("$tuple");
            for e in elems {
                out.push('$');
                out.push_str(&mangle_type(e));
            }
            out.push_str("$e");
            out
        }
        TypeRef::Arrow { from, to, .. } => {
            format!("$arrow${}${}$e", mangle_type(from), mangle_type(to))
        }
    }
}

/// Specification 1.0 `basic/overloading.tex:100-108`: two declarations of one
/// functional name may not differ in their static parameters, nor may one have
/// them and another not. Enforcing it is what makes an overload set uniformly
/// generic or uniformly ground, which is what makes monomorphizing one produce a
/// fresh disjoint set rather than adding a member to an existing one -- and
/// therefore what makes the dispatch tables built after this pass correct.
fn check_uniformity(component: &Component) -> Result<(), TypeError> {
    // EVERY DECLARATION, NOT JUST `Decl::Function`. This walked functions
    // alone, so a trait or object overload set was uniformity-checked by
    // NOTHING: `trait Holder[\T\]` beside `trait Holder` compiled to exit 0,
    // and expansion then met a set whose members disagree on how many static
    // arguments they take -- which is the one thing `expand_types` states it
    // may assume ("check_uniformity has already established that they agree
    // on how many static parameters they take").
    //
    // THE RETROACTIVE COST IS ZERO AND IT WAS MEASURED, not asserted: swept
    // over all 1956 corpus files, 397 compiling either way, 0 gained, 0 lost,
    // and 0 of the 397 emitted IR bodies changed by a single byte. No corpus
    // file writes such a set, which is exactly why fixtures carry this rule.
    let mut seen: BTreeMap<&str, (&[StaticParam], Span)> = BTreeMap::new();
    for decl in &component.decls {
        let (f_name, params, f_span) = match decl {
            Decl::Function(f) => (&f.name, f.static_params.as_slice(), f.span),
            Decl::Trait(t) => (&t.name, t.static_params.as_slice(), t.span),
            Decl::Object(o) => (&o.name, o.static_params.as_slice(), o.span),
        };
        match seen.get(f_name.as_str()) {
            None => {
                seen.insert(f_name, (params, f_span));
            }
            Some((first, first_span)) => {
                // KIND IS PART OF THE SHAPE, not just count and bound-count.
                // An overload set mixing `f[\T\]` and `f[\nat n\]` has one
                // parameter each with no bounds either side, so the length
                // comparison alone accepts it -- and then one member wants a
                // type and the other a number at the same position. That is
                // the GenFun6 coarseness class on a new axis, and it is the
                // axis this milestone created.
                let same = first.len() == params.len()
                    && first
                        .iter()
                        .zip(params)
                        .all(|(a, b)| a.bounds.len() == b.bounds.len() && a.kind == b.kind);
                if !same {
                    return Err(TypeError::OverloadSetStaticParamsDiffer {
                        span: f_span,
                        name: f_name.clone(),
                        first: *first_span,
                    });
                }
            }
        }
    }
    Ok(())
}

fn members_of(decl: &Decl) -> &[Member] {
    match decl {
        Decl::Trait(t) => &t.members,
        Decl::Object(o) => &o.members,
        Decl::Function(_) => &[],
    }
}

fn static_params(decl: &Decl) -> &[StaticParam] {
    match decl {
        Decl::Function(f) => &f.static_params,
        Decl::Trait(t) => &t.static_params,
        Decl::Object(o) => &o.static_params,
    }
}

fn decl_span(decl: &Decl) -> Span {
    match decl {
        Decl::Function(f) => f.span,
        Decl::Trait(t) => t.span,
        Decl::Object(o) => o.span,
    }
}

fn decl_name(decl: &Decl) -> &str {
    match decl {
        Decl::Function(f) => &f.name,
        Decl::Trait(t) => &t.name,
        Decl::Object(o) => &o.name,
    }
}

/// The names a declaration header may write: everything the component
/// declares, plus the builtins.
///
/// `Object` and `Any` are here because they ARE types now -- SPIKE-OBJECT-ANY-
/// REMEASURE seeded them as root traits in `Checker::new`. This pass cannot ask
/// the registry, because it runs BEFORE `Checker::new` builds one, so the two
/// names are listed rather than looked up. They come out of this list on the
/// day import resolution can supply them from `LibraryBuiltin/AnyType.fss` and
/// `CompilerBuiltin.fsi` and the seed is deleted with them.
fn declared_type_names(component: &Component) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = BUILTIN_TYPE_NAMES.iter().map(|s| (*s).to_owned()).collect();
    for decl in &component.decls {
        names.insert(decl_name(decl).to_owned());
    }
    names
}

fn check_header_type(
    t: &TypeRef,
    known: &BTreeSet<String>,
    params: &BTreeSet<String>,
) -> Result<(), TypeError> {
    match t {
        TypeRef::Named { name, args, span } => {
            if !known.contains(name) && !params.contains(name) {
                return Err(TypeError::UnknownType {
                    span: *span,
                    name: name.clone(),
                });
            }
            for a in args {
                check_header_type(a, known, params)?;
            }
            Ok(())
        }
        // The ELEMENT names a type and is checked; an extent is a static
        // argument, and whether the names in one resolve is `eval_static`'s
        // question at the substitution, exactly as for `TypeRef::Static`.
        TypeRef::Shaped { base, .. } => check_header_type(base, known, params),
        // A static VALUE names no type. Whether the names inside it resolve to
        // value parameters is `eval_static`'s question, at the substitution,
        // where the kinds are known.
        TypeRef::Static { .. } | TypeRef::Unit { .. } => Ok(()),
        TypeRef::Tuple { elems, .. } => {
            for e in elems {
                check_header_type(e, known, params)?;
            }
            Ok(())
        }
        TypeRef::Arrow { from, to, .. } => {
            check_header_type(from, known, params)?;
            check_header_type(to, known, params)
        }
    }
}

/// Resolves the type NAMES a generic declaration's header writes.
///
/// A generic declaration nothing instantiates is DELETED by `emit`, so no name
/// in its header is ever resolved by anything: `trait R[\T\] extends
/// Nowhere[\T\] end` was exit 0 while the non-generic sibling was exit 1,
/// `unknown type Nowhere`. Both resolution moments the compiler has -- `ty` for
/// an applied name, `build_hierarchy` for a bare one -- are gated on
/// instantiation.
///
/// NAMES ONLY, and that is the whole scope. No body is checked, no parameter is
/// made opaque, no bound is discharged, so this cannot refuse a program for a
/// reason that depends on a type it does not have. Members are not walked
/// either: a method's header inside a generic object may legitimately name its
/// OWNER's static parameters, which are not in scope here.
fn check_template_headers(component: &Component) -> Result<(), TypeError> {
    let known = declared_type_names(component);
    for decl in &component.decls {
        let statics = static_params(decl);
        if statics.is_empty() {
            continue;
        }
        let params: BTreeSet<String> = statics.iter().map(|p| p.name.clone()).collect();
        for p in statics {
            for b in &p.bounds {
                check_header_type(b, &known, &params)?;
            }
        }
        match decl {
            Decl::Trait(t) => {
                for list in [&t.extends, &t.comprises, &t.excludes] {
                    for r in list {
                        check_header_type(r, &known, &params)?;
                    }
                }
            }
            Decl::Object(o) => {
                for list in [&o.extends, &o.comprises, &o.excludes] {
                    for r in list {
                        check_header_type(r, &known, &params)?;
                    }
                }
                for p in o.params.iter().flatten() {
                    check_header_type(&p.ty, &known, &params)?;
                }
            }
            Decl::Function(f) => {
                for p in &f.params {
                    check_header_type(&p.ty, &known, &params)?;
                }
                if let Some(r) = &f.return_type {
                    check_header_type(r, &known, &params)?;
                }
            }
        }
    }
    Ok(())
}

/// Visit every node of a type term, outermost first.
fn walk_type(t: &TypeRef, f: &mut impl FnMut(&TypeRef)) {
    f(t);
    match t {
        TypeRef::Named { args, .. } => args.iter().for_each(|a| walk_type(a, f)),
        TypeRef::Tuple { elems, .. } => elems.iter().for_each(|e| walk_type(e, f)),
        TypeRef::Arrow { from, to, .. } => {
            walk_type(from, f);
            walk_type(to, f);
        }
        TypeRef::Shaped { base, .. } => walk_type(base, f),
        TypeRef::Unit { .. } | TypeRef::Static { .. } => {}
    }
}

/// `t` mentions one of `params` but is not simply that parameter written bare.
fn properly_contains_param(t: &TypeRef, params: &[String]) -> bool {
    if let TypeRef::Named { args, .. } = t {
        if args.is_empty() {
            return false;
        }
    }
    let mut mentions = false;
    walk_type(t, &mut |node| {
        if let TypeRef::Named { name, args, .. } = node {
            if args.is_empty() && params.iter().any(|p| p == name) {
                mentions = true;
            }
        }
    });
    mentions
}

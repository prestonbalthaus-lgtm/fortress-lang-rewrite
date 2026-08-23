//! `<| e | x <- lo:hi, guard |>`, lowered to a real `List[\T\]`.
//!
//! ROUTE 4, and the whole of it is here: an AST-to-AST pass that rewrites the
//! comprehension into an ordinary block over a minted `List[\T\]` object, and
//! splices that object's declaration in when something demanded it. It runs
//! BEFORE `mono::expand`, so `List[\ZZ64\]` is stamped like any other
//! instantiation and monomorphization does all the work.
//!
//! WHAT THE LOWERING IS:
//!
//! ```text
//! <|[\T\] e | x <- lo:hi, p |>
//!
//!   do
//!     acc$0 = List[\T\](0)
//!     i$0: ZZ64 := lo
//!     while i$0 <= hi do
//!       x = i$0
//!       if p then acc$0.append(e) end
//!       i$0 := i$0 + 1
//!     end
//!     acc$0
//!   end
//! ```
//!
//! SEQUENTIAL, AND THAT IS A NAMED DEVIATION. 1.0 defines a comprehension as a
//! `BIG` reduction, which is parallel unless every generator is `seq`. A `while`
//! is used here rather than a `for` for two reasons that are the same reason: a
//! `for` body is OUTLINED and its iterations may run on several workers, so
//! appending to one shared list would be a data race, and the ORDER of a list
//! comprehension's result is defined by the generator. Getting the parallel
//! version right needs an associative CONCAT reduction over a list monoid,
//! which is a milestone and not a lowering.
//!
//! THE ELEMENT TYPE IS WRITTEN, NEVER INFERRED, exactly as a static argument is
//! everywhere else in this compiler. It comes from the comprehension's own
//! `[\T\]` -- which is 1.0's spelling, `parser_tests/XXXPreparser.ad.fss`
//! writes `<|[\ZZ32\] x | x <- xset` -- or from the written type of the binding
//! it initialises. Neither, and it is refused by name.

use fortress_ast::{
    Assign, Binding, BlockItem, Component, Decl, Expr, GeneratorClause, Member, Span, TypeRef,
};

use crate::error::TypeError;

/// The minted collection. Parsed, never linked: only its declarations are
/// spliced, and only into a component that used a comprehension.
const LIST_SOURCE: &str = include_str!("List.fss");

/// The bracket pair a LIST comprehension is written with, as
/// `enclosing_application` names it.
const LIST_BRACKET: &str = "<|_|>";

const LIST: &str = "List";

pub fn lower(component: &Component) -> Result<Component, TypeError> {
    let mut pass = Pass {
        counter: 0,
        demanded: false,
    };
    let mut out = component.clone();
    for decl in &mut out.decls {
        pass.decl(decl)?;
    }
    if pass.demanded {
        let already = out.decls.iter().any(|d| named(d) == Some(LIST));
        if already {
            return Err(TypeError::ComprehensionListTaken {
                span: component.span,
            });
        }
        out.decls.extend(minted_list());
    }
    Ok(out)
}

fn named(decl: &Decl) -> Option<&str> {
    match decl {
        Decl::Trait(t) => Some(t.name.as_str()),
        Decl::Object(o) => Some(o.name.as_str()),
        Decl::Function(f) => Some(f.name.as_str()),
        Decl::Value(v) => Some(v.name.as_str()),
    }
}

/// `List.fss`'s declarations. Parsed once per component that needs them; the
/// file is a `component` so it reads as ordinary Fortress rather than as a
/// fragment with no home.
fn minted_list() -> Vec<Decl> {
    let tokens = match fortress_lexer::lex(LIST_SOURCE) {
        Ok(t) => t,
        // Unreachable: the source is a constant in this crate, and
        // `list_source_parses` is the test that says so.
        Err(_) => return Vec::new(),
    };
    match fortress_parser::parse(&tokens) {
        Ok(c) => c.decls,
        Err(_) => Vec::new(),
    }
}

struct Pass {
    counter: usize,
    demanded: bool,
}

impl Pass {
    fn decl(&mut self, decl: &mut Decl) -> Result<(), TypeError> {
        match decl {
            Decl::Function(f) => {
                let wanted = f.return_type.clone();
                if let Some(body) = &mut f.body {
                    self.expr(body, wanted.as_ref())?;
                }
            }
            Decl::Value(v) => {
                if let Some(init) = &mut v.init {
                    let wanted = v.ty.clone();
                    self.expr(init, wanted.as_ref())?;
                }
            }
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
                let wanted = m.return_type.clone();
                if let Some(body) = &mut m.body {
                    self.expr(body, wanted.as_ref())?;
                }
            }
            Member::Field(f) => {
                if let Some(init) = &mut f.init {
                    let wanted = f.ty.clone();
                    self.expr(init, Some(&wanted))?;
                }
            }
            Member::Coercion { .. } => {}
        }
        Ok(())
    }

    /// `wanted` is the written type of the slot this expression initialises,
    /// and it is consulted for ONE thing: a comprehension's element type.
    fn expr(&mut self, e: &mut Expr, wanted: Option<&TypeRef>) -> Result<(), TypeError> {
        if matches!(e, Expr::Comprehension { .. }) {
            return self.comprehension(e, wanted);
        }
        self.children(e)
    }

    /// EXHAUSTIVE ON PURPOSE. A catch-all here would swallow the next `Expr`
    /// variant silently and leave a comprehension inside it un-lowered, which
    /// arrives as `a comprehension parses and its lowering is not implemented`
    /// on a program that writes an ordinary one. E0004 is the instrument.
    fn children(&mut self, e: &mut Expr) -> Result<(), TypeError> {
        match e {
            Expr::Unit { .. }
            | Expr::IntLit { .. }
            | Expr::FloatLit { .. }
            | Expr::StrLit { .. }
            | Expr::CharLit { .. }
            | Expr::BoolLit { .. }
            | Expr::Var { .. }
            | Expr::Exit { value: None, .. } => {}
            Expr::Comprehension { .. } => self.comprehension(e, None)?,
            Expr::Tuple { items, .. } | Expr::Juxt { items, .. } => {
                for item in items {
                    self.expr(item, None)?;
                }
            }
            Expr::ArrayLit { items, .. } => {
                for item in items {
                    self.expr(item, None)?;
                }
            }
            Expr::Infix { lhs, rhs, .. } => {
                self.expr(lhs, None)?;
                self.expr(rhs, None)?;
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
            } => self.expr(inner, None)?,
            Expr::Call { callee, args, .. } => {
                self.expr(callee, None)?;
                for arg in args {
                    self.expr(arg, None)?;
                }
            }
            Expr::Index { base, indices, .. } => {
                self.expr(base, None)?;
                for index in indices {
                    self.expr(index, None)?;
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.expr(cond, None)?;
                self.expr(then_branch, None)?;
                if let Some(otherwise) = else_branch {
                    self.expr(otherwise, None)?;
                }
            }
            Expr::While { cond, body, .. } => {
                self.expr(cond, None)?;
                self.expr(body, None)?;
            }
            Expr::BindingCondition {
                source,
                body,
                otherwise,
                ..
            } => {
                self.expr(source, None)?;
                self.expr(body, None)?;
                if let Some(o) = otherwise {
                    self.expr(o, None)?;
                }
            }
            Expr::Block { items, .. } => {
                for item in items {
                    match item {
                        BlockItem::Binding(b) => {
                            let wanted = b.ty.clone();
                            self.expr(&mut b.value, wanted.as_ref())?;
                        }
                        BlockItem::TupleBinding(b) => self.expr(&mut b.value, None)?,
                        BlockItem::Assign(a) => {
                            self.expr(&mut a.target, None)?;
                            self.expr(&mut a.value, None)?;
                        }
                        BlockItem::Expr(x) => self.expr(x, None)?,
                    }
                }
            }
            Expr::ObjectExpr { members, .. } => {
                for member in members {
                    self.member(member)?;
                }
            }
            Expr::Try {
                body,
                arms,
                finally,
                ..
            } => {
                self.expr(body, None)?;
                for arm in arms {
                    self.expr(&mut arm.body, None)?;
                }
                if let Some(f) = finally {
                    self.expr(f, None)?;
                }
            }
            Expr::For { lo, hi, body, .. } | Expr::BigReduction { lo, hi, body, .. } => {
                self.expr(lo, None)?;
                self.expr(hi, None)?;
                self.expr(body, None)?;
            }
            Expr::ForIn { source, body, .. } => {
                self.expr(source, None)?;
                self.expr(body, None)?;
            }
            Expr::AlsoDo { blocks, .. } => {
                for block in blocks {
                    self.expr(block, None)?;
                }
            }
            Expr::Case {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.expr(subject, None)?;
                for arm in arms {
                    self.expr(&mut arm.guard, None)?;
                    self.expr(&mut arm.body, None)?;
                }
                if let Some(otherwise) = else_arm {
                    self.expr(otherwise, None)?;
                }
            }
            Expr::TypeCase {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.expr(subject, None)?;
                for arm in arms {
                    self.expr(&mut arm.body, None)?;
                }
                self.expr(else_arm, None)?;
            }
        }
        Ok(())
    }

    fn comprehension(&mut self, e: &mut Expr, wanted: Option<&TypeRef>) -> Result<(), TypeError> {
        let Expr::Comprehension {
            bracket,
            static_args,
            body,
            gens,
            span,
        } = e
        else {
            return Ok(());
        };
        let span = *span;
        if bracket != LIST_BRACKET {
            return Err(TypeError::ComprehensionUnsupported {
                span,
                bracket: bracket.clone(),
            });
        }
        let element = match (static_args.first(), wanted.and_then(list_element)) {
            (Some(written), _) => written.clone(),
            (None, Some(slot)) => slot,
            (None, None) => return Err(TypeError::ComprehensionElementUnwritten { span }),
        };
        let mut body = (**body).clone();
        self.expr(&mut body, None)?;
        let mut gens = std::mem::take(gens);
        for g in &mut gens {
            self.expr(&mut g.init, None)?;
            if let Some(h) = &mut g.hi {
                self.expr(h, None)?;
            }
        }

        let index = self.counter;
        self.counter = self.counter.saturating_add(1);
        self.demanded = true;
        let acc = format!("acc${index}");

        // Innermost first: each clause wraps what the ones to its right built.
        let mut inner = call(field(var(&acc, span), "append", span), vec![body], span);
        for (depth, clause) in gens.iter().enumerate().rev() {
            inner = self.clause(clause, inner, index, depth, &acc, span)?;
        }

        *e = Expr::Block {
            items: vec![
                BlockItem::Binding(Binding {
                    name: acc.clone(),
                    ty: None,
                    value: call(
                        Expr::Instantiate {
                            callee: Box::new(var(LIST, span)),
                            args: vec![element],
                            span,
                        },
                        vec![zero(span)],
                        span,
                    ),
                    mutable: false,
                    span,
                }),
                BlockItem::Expr(inner),
                BlockItem::Expr(var(&acc, span)),
            ],
            span,
        };
        Ok(())
    }

    /// One generator clause, wrapped around what the clauses to its right
    /// built. A clause with no binder is a GUARD.
    fn clause(
        &mut self,
        clause: &GeneratorClause,
        inner: Expr,
        index: usize,
        depth: usize,
        acc: &str,
        span: Span,
    ) -> Result<Expr, TypeError> {
        if clause.binders.is_empty() {
            let _ = acc;
            return Ok(Expr::If {
                cond: Box::new(clause.init.clone()),
                then_branch: Box::new(inner),
                else_branch: None,
                span,
            });
        }
        let [binder] = clause.binders.as_slice() else {
            return Err(TypeError::ComprehensionGeneratorUnsupported {
                span: clause.span,
                form: "a generator binding more than one name",
            });
        };
        let Some(hi) = clause.hi.clone() else {
            return Err(TypeError::ComprehensionGeneratorUnsupported {
                span: clause.span,
                form: "a generator over a collection rather than a range",
            });
        };
        let counter = format!("i${index}${depth}");
        // `lo:hi` runs while `i <= hi`; `lo#n` runs `n` times from `lo`.
        let bound = if clause.inclusive {
            infix_le(var(&counter, span), hi, span)
        } else {
            infix_lt(
                var(&counter, span),
                infix_add(clause.init.clone(), hi, span),
                span,
            )
        };
        Ok(Expr::Block {
            items: vec![
                BlockItem::Binding(Binding {
                    name: counter.clone(),
                    ty: Some(zz64(span)),
                    value: clause.init.clone(),
                    mutable: true,
                    span,
                }),
                BlockItem::Expr(Expr::While {
                    cond: Box::new(bound),
                    body: Box::new(Expr::Block {
                        items: vec![
                            BlockItem::Binding(Binding {
                                name: (*binder).clone(),
                                ty: None,
                                value: var(&counter, span),
                                mutable: false,
                                span,
                            }),
                            BlockItem::Expr(inner),
                            BlockItem::Assign(Assign {
                                target: var(&counter, span),
                                op: None,
                                value: infix_add(var(&counter, span), one(span), span),
                                span,
                            }),
                        ],
                        span,
                    }),
                    span,
                }),
            ],
            span,
        })
    }
}

/// `List[\T\]` written in a slot, and nothing else: the element type a
/// comprehension takes from the binding it initialises.
fn list_element(wanted: &TypeRef) -> Option<TypeRef> {
    match wanted {
        TypeRef::Named { name, args, .. } if name == LIST && args.len() == 1 => {
            args.first().cloned()
        }
        _ => None,
    }
}

// --------------------------------------------------------------- AST helpers

fn var(name: &str, span: Span) -> Expr {
    Expr::Var {
        name: name.to_owned(),
        span,
    }
}

fn field(base: Expr, name: &str, span: Span) -> Expr {
    Expr::Field {
        base: Box::new(base),
        name: name.to_owned(),
        span,
    }
}

fn call(callee: Expr, args: Vec<Expr>, span: Span) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args,
        span,
    }
}

fn zz64(span: Span) -> TypeRef {
    TypeRef::Named {
        name: "ZZ64".to_owned(),
        args: Vec::new(),
        span,
    }
}

fn int(text: &str, span: Span) -> Expr {
    Expr::IntLit {
        digits: text.to_owned(),
        span,
    }
}

fn zero(span: Span) -> Expr {
    int("0", span)
}

fn one(span: Span) -> Expr {
    int("1", span)
}

fn infix(op: fortress_ast::BinOp, lhs: Expr, rhs: Expr, span: Span) -> Expr {
    Expr::Infix {
        op,
        fixity: fortress_ast::Fixity::Loose,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span,
    }
}

fn infix_le(left: Expr, right: Expr, span: Span) -> Expr {
    infix(fortress_ast::BinOp::Le, left, right, span)
}

fn infix_lt(left: Expr, right: Expr, span: Span) -> Expr {
    infix(fortress_ast::BinOp::Lt, left, right, span)
}

fn infix_add(left: Expr, right: Expr, span: Span) -> Expr {
    infix(fortress_ast::BinOp::Add, left, right, span)
}

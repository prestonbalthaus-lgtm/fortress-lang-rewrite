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
mod types;

pub use error::TypeError;
pub use types::{
    ArithOp, AssignTarget, CompareOp, Elem, MpiOp, Target, Type, TypedBlockItem, TypedComponent,
    TypedExpr, TypedExprKind, TypedFn, TypedParam, ARRAY_ALLOC, ARRAY_LENGTH, ARRAY_SLOT,
};

use std::collections::HashMap;

use fortress_ast::{Assign, BinOp, BlockItem, Component, Decl, Expr, FnDecl, Span, TypeRef, UnOp};

/// What a name in scope is: its type, and whether it can be assigned to.
#[derive(Debug, Clone, Copy)]
struct Local {
    ty: Type,
    mutable: bool,
}

type Checked<T> = Result<T, TypeError>;

pub fn check(component: &Component) -> Checked<TypedComponent> {
    Checker::new(component)?.run(component)
}

struct Signature {
    params: Vec<Type>,
    returns: Type,
}

struct Checker {
    functions: HashMap<String, Signature>,
    scopes: Vec<HashMap<String, Local>>,
    uses_mpi: bool,
}

impl Checker {
    /// Pass one: every signature, so recursion and forward references resolve.
    fn new(component: &Component) -> Checked<Self> {
        let mut functions: HashMap<String, Signature> = HashMap::new();
        for decl in &component.decls {
            let Decl::Function(f) = decl;
            let params = f
                .params
                .iter()
                .map(|p| resolve_type(&p.ty))
                .collect::<Checked<Vec<Type>>>()?;
            let returns = match &f.return_type {
                Some(t) => resolve_type(t)?,
                // Inferred in pass two; Void until then, and overwritten there.
                None => Type::Void,
            };
            if functions
                .insert(f.name.clone(), Signature { params, returns })
                .is_some()
            {
                return Err(TypeError::DuplicateDefinition {
                    span: f.span,
                    name: f.name.clone(),
                });
            }
        }
        Ok(Self {
            functions,
            scopes: Vec::new(),
            uses_mpi: false,
        })
    }

    fn run(mut self, component: &Component) -> Checked<TypedComponent> {
        let mut functions = Vec::new();
        for decl in &component.decls {
            let Decl::Function(f) = decl;
            functions.push(self.function(f)?);
        }
        Ok(TypedComponent {
            name: component.name.clone(),
            exports: component.exports.clone(),
            functions,
            uses_mpi: self.uses_mpi,
        })
    }

    fn function(&mut self, f: &FnDecl) -> Checked<TypedFn> {
        let declared = f.return_type.as_ref().map(resolve_type).transpose()?;

        let mut params = Vec::new();
        let mut scope = HashMap::new();
        for p in &f.params {
            let ty = resolve_type(&p.ty)?;
            scope.insert(p.name.clone(), Local { ty, mutable: false });
            params.push(TypedParam {
                name: p.name.clone(),
                ty,
                span: p.span,
            });
        }

        self.scopes.push(scope);
        let body = self.expr(&f.body, declared);
        self.scopes.pop();
        let body = body?;

        let return_type = declared.unwrap_or(body.ty);
        if let Some(sig) = self.functions.get_mut(&f.name) {
            sig.returns = return_type;
        }
        Ok(TypedFn {
            name: f.name.clone(),
            params,
            return_type,
            body,
            span: f.span,
        })
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
                require(typed.ty, expected, *span)?;
                Ok(typed)
            }
            Expr::StrLit { value, span } => {
                require(Type::String, expected, *span)?;
                Ok(TypedExpr {
                    kind: TypedExprKind::StrConst(value.clone()),
                    ty: Type::String,
                    span: *span,
                })
            }
            Expr::BoolLit { value, span } => {
                require(Type::Boolean, expected, *span)?;
                Ok(TypedExpr {
                    kind: TypedExprKind::BoolConst(*value),
                    ty: Type::Boolean,
                    span: *span,
                })
            }
            Expr::Var { name, span } => {
                let ty = self
                    .lookup(name)
                    .ok_or_else(|| TypeError::UnknownName {
                        span: *span,
                        name: name.clone(),
                    })?
                    .ty;
                require(ty, expected, *span)?;
                Ok(TypedExpr {
                    kind: TypedExprKind::Var(name.clone()),
                    ty,
                    span: *span,
                })
            }
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
        }
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
        require(ty, expected, span)?;
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
        require(ty, expected, span)?;
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
        require(Type::Void, expected, span)?;
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
                    target: AssignTarget::Element { base, index, elem },
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
            kind: TypedExprKind::IntConst(value),
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
        }
    }

    fn infix(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
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
        if !left.ty.is_numeric() {
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
        };
        require(ty, expected, span)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Apply {
                target,
                args: vec![left, right],
            },
            ty,
            span,
        })
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
        require(ty, expected, span)?;
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
        require(Type::String, expected, span)?;
        Ok(folded)
    }

    fn call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Checked<TypedExpr> {
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
        match name.as_str() {
            "widen" => self.widen(args, span, expected),
            "println" => self.println(args, span, expected),
            "array" => self.array_new(args, span, expected),
            "length" => self.array_length(args, span, expected),
            _ => {
                let Some(sig) = self.functions.get(name) else {
                    return Err(TypeError::UnknownName {
                        span: *callee_span,
                        name: name.clone(),
                    });
                };
                let (params, returns) = (sig.params.clone(), sig.returns);
                if params.len() != args.len() {
                    return Err(TypeError::ArityMismatch {
                        span,
                        name: name.clone(),
                        expected: params.len(),
                        found: args.len(),
                    });
                }
                let mut typed = Vec::with_capacity(args.len());
                for (arg, want) in args.iter().zip(params) {
                    typed.push(self.expr(arg, Some(want))?);
                }
                require(returns, expected, span)?;
                Ok(TypedExpr {
                    kind: TypedExprKind::Apply {
                        target: Target::UserFn { name: name.clone() },
                        args: typed,
                    },
                    ty: returns,
                    span,
                })
            }
        }
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
        require(ty, expected, span)?;
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
        require(Type::ZZ64, expected, span)?;
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
        require(Type::ZZ64, expected, span)?;
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
        require(Type::Void, expected, span)?;
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
        if then_typed.ty != else_typed.ty {
            return Err(TypeError::BranchTypeMismatch {
                span,
                then_type: then_typed.ty,
                else_type: else_typed.ty,
            });
        }
        let ty = then_typed.ty;
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
                    let declared = b.ty.as_ref().map(resolve_type).transpose()?;
                    let value = self.expr(&b.value, declared)?;
                    let ty = declared.unwrap_or(value.ty);
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

        require(Type::Void, expected, span)?;
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

/// Checks a computed type against its context. This is where the
/// no-implicit-widening rule is enforced, and it never converts anything.
fn require(found: Type, expected: Option<Type>, span: Span) -> Checked<()> {
    match expected {
        None => Ok(()),
        Some(want) if want == found => Ok(()),
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

const fn is_int_literal(e: &Expr) -> bool {
    matches!(e, Expr::IntLit { .. })
}

fn resolve_type(t: &TypeRef) -> Checked<Type> {
    if t.name == "Array" {
        let Some(argument) = &t.argument else {
            return Err(TypeError::UnsupportedElementType {
                span: t.span,
                name: "Array".to_owned(),
            });
        };
        let inner = resolve_type(argument)?;
        return Elem::of(inner)
            .map(Type::Array)
            .ok_or_else(|| TypeError::UnsupportedElementType {
                span: argument.span,
                name: inner.name().to_owned(),
            });
    }
    match t.name.as_str() {
        "ZZ32" => Ok(Type::ZZ32),
        "ZZ64" => Ok(Type::ZZ64),
        "RR64" => Ok(Type::RR64),
        "Boolean" => Ok(Type::Boolean),
        "String" => Ok(Type::String),
        _ => Err(TypeError::UnknownType {
            span: t.span,
            name: t.name.clone(),
        }),
    }
}

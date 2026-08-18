// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules; an
// integration test is its own crate, so the workspace denies apply here.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use fortress_types::{check, Type, TypeError, TypedComponent, TypedExpr, TypedExprKind};

fn typed(src: &str) -> TypedComponent {
    let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex: {e}"));
    let ast = fortress_parser::parse(&tokens).unwrap_or_else(|e| panic!("parse: {e}"));
    check(&ast).unwrap_or_else(|e| panic!("typecheck failed: {e}\nsource:\n{src}"))
}

fn type_error(src: &str) -> TypeError {
    let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex: {e}"));
    let ast = fortress_parser::parse(&tokens).unwrap_or_else(|e| panic!("parse: {e}"));
    match check(&ast) {
        Ok(_) => panic!("expected a type error from:\n{src}"),
        Err(e) => e,
    }
}

/// Wraps a function body so single expressions can be checked in isolation.
fn body(decl: &str) -> TypedExpr {
    let src = format!("component t\n{decl}\nend\n");
    let c = typed(&src);
    c.functions
        .into_iter()
        .next()
        .map(|f| f.body)
        .unwrap_or_else(|| panic!("no function"))
}

fn body_error(decl: &str) -> TypeError {
    type_error(&format!("component t\n{decl}\nend\n"))
}

fn target_of(e: &TypedExpr) -> Option<String> {
    match &e.kind {
        TypedExprKind::Apply { target, .. } => Some(target.symbol()),
        _ => None,
    }
}

// -------------------------------------- literals are unfixed until pinned

#[test]
fn a_literal_pins_to_the_type_its_slot_requires() {
    let e = body("f(x:ZZ64):ZZ64 = x + 1");
    assert_eq!(e.ty, Type::ZZ64);
    assert_eq!(target_of(&e).as_deref(), Some("add_zz64_zz64"));
}

#[test]
fn the_same_literal_pins_to_zz32_in_a_zz32_slot() {
    let e = body("f(x:ZZ32):ZZ32 = x + 1");
    assert_eq!(e.ty, Type::ZZ32);
    assert_eq!(target_of(&e).as_deref(), Some("add_zz32_zz32"));
}

#[test]
fn an_unpinned_literal_defaults_to_zz32() {
    let e = body("f():ZZ32 = 1 + 2");
    assert_eq!(e.ty, Type::ZZ32);
    assert_eq!(target_of(&e).as_deref(), Some("add_zz32_zz32"));
}

#[test]
fn a_literal_on_the_left_still_pins_from_the_right() {
    let e = body("f(x:ZZ64):ZZ64 = 1 + x");
    assert_eq!(target_of(&e).as_deref(), Some("add_zz64_zz64"));
}

#[test]
fn a_literal_too_large_for_its_pinned_type_is_rejected() {
    assert!(matches!(
        body_error("f():ZZ32 = 99999999999999"),
        TypeError::LiteralOutOfRange { ty: Type::ZZ32, .. }
    ));
    // The same digits are fine once the slot is ZZ64.
    let e = body("f():ZZ64 = 99999999999999");
    assert_eq!(e.ty, Type::ZZ64);
}

// ------------------------------- values are never implicitly converted

#[test]
fn a_zz32_variable_in_a_zz64_slot_is_rejected() {
    // The rule, stated as its own error so the diagnostic can name the fix.
    match body_error("g(y:ZZ64):ZZ64 = y\nf(x:ZZ32):ZZ64 = g(x)") {
        TypeError::ImplicitWideningRejected { from, to, .. } => {
            assert_eq!(from, Type::ZZ32);
            assert_eq!(to, Type::ZZ64);
        }
        other => panic!("expected implicit widening to be rejected, got {other}"),
    }
}

#[test]
fn a_zz32_variable_returned_from_a_zz64_function_is_rejected() {
    assert!(matches!(
        body_error("f(x:ZZ32):ZZ64 = x"),
        TypeError::ImplicitWideningRejected {
            from: Type::ZZ32,
            to: Type::ZZ64,
            ..
        }
    ));
}

#[test]
fn mixing_two_numeric_variables_of_different_types_is_rejected() {
    assert!(matches!(
        body_error("f(a:ZZ32, b:ZZ64):ZZ64 = a + b"),
        TypeError::ImplicitWideningRejected { .. } | TypeError::MixedNumericOperands { .. }
    ));
}

#[test]
fn widen_is_the_only_way_across_and_it_is_explicit() {
    let e = body("f(x:ZZ32):ZZ64 = widen(x)");
    assert_eq!(e.ty, Type::ZZ64);
    assert_eq!(target_of(&e).as_deref(), Some("widen_zz32_zz64"));
}

#[test]
fn a_literal_is_not_a_value_and_the_two_rules_do_not_collide() {
    // `1` in a ZZ64 slot is fine. A ZZ32 *variable* holding 1 is not. This is
    // the distinction the whole design turns on.
    assert_eq!(body("f():ZZ64 = 1").ty, Type::ZZ64);
    assert!(matches!(
        body_error("f(one:ZZ32):ZZ64 = one"),
        TypeError::ImplicitWideningRejected { .. }
    ));
}

// --------------------------------------------------- juxtaposition folding

#[test]
fn numeric_juxtaposition_folds_to_multiplication() {
    let e = body("f(x:ZZ64, y:ZZ64):ZZ64 = x y");
    assert_eq!(e.ty, Type::ZZ64);
    assert_eq!(target_of(&e).as_deref(), Some("mul_zz64_zz64"));
}

#[test]
fn string_juxtaposition_folds_to_concatenation() {
    let e = body("f(s:String):String = s s");
    assert_eq!(e.ty, Type::String);
    assert_eq!(target_of(&e).as_deref(), Some("concat_string_string"));
}

#[test]
fn a_string_juxtaposed_with_a_number_stringifies_the_number() {
    let e = body("f(n:ZZ64):String = \"n = \" n");
    assert_eq!(e.ty, Type::String);
    assert_eq!(target_of(&e).as_deref(), Some("concat_string_string"));

    // and the numeric operand acquired an explicit conversion target
    let TypedExprKind::Apply { args, .. } = &e.kind else {
        panic!("expected an apply")
    };
    let converted = args.get(1).and_then(target_of);
    assert_eq!(converted.as_deref(), Some("to_string_zz64"));
}

#[test]
fn juxtaposing_two_different_numeric_types_does_not_resolve() {
    match body_error("f(a:ZZ32, b:ZZ64):ZZ64 = a b") {
        TypeError::MixedNumericOperands { left, right, .. } => {
            assert_eq!(left, Type::ZZ32);
            assert_eq!(right, Type::ZZ64);
        }
        other => panic!("expected mixed numeric operands, got {other}"),
    }
}

#[test]
fn juxtaposing_a_boolean_does_not_resolve() {
    assert!(matches!(
        body_error("f(a:Boolean, b:Boolean):Boolean = a b"),
        TypeError::UnresolvableJuxtaposition { .. }
    ));
}

// ------------------------------------------------------ static resolution

#[test]
fn comparisons_resolve_on_operand_type_and_yield_boolean() {
    let e = body("f(x:ZZ64):Boolean = x < 2");
    assert_eq!(e.ty, Type::Boolean);
    assert_eq!(target_of(&e).as_deref(), Some("lt_zz64_zz64"));
}

#[test]
fn a_user_call_resolves_to_the_declared_function() {
    let c = typed("component t\ng(x:ZZ64):ZZ64 = x\nf(x:ZZ64):ZZ64 = g(x)\nend\n");
    let f = c.functions.iter().find(|f| f.name == "f").expect("f");
    assert_eq!(target_of(&f.body).as_deref(), Some("g"));
}

#[test]
fn every_apply_in_the_acceptance_program_names_a_concrete_target() {
    let c = typed(ACCEPTANCE);
    let mut targets = Vec::new();
    for f in &c.functions {
        collect_targets(&f.body, &mut targets);
    }
    assert!(!targets.is_empty());
    for t in &targets {
        assert!(!t.is_empty(), "an unresolved target survived: {targets:?}");
    }
    // Codegen must never have to ask which `+` or which juxtaposition this is.
    for expect in [
        "lt_zz64_zz64",
        "sub_zz64_zz64",
        "mul_zz64_zz64",
        "widen_zz32_zz64",
        "f",
    ] {
        assert!(
            targets.iter().any(|t| t == expect),
            "missing {expect} in {targets:?}"
        );
    }
}

fn collect_targets(e: &TypedExpr, out: &mut Vec<String>) {
    match &e.kind {
        TypedExprKind::Apply { target, args } => {
            out.push(target.symbol());
            for a in args {
                collect_targets(a, out);
            }
        }
        TypedExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_targets(cond, out);
            collect_targets(then_branch, out);
            if let Some(e) = else_branch {
                collect_targets(e, out);
            }
        }
        TypedExprKind::Block { items, tail } => {
            for item in items {
                match item {
                    fortress_types::TypedBlockItem::Binding { value, .. } => {
                        collect_targets(value, out);
                    }
                    fortress_types::TypedBlockItem::Expr(e) => collect_targets(e, out),
                }
            }
            if let Some(t) = tail {
                collect_targets(t, out);
            }
        }
        _ => {}
    }
}

// -------------------------------------------------------------- the program

const ACCEPTANCE: &str = concat!(
    "component fact\n",
    "export Executable\n",
    "\n",
    "f(x:ZZ64):ZZ64 = if x < 2 then 1 else x f(x-1) end\n",
    "\n",
    "run() = do\n",
    "   j:ZZ64 = widen(20)\n",
    "   println(\"fact(20) = \" f(j))\n",
    "end\n",
    "end\n",
);

#[test]
fn the_m1_acceptance_program_typechecks() {
    let c = typed(ACCEPTANCE);
    assert_eq!(c.name, "fact");
    assert_eq!(c.functions.len(), 2);

    let f = c.functions.first().expect("f");
    assert_eq!(f.name, "f");
    assert_eq!(f.return_type, Type::ZZ64);
    assert_eq!(f.params.first().map(|p| p.ty), Some(Type::ZZ64));
    assert_eq!(f.body.ty, Type::ZZ64, "the if must produce ZZ64");

    let run = c.functions.get(1).expect("run");
    assert_eq!(run.name, "run");
    assert_eq!(run.return_type, Type::Void, "run ends in println");
}

#[test]
fn the_recursive_multiplication_is_resolved_to_zz64() {
    let c = typed(ACCEPTANCE);
    let f = c.functions.first().expect("f");
    let TypedExprKind::If {
        else_branch: Some(else_branch),
        ..
    } = &f.body.kind
    else {
        panic!("expected an if with an else")
    };
    // `x f(x-1)` folded from a juxtaposition into a concrete multiplication.
    assert_eq!(target_of(else_branch).as_deref(), Some("mul_zz64_zz64"));
    assert_eq!(else_branch.ty, Type::ZZ64);
}

#[test]
fn the_println_argument_became_a_concatenation() {
    let c = typed(ACCEPTANCE);
    let run = c.functions.get(1).expect("run");
    let TypedExprKind::Block {
        tail: Some(tail), ..
    } = &run.body.kind
    else {
        panic!("run should be a block with a tail expression")
    };
    assert_eq!(target_of(tail).as_deref(), Some("println_string"));
    let TypedExprKind::Apply { args, .. } = &tail.kind else {
        panic!("expected apply")
    };
    assert_eq!(args.first().map(|a| a.ty), Some(Type::String));
    assert_eq!(
        args.first().and_then(target_of).as_deref(),
        Some("concat_string_string")
    );
}

#[test]
fn widen_in_the_acceptance_program_pins_its_literal_to_zz32() {
    let c = typed(ACCEPTANCE);
    let run = c.functions.get(1).expect("run");
    let TypedExprKind::Block { items, .. } = &run.body.kind else {
        panic!("expected a block")
    };
    let Some(fortress_types::TypedBlockItem::Binding { ty, value, .. }) = items.first() else {
        panic!("expected the j binding")
    };
    assert_eq!(*ty, Type::ZZ64);
    assert_eq!(target_of(value).as_deref(), Some("widen_zz32_zz64"));
    let TypedExprKind::Apply { args, .. } = &value.kind else {
        panic!("expected apply")
    };
    assert_eq!(
        args.first().map(|a| a.ty),
        Some(Type::ZZ32),
        "20 pins to ZZ32 inside widen"
    );
}

// -------------------------------------------------------- the MPI builtins

#[test]
fn mpi_comm_rank_resolves_to_the_prefixed_shim_and_returns_zz32() {
    let e = body("f():ZZ32 = mpiCommRank()");
    assert_eq!(e.ty, Type::ZZ32);
    assert_eq!(target_of(&e).as_deref(), Some("fortress_mpi_comm_rank"));
}

#[test]
fn mpi_comm_size_resolves_to_the_prefixed_shim_and_returns_zz32() {
    let e = body("f():ZZ32 = mpiCommSize()");
    assert_eq!(e.ty, Type::ZZ32);
    assert_eq!(target_of(&e).as_deref(), Some("fortress_mpi_comm_size"));
}

#[test]
fn mpi_init_is_void_and_names_the_prefixed_shim() {
    let e = body("f() = mpiInit()");
    assert_eq!(e.ty, Type::Void);
    assert_eq!(target_of(&e).as_deref(), Some("fortress_mpi_init"));
}

#[test]
fn mpi_finalize_is_void_and_names_the_prefixed_shim() {
    let e = body("f() = mpiFinalize()");
    assert_eq!(e.ty, Type::Void);
    assert_eq!(target_of(&e).as_deref(), Some("fortress_mpi_finalize"));
}

/// The prefix is the point: `MPI_Comm_rank` is a real symbol in libmpi and a
/// Fortress function called `mpiCommRank` must never collide with it.
#[test]
fn no_mpi_target_emits_a_bare_mpi_symbol() {
    for decl in [
        "f() = mpiInit()",
        "f():ZZ32 = mpiCommRank()",
        "f():ZZ32 = mpiCommSize()",
        "f() = mpiFinalize()",
    ] {
        let symbol = target_of(&body(decl)).unwrap_or_default();
        assert!(
            symbol.starts_with("fortress_mpi_"),
            "{decl} emitted `{symbol}`"
        );
    }
}

#[test]
fn an_mpi_builtin_takes_no_arguments() {
    let e = body_error("f():ZZ32 = mpiCommRank(1)");
    assert!(
        matches!(
            e,
            TypeError::ArityMismatch {
                ref name,
                expected: 0,
                found: 1,
                ..
            } if name == "mpiCommRank"
        ),
        "expected an arity error, got {e}"
    );
}

#[test]
fn an_mpi_rank_is_not_implicitly_a_zz64() {
    let e = body_error("f():ZZ64 = mpiCommRank()");
    assert!(
        matches!(e, TypeError::ImplicitWideningRejected { .. }),
        "the no-implicit-conversion rule applies to builtins too, got {e}"
    );
}

#[test]
fn a_component_that_never_calls_mpi_does_not_claim_to_use_it() {
    let c = typed("component t\nrun() = println(\"hi\")\nend\n");
    assert!(!c.uses_mpi);
}

#[test]
fn one_mpi_call_anywhere_marks_the_whole_component() {
    let c = typed("component t\nhelper():ZZ32 = mpiCommSize()\nrun() = println(helper())\nend\n");
    assert!(
        c.uses_mpi,
        "uses_mpi drives whether the driver links the MPI shim"
    );
}

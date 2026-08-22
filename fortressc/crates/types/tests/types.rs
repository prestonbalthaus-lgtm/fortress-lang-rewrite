// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules; an
// integration test is its own crate, so the workspace denies apply here.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use fortress_types::{
    check, intern_types, Type, TypeError, TypedComponent, TypedExpr, TypedExprKind,
};

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
                    fortress_types::TypedBlockItem::TupleBinding { parts, .. } => {
                        for part in parts {
                            collect_targets(&part.value, out);
                        }
                    }
                    fortress_types::TypedBlockItem::Assign { value, .. } => {
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

// ------------------------------------------------ M3b: arrays and iteration

#[test]
fn an_array_literal_takes_its_element_type_from_its_slot() {
    let e = body("f():Array[\\ZZ64\\] = [1, 2, 3]");
    assert_eq!(e.ty, Type::Array(fortress_types::Elem::ZZ64, 1));
}

#[test]
fn an_unpinned_array_literal_defaults_to_zz32_like_a_bare_literal() {
    let c = typed("component t\nf() = do\n   a = [1, 2]\n   println(a[0])\nend\nend\n");
    let f = c.functions.first().expect("f");
    let TypedExprKind::Block { items, .. } = &f.body.kind else {
        panic!("expected a block")
    };
    let Some(fortress_types::TypedBlockItem::Binding { ty, .. }) = items.first() else {
        panic!("expected the a binding")
    };
    assert_eq!(*ty, Type::Array(fortress_types::Elem::ZZ32, 1));
}

#[test]
fn indexing_an_array_yields_its_element_type() {
    let e = body("f(a:Array[\\ZZ64\\]):ZZ64 = a[0]");
    assert_eq!(e.ty, Type::ZZ64);
}

#[test]
fn length_of_an_array_is_a_zz64() {
    let e = body("f(a:Array[\\ZZ32\\]):ZZ64 = length(a)");
    assert_eq!(e.ty, Type::ZZ64);
    assert_eq!(target_of(&e).as_deref(), Some("fortress_array_length"));
}

#[test]
fn length_of_something_that_is_not_an_array_is_rejected() {
    let e = body_error("f(x:ZZ64):ZZ64 = length(x)");
    assert!(
        matches!(e, TypeError::NotAnArray { .. }),
        "expected a not-an-array error, got {e}"
    );
}

#[test]
fn indexing_something_that_is_not_an_array_is_rejected() {
    let e = body_error("f(x:ZZ64):ZZ64 = x[0]");
    assert!(
        matches!(e, TypeError::NotAnArray { .. }),
        "expected a not-an-array error, got {e}"
    );
}

/// The no-implicit-conversion rule holds at the new boundary too: subscripts
/// are ZZ64, and a ZZ32 variable in that slot needs `widen`.
#[test]
fn a_zz32_subscript_is_not_implicitly_widened() {
    let e = body_error("f(a:Array[\\ZZ64\\], i:ZZ32):ZZ64 = a[i]");
    assert!(
        matches!(e, TypeError::ImplicitWideningRejected { .. }),
        "expected the widening diagnostic, got {e}"
    );
}

#[test]
fn a_sized_array_takes_its_element_type_from_the_binding() {
    let c = typed(
        "component t\nf() = do\n   a:Array[\\ZZ64\\] = array(8)\n   println(a[0])\nend\nend\n",
    );
    let f = c.functions.first().expect("f");
    let TypedExprKind::Block { items, .. } = &f.body.kind else {
        panic!("expected a block")
    };
    let Some(fortress_types::TypedBlockItem::Binding { ty, .. }) = items.first() else {
        panic!("expected the a binding")
    };
    assert_eq!(*ty, Type::Array(fortress_types::Elem::ZZ64, 1));
}

#[test]
fn a_sized_array_with_no_context_cannot_pick_an_element_type() {
    let e = body_error("f() = do\n   a = array(8)\n   println(length(a))\nend");
    assert!(
        matches!(e, TypeError::ElementTypeUnknown { .. }),
        "expected an unknown-element-type error, got {e}"
    );
}

#[test]
fn an_array_of_arrays_is_refused_rather_than_half_supported() {
    let e = body_error("f(a:Array[\\Array[\\ZZ64\\]\\]):ZZ64 = 0");
    assert!(
        matches!(e, TypeError::UnsupportedElementType { .. }),
        "expected an unsupported-element error, got {e}"
    );
}

#[test]
fn assigning_to_an_immutable_binding_is_rejected() {
    let e = body_error("f() = do\n   i:ZZ64 = 0\n   i := 1\nend");
    assert!(
        matches!(e, TypeError::AssignToImmutable { .. }),
        "expected an immutability error, got {e}"
    );
}

#[test]
fn assigning_to_an_undeclared_name_names_the_fix() {
    let e = body_error("f() = do\n   i := 1\nend");
    assert!(
        matches!(e, TypeError::AssignToUndeclared { .. }),
        "expected an undeclared-name error, got {e}"
    );
    assert!(
        e.to_string().contains(":="),
        "the diagnostic should show the declaration form:\n{e}"
    );
}

#[test]
fn a_mutable_binding_can_be_assigned() {
    let c = typed("component t\nf() = do\n   i:ZZ64 := 0\n   i := 1\nend\nend\n");
    assert_eq!(c.functions.len(), 1);
}

/// The binding is immutable, the container is not. `a` cannot be rebound; its
/// elements can be written.
#[test]
fn elements_of_an_immutable_array_binding_can_still_be_assigned() {
    let c =
        typed("component t\nf() = do\n   a:Array[\\ZZ64\\] = array(4)\n   a[0] := 7\nend\nend\n");
    assert_eq!(c.functions.len(), 1);
}

#[test]
fn assigning_the_wrong_type_to_an_element_is_rejected() {
    let e = body_error("f() = do\n   a:Array[\\ZZ64\\] = array(4)\n   a[0] := true\nend");
    assert!(
        matches!(e, TypeError::Mismatch { .. }),
        "expected a mismatch, got {e}"
    );
}

#[test]
fn a_while_condition_must_be_boolean() {
    let e = body_error("f() = do\n   while 1 do\n      println(\"x\")\n   end\nend");
    assert!(
        matches!(e, TypeError::ConditionNotBoolean { .. }),
        "expected a condition error, got {e}"
    );
}

#[test]
fn a_while_loop_is_void() {
    let e = body("f() = do\n   i:ZZ64 := 0\n   while i < 3 do\n      i := i + 1\n   end\nend");
    assert_eq!(e.ty, Type::Void);
}

#[test]
fn a_function_call_is_not_an_assignment_target() {
    let e = body_error("f() = do\n   println(1) := 2\nend");
    assert!(
        matches!(e, TypeError::InvalidAssignTarget { .. }),
        "expected an invalid-target error, got {e}"
    );
}

// ------------------------------------ M3c: traits, objects and dispatch

/// A program with a hierarchy in front of it, so the tests below say only what
/// they are about.
fn with_shapes(rest: &str) -> String {
    format!(
        "component t\n\
         trait Ink end\n\
         object Solid extends {{Ink}} end\n\
         object Dotted extends {{Ink}} end\n\
         {rest}\n\
         end\n"
    )
}

fn last_target(src: &str) -> String {
    let c = typed(src);
    let f = c.functions.last().expect("a function");
    target_of(&f.body).unwrap_or_else(|| panic!("the body is not a call: {:?}", f.body))
}

#[test]
fn an_object_is_a_subtype_of_the_trait_it_extends() {
    let c = typed(&with_shapes("ink(): Ink = Solid"));
    let f = c.functions.last().expect("a function");
    assert_eq!(f.return_type, Type::Trait("Ink"));
    // The value keeps its concrete type; only the slot is the trait.
    assert_eq!(f.body.ty, Type::Object("Solid"));
}

#[test]
fn a_lone_declaration_still_gets_its_bare_symbol_and_a_direct_call() {
    // The whole pre-M3c language is this case, and it has to be unchanged.
    let c = typed(&with_shapes(
        "one(x: Ink): ZZ32 = 1\nrun() = println(one(Solid))",
    ));
    assert_eq!(
        c.functions.first().map(|f| f.name.as_str()),
        Some("one"),
        "an overload set of one is not mangled"
    );
    assert!(c.dispatches.is_empty(), "nothing to decide at run time");
}

#[test]
fn concrete_arguments_never_reach_a_switch() {
    let src =
        with_shapes("name(x: Solid): ZZ32 = 1\nname(x: Ink): ZZ32 = 2\nrun(): ZZ32 = name(Solid)");
    assert_eq!(last_target(&src), "name$Solid");
    assert!(typed(&src).dispatches.is_empty());
}

/// The case a naive reading gets wrong: at the static type `Ink` only
/// `name(Ink)` is applicable, but the cell `(Solid)` has a different winner, so
/// the call has to dispatch rather than bind statically to `name(Ink)`.
#[test]
fn a_trait_typed_argument_dispatches_even_when_one_declaration_applies_statically() {
    let src = with_shapes(
        "name(x: Solid): ZZ32 = 1\n\
         name(x: Ink): ZZ32 = 2\n\
         pick(n: ZZ32): Ink = if n === 0 then Solid else Dotted end\n\
         run(): ZZ32 = name(pick(0))",
    );
    assert_eq!(last_target(&src), "name$dispatch$Ink");

    let c = typed(&src);
    let d = c.dispatches.first().expect("a dispatch function");
    assert_eq!(d.params, vec![Type::Trait("Ink")]);
    assert_eq!(d.returns, Type::ZZ32);
    match &d.tree {
        fortress_types::DispatchNode::Switch { position, arms } => {
            assert_eq!(*position, 0);
            assert_eq!(arms.len(), 2, "one arm per concrete type under Ink");
        }
        other => panic!("expected a switch, got {other:?}"),
    }
}

#[test]
fn a_row_whose_winners_agree_collapses_instead_of_switching() {
    // Both concrete types under Ink land on the same declaration, so there is
    // nothing left to decide and the table is a leaf.
    let src = with_shapes(
        "name(x: Ink): ZZ32 = 2\n\
         pick(n: ZZ32): Ink = if n === 0 then Solid else Dotted end\n\
         run(): ZZ32 = name(pick(0))",
    );
    assert_eq!(last_target(&src), "name");
    assert!(typed(&src).dispatches.is_empty());
}

#[test]
fn a_symmetrically_ambiguous_call_is_refused_and_names_both_declarations() {
    let src = "component t\n\
        trait Top end\n\
        trait Left extends {Top} end\n\
        trait Right extends {Top} end\n\
        object OLeft extends {Left} end\n\
        object ORight extends {Right} end\n\
        pick(x: Top, y: Top): ZZ32 = 0\n\
        pick(x: Left, y: Top): ZZ32 = 1\n\
        pick(x: Top, y: Right): ZZ32 = 2\n\
        topOf(n: ZZ32): Top = if n === 0 then OLeft else ORight end\n\
        run(): ZZ32 = pick(topOf(0), topOf(1))\n\
        end\n";
    let e = type_error(src);
    let TypeError::AmbiguousDispatch {
        arguments,
        first,
        second,
        ..
    } = &e
    else {
        panic!("expected an ambiguity, got {e}")
    };
    assert_eq!(arguments, "OLeft, ORight");
    assert_ne!(first, second, "two different declarations must be named");
}

#[test]
fn a_call_no_declaration_covers_is_refused_before_the_table_is_built() {
    // Every cell has a winner, but nothing applies to (Ink) itself, so there is
    // no statically computed return type for the covariance check to use.
    let e = body_error(
        "trait Ink end\n\
         object Solid extends {Ink} end\n\
         object Dotted extends {Ink} end\n\
         name(x: Solid): ZZ32 = 1\n\
         name(x: Dotted): ZZ32 = 2\n\
         pick(n: ZZ32): Ink = if n === 0 then Solid else Dotted end\n\
         run(): ZZ32 = name(pick(0))",
    );
    assert!(
        matches!(e, TypeError::NoApplicableDeclaration { .. }),
        "got {e}"
    );
}

#[test]
fn two_overloads_with_inferred_returns_keep_their_own_types() {
    let c = typed(&with_shapes(
        "size(x: Solid) = 7\nsize(x: Dotted) = \"wide\"",
    ));
    let mut returns: Vec<(String, Type)> = c
        .functions
        .iter()
        .map(|f| (f.name.clone(), f.return_type))
        .collect();
    returns.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        returns,
        vec![
            ("size$Dotted".to_owned(), Type::String),
            ("size$Solid".to_owned(), Type::ZZ32),
        ]
    );
}

#[test]
fn a_field_reads_at_its_declared_type() {
    let e = body("object Box(width: ZZ32) end\nrun(): ZZ32 = Box(3).width");
    assert_eq!(e.ty, Type::ZZ32);
    assert!(matches!(e.kind, TypedExprKind::Field { index: 0, .. }));
}

#[test]
fn a_field_that_does_not_exist_is_a_diagnostic() {
    let e = body_error("object Box(width: ZZ32) end\nrun(): ZZ32 = Box(3).height");
    assert!(matches!(e, TypeError::UnknownField { .. }), "got {e}");
}

#[test]
fn a_dotted_method_is_not_the_function_of_the_same_name() {
    let e = body_error(
        "object Box(width: ZZ32) end\nwidth(b: Box): ZZ32 = 1\nrun(): ZZ32 = Box(3).width(0)",
    );
    assert!(
        matches!(e, TypeError::DottedMethodUnsupported { .. }),
        "got {e}"
    );
}

#[test]
fn a_singleton_is_a_value_and_not_a_constructor() {
    let e = body_error("object Solid end\nrun(): ZZ32 = do Solid(1)\n 1 end");
    assert!(
        matches!(e, TypeError::SingletonNotConstructible { .. }),
        "got {e}"
    );
}

#[test]
fn a_field_initializer_may_not_reach_a_singleton() {
    // Initializers run when the object is built, which for a singleton is
    // before `run`. Reaching another one would make declaration order load
    // bearing, and a forward reference a null dereference.
    let e = body_error(
        "object Unit end\nobject Box(w: ZZ32) other: Unit = Unit end\nrun(): ZZ32 = Box(1).w",
    );
    assert!(
        matches!(e, TypeError::SingletonInitializerRestricted { .. }),
        "got {e}"
    );
}

#[test]
fn a_mutable_field_is_storage_and_an_immutable_one_is_not() {
    let source = "component t\nobject Box(w: ZZ32) var seen: ZZ32 = 0 end\n\
                  run(): ZZ32 = do b = Box(1)\n b.seen := 7\n b.seen end\nend\n";
    let component = typed(source);
    let boxed = component
        .objects
        .iter()
        .find(|o| o.name == "Box")
        .expect("Box is registered");
    let named = |n: &str| {
        boxed
            .fields
            .iter()
            .find(|f| f.name == n)
            .unwrap_or_else(|| panic!("no field `{n}`"))
    };
    assert!(!named("w").mutable, "a constructor parameter is not");
    assert!(named("seen").mutable, "`var seen` is");

    let e = body_error("object Box(w: ZZ32) end\nrun(): ZZ32 = do b = Box(1)\n b.w := 2\n b.w end");
    assert!(
        matches!(e, TypeError::FieldIsImmutable { .. }),
        "an immutable field is not an assignment target: got {e}"
    );
}

#[test]
fn an_abstract_member_may_name_an_unrepresentable_type_but_not_a_missing_one() {
    // An abstract member is the one place in this compiler where a type is
    // WRITTEN and READ BY NOTHING, so anything at all used to be accepted
    // there -- in silence, at exit 0. That is how the closure pass came to
    // need its own guard against `Foo -> ZZ32` with no `Foo`.
    for source in [
        "trait T
 m(x: Foo): ZZ32
end
run(): ZZ32 = 1",
        "trait T
 m(x: ZZ32): Foo
end
run(): ZZ32 = 1",
        "trait T
 m(self, x: Foo): ZZ32
end
run(): ZZ32 = 1",
    ] {
        match body_error(source) {
            TypeError::UnknownType { name, .. } => assert_eq!(name, "Foo"),
            other => panic!("expected UnknownType for `{source}`, got {other:?}"),
        }
    }

    // And the line that keeps it honest: a type this compiler cannot
    // REPRESENT is our limitation, not the program's. `tupleTypeParam2.fss`
    // instantiates a trait at a tuple, so the instance's abstract member
    // cannot be typed here -- and nothing calls it, because the object that
    // extends it declares its own concrete one. It compiles and prints 7.
    let component = typed(
        "component t
trait T
 m(x: (ZZ32, ZZ32)): ZZ32
end
run(): ZZ32 = 1
end
",
    );
    assert_eq!(component.functions.len(), 1, "only `run` is emitted");
}

#[test]
fn a_trait_that_extends_itself_is_a_diagnostic_rather_than_a_hang() {
    let e = body_error("trait A extends {B} end\ntrait B extends {A} end\nrun(): ZZ32 = 1");
    assert!(matches!(e, TypeError::TraitCycle { .. }), "got {e}");
}

#[test]
fn an_object_cannot_extend_an_object() {
    let e = body_error("object A end\nobject B extends {A} end\nrun(): ZZ32 = 1");
    assert!(matches!(e, TypeError::NotATrait { .. }), "got {e}");
}

/// `SPIKE-API-CHECK-MODE`. An api used to be refused as the FIRST statement of
/// `run`, before `discharge_bounds` and before anything was resolved -- a stub
/// sitting exactly where phase 3 goes. It is CHECKED now: headers resolved,
/// bounds discharged, no bodies, nothing emitted.
#[test]
fn an_api_is_checked_rather_than_refused() {
    let c = typed("api t\ntrait A end\nf(x: ZZ32): A\nend\n");
    assert!(c.is_api);
    assert!(c.functions.is_empty(), "signatures are not code");
    assert!(c.objects.is_empty());
}

/// And it is a REAL check, not an accept-everything path: the header types have
/// to resolve. This is what the api census measures.
#[test]
fn an_api_whose_header_does_not_resolve_is_refused() {
    let e = type_error("api t\nf(x: NoSuchType): ZZ32\nend\n");
    assert!(matches!(e, TypeError::UnknownType { .. }), "got {e}");
    let e = type_error("api t\ntrait A extends {Nowhere} end\nend\n");
    assert!(matches!(e, TypeError::UnknownType { .. }), "got {e}");
}

/// `source-code.tex:313-320`: an api holds DECLARATIONS. A declaration with a
/// body is a definition, and the diagnostic names the one that broke the rule
/// rather than the whole file.
#[test]
fn an_api_declaration_may_not_have_a_body() {
    let e = type_error("api t\nf(x: ZZ32): ZZ32 = x\nend\n");
    match e {
        TypeError::ApiDeclarationHasBody { name, .. } => assert_eq!(name, "f"),
        other => panic!("got {other}"),
    }
    let e = type_error("api t\ntrait A\n  g(): ZZ32 = 1\nend\nend\n");
    match e {
        TypeError::ApiDeclarationHasBody { name, .. } => assert_eq!(name, "A.g"),
        other => panic!("got {other}"),
    }
}

#[test]
fn println_refuses_what_there_is_no_shim_for() {
    let e = body_error("run() = do a:Array[\\ZZ64\\] = array(2)\n println(a) end");
    assert!(matches!(e, TypeError::NotPrintable { .. }), "got {e}");
}

#[test]
fn a_branch_of_each_concrete_type_is_allowed_under_a_shared_trait() {
    let c = typed(&with_shapes(
        "pick(n: ZZ32): Ink = if n === 0 then Solid else Dotted end",
    ));
    assert_eq!(
        c.functions.last().map(|f| f.return_type),
        Some(Type::Trait("Ink"))
    );
}

#[test]
fn objects_are_tagged_from_one_so_that_zero_is_never_valid() {
    let c = typed(&with_shapes("run(): ZZ32 = 1"));
    let tags: Vec<u32> = c.objects.iter().map(|o| o.tag).collect();
    assert_eq!(tags, vec![1, 2]);
}

#[test]
fn a_cell_may_not_return_something_the_call_site_cannot_hold() {
    // The static type `Ink` picks `name(Ink)` and with it a ZZ32 result, but
    // the cell (Solid) would return a String through the same signature.
    let e = body_error(
        "trait Ink end\n\
         object Solid extends {Ink} end\n\
         object Dotted extends {Ink} end\n\
         name(x: Ink): ZZ32 = 1\n\
         name(x: Solid): String = \"a\"\n\
         pick(n: ZZ32): Ink = if n === 0 then Solid else Dotted end\n\
         run(): ZZ32 = name(pick(0))",
    );
    assert!(
        matches!(e, TypeError::ReturnTypeNotCovariant { .. }),
        "got {e}"
    );
}

#[test]
fn a_table_that_would_not_fit_is_a_diagnostic_rather_than_a_hang() {
    // 101 concrete types across three dispatched positions is 1,030,301 cells.
    // The bound is checked on the product, before anything is enumerated.
    let objects: String = (0..101)
        .map(|i| format!("object O{i} extends {{Big}} end\n"))
        .collect();
    let e = body_error(&format!(
        "trait Big end\n{objects}\
         f(a: Big, b: Big, c: Big): ZZ32 = 1\n\
         f(a: O0, b: Big, c: Big): ZZ32 = 2\n\
         g(x: Big): ZZ32 = f(x, x, x)"
    ));
    match e {
        TypeError::DispatchTableTooLarge { cells, .. } => assert_eq!(cells, 101 * 101 * 101),
        other => panic!("got {other}"),
    }
}

#[test]
fn a_lone_declaration_is_never_sized_or_enumerated() {
    // The same hierarchy with one declaration is a direct call, so the bound
    // above must not fire on it.
    let objects: String = (0..101)
        .map(|i| format!("object O{i} extends {{Big}} end\n"))
        .collect();
    let src = format!(
        "component t\ntrait Big end\n{objects}\
         f(a: Big, b: Big, c: Big): ZZ32 = 1\n\
         g(x: Big): ZZ32 = f(x, x, x)\nend\n"
    );
    assert_eq!(last_target(&src), "f");
    assert!(typed(&src).dispatches.is_empty());
}

// ------------------------------------ M3d: generics by monomorphization

#[test]
fn an_instantiation_is_a_distinct_concrete_type_per_argument() {
    let c = typed(
        "component t\n\
         object Cell[\\T\\](held: T) end\n\
         run() = do\n\
           a: Cell[\\ZZ64\\] = Cell[\\ZZ64\\](1)\n\
           b: Cell[\\String\\] = Cell[\\String\\](\"x\")\n\
           println(a.held)\n\
         end\n\
         end\n",
    );
    let names: Vec<&str> = c.objects.iter().map(|o| o.name).collect();
    assert_eq!(names, vec!["Cell$String$e", "Cell$ZZ64$e"]);
    // Distinct tags: they are different types, not one erased type.
    let tags: Vec<u32> = c.objects.iter().map(|o| o.tag).collect();
    assert_eq!(tags, vec![1, 2]);
}

#[test]
fn the_stored_field_keeps_its_own_type_rather_than_being_boxed() {
    let c = typed(
        "component t\n\
         object Cell[\\T\\](held: T) end\n\
         run() = do\n\
           a: Cell[\\ZZ64\\] = Cell[\\ZZ64\\](1)\n\
           println(a.held)\n\
         end\n\
         end\n",
    );
    let cell = c.objects.first().expect("an instantiation");
    assert_eq!(cell.fields.first().map(|f| f.ty), Some(Type::ZZ64));
}

#[test]
fn a_generic_named_without_static_arguments_is_refused() {
    let e = body_error("object Cell[\\T\\](held: T) end\nrun(): ZZ32 = do Cell\n 1 end");
    assert!(
        matches!(e, TypeError::StaticArgumentsRequired { .. }),
        "got {e}"
    );
}

#[test]
fn static_arguments_are_counted() {
    let e = body_error(
        "object Pair[\\A, B\\](a: A, b: B) end\n\
         run(): ZZ32 = do Pair[\\ZZ32\\](1, 2)\n 1 end",
    );
    assert!(
        matches!(e, TypeError::StaticArgumentCountMismatch { .. }),
        "got {e}"
    );
}

/// Specification 1.0's uniformity rule, enforced. It is what makes an overload
/// set uniformly generic or uniformly ground, and therefore what makes
/// monomorphizing one produce a fresh disjoint set instead of adding a member
/// to an existing one.
#[test]
fn an_overload_set_may_not_mix_generic_and_ground_declarations() {
    let e = body_error("size[\\T\\](x: T): ZZ32 = 1\nsize(x: ZZ32): ZZ32 = 2\nrun(): ZZ32 = 0");
    assert!(
        matches!(e, TypeError::OverloadSetStaticParamsDiffer { .. }),
        "got {e}"
    );
}

#[test]
fn a_bound_is_discharged_after_the_registry_exists() {
    let ok = typed(
        "component t\n\
         trait Ink end\n\
         object Solid extends {Ink} end\n\
         object Pen[\\T extends Ink\\](tip: T) end\n\
         run() = do p: Pen[\\Solid\\] = Pen[\\Solid\\](Solid)\n println(1) end\n\
         end\n",
    );
    assert!(ok.objects.iter().any(|o| o.name == "Pen$Solid$e"));

    let e = body_error(
        "trait Ink end\n\
         object Plain end\n\
         object Pen[\\T extends Ink\\](tip: T) end\n\
         run() = do p: Pen[\\Plain\\] = Pen[\\Plain\\](Plain)\n println(1) end",
    );
    assert!(matches!(e, TypeError::BoundNotSatisfied { .. }), "got {e}");
}

/// The named casualty. No monomorphizing compiler compiles this at any limit,
/// so it has to be refused rather than hung on.
#[test]
fn polymorphic_recursion_stops_at_the_ceiling() {
    let e = body_error(
        "object Wrap[\\T\\](held: T) end\n\
         deeper[\\T\\](x: T): ZZ32 = deeper[\\Wrap[\\T\\]\\](Wrap[\\T\\](x))\n\
         run(): ZZ32 = deeper[\\ZZ64\\](1)",
    );
    match e {
        TypeError::TooManyInstantiations { limit, .. } => {
            assert_eq!(limit, fortress_types::MAX_INSTANTIATIONS);
        }
        other => panic!("got {other}"),
    }
}

/// An F-bound is not polymorphic recursion and must not be caught by the same
/// net: `Equality`, `Integral` and `List` are all F-bounded, and rejecting them
/// would reject the prelude.
#[test]
fn an_f_bound_converges_in_one_step() {
    let c = typed(
        "component t\n\
         trait Same[\\T\\] end\n\
         object Num extends {Same[\\Num\\]} end\n\
         object Holder[\\T extends Same[\\T\\]\\](x: T) end\n\
         run() = do h: Holder[\\Num\\] = Holder[\\Num\\](Num)\n println(1) end\n\
         end\n",
    );
    assert!(c.objects.iter().any(|o| o.name == "Holder$Num$e"));
}

#[test]
fn a_generic_instantiation_reaches_the_dispatch_table() {
    let c = typed(
        "component t\n\
         trait Shape end\n\
         object Box[\\T\\](held: T) extends {Shape} end\n\
         object Dot extends {Shape} end\n\
         area(s: Shape): ZZ32 = 1\n\
         area(s: Box[\\ZZ64\\]): ZZ32 = 3\n\
         pick(n: ZZ32): Shape = if n === 0 then Dot else Box[\\ZZ64\\](1) end\n\
         run(): ZZ32 = area(pick(0))\n\
         end\n",
    );
    let d = c
        .dispatches
        .first()
        .expect("the trait-typed call must dispatch");
    match &d.tree {
        fortress_types::DispatchNode::Switch { arms, .. } => assert_eq!(
            arms.len(),
            2,
            "an arm for Dot and one for the instantiation of Box"
        ),
        other => panic!("expected a switch, got {other:?}"),
    }
}

/// `array(n)` hands back slots nothing wrote, and the runtime's fill is a
/// one-byte empty string. A reference element would give dispatch a tag load
/// four bytes into it.
#[test]
fn an_array_of_objects_cannot_be_made_uninitialised() {
    let e = body_error("object Node(k: ZZ32) end\nrun() = do a:Array[\\Node\\] = array(4)\n end");
    assert!(
        matches!(e, TypeError::UnsupportedElementType { .. }),
        "got {e}"
    );
}

#[test]
fn mangling_distinguishes_nesting_from_arity() {
    use fortress_ast::{Span, TypeRef};
    let bare = |n: &str| TypeRef::Named {
        name: n.to_owned(),
        args: Vec::new(),
        span: Span::new(0, 0),
    };
    let nested = TypeRef::Named {
        name: "List".to_owned(),
        args: vec![bare("B")],
        span: Span::new(0, 0),
    };
    // Foo[\List[\B\]\] and Foo[\List, B\] must not collide.
    assert_ne!(
        fortress_types::mangle_static("Foo", &[nested]),
        fortress_types::mangle_static("Foo", &[bare("List"), bare("B")])
    );
}

// ------------------------------------------------------- void is not storable

#[test]
fn a_void_valued_binding_is_a_diagnostic_not_an_internal_error() {
    match body_error("f(): ZZ32 = do\n  x = println(\"hi\")\n  0\nend") {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "a binding"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}

#[test]
fn a_binding_of_a_void_while_is_refused_too() {
    match body_error(
        "f(): ZZ32 = do\n  y: ZZ32 := 0\n  x = while y < 0 do y := y + 1 end\n  0\nend",
    ) {
        TypeError::VoidNotStorable { .. } => {}
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}

#[test]
fn the_unit_type_resolves_to_void() {
    let c = typed("component t\nf(): () = println(\"hi\")\nend\n");
    assert_eq!(c.functions.first().map(|f| f.return_type), Some(Type::Void));
}

#[test]
fn a_unit_parameter_is_refused() {
    match type_error("component t\nf(x: ()): ZZ32 = 1\nend\n") {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "a parameter"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}

#[test]
fn a_unit_field_is_refused() {
    match type_error("component t\nobject O(x: ()) end\nf(): ZZ32 = 1\nend\n") {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "a field"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}

#[test]
fn a_unit_array_element_is_refused() {
    match type_error("component t\nf(): ZZ32 = do\n  a: Array[\\()\\] = array(1)\n  1\nend\nend\n")
    {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "an array element"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}

#[test]
fn a_tuple_result_on_a_function_with_a_body_is_refused() {
    // THE REFUSAL MOVED AND THE REASON CHANGED. A tuple TYPE resolves now;
    // what is refused is a tuple in a position a DEFINED function would have
    // to lower, because there is no representation yet. An `api` names one
    // freely -- see the test below.
    match type_error("component t\nf(): (ZZ32, String) = 1\nend\n") {
        TypeError::TupleNotStorable { position, .. } => assert_eq!(position, "the result"),
        other => panic!("expected TupleNotStorable, got {other:?}"),
    }
}

#[test]
fn an_arrow_over_liftable_types_becomes_a_trait_and_the_rest_stay_refused() {
    // SPIKE-CLOSURE-REPRESENTATION lifts an arrow whose sides are types this
    // subset can store: `ZZ32 -> String` is a trait now, so a literal in that
    // slot is an ordinary type error rather than "an arrow type".
    let e = body_error("f(): ZZ32 -> String = 1");
    assert!(
        matches!(
            e,
            TypeError::Mismatch { .. } | TypeError::LiteralNotApplicable { .. }
        ),
        "got {e}"
    );

    // AN ARROW OVER A NAME THAT DOES NOT EXIST NAMES THE NAME. Leaving the
    // arrow unlifted handed it to a diagnostic about arrows -- and inside an
    // ABSTRACT member, to no diagnostic at all, because an unliftable arrow is
    // `TypeNotImplemented`, which an abstract member is allowed to keep.
    for source in [
        "f(g: Foo -> ZZ32): ZZ32 = 1",
        "trait T\n m(g: Foo -> ZZ32): ZZ32\nend\nrun(): ZZ32 = 1",
    ] {
        match body_error(source) {
            TypeError::UnknownType { name, .. } => assert_eq!(name, "Foo"),
            other => panic!("expected UnknownType for `{source}`, got {other:?}"),
        }
    }

    // A TRAIT'S FIELD TYPE IS RESOLVED even though the field itself carries no
    // storage. `trait T  x: Foo  end` compiled to exit 0 in silence otherwise.
    match body_error("trait T\n x: Foo = 1\nend\nrun(): ZZ32 = 1") {
        TypeError::UnknownType { name, .. } => assert_eq!(name, "Foo"),
        other => panic!("expected UnknownType for a trait field, got {other:?}"),
    }

    // A TUPLE DOMAIN IS LIFTABLE NOW and is not in this list any more.
    // `basic/overloading.tex:125` -- a functional has a single parameter WHICH
    // MAY BE A TUPLE -- so `(A,B) -> C` is `apply(x: A, y: B): C` and needs no
    // tuple VALUE at all. `Compiled17d.fss` writes `combine: (R,R) -> R`.
    // It COMPILES: the arrow becomes a minted trait, so this is an ordinary
    // function with an ordinary parameter and an ordinary body.
    {
        let src = "component t\nf(g: (ZZ32, ZZ32) -> ZZ32): ZZ32 = 1\nend\n";
        let tokens = fortress_lexer::lex(src).expect("lex");
        let ast = fortress_parser::parse(&tokens).expect("parse");
        assert!(check(&ast).is_ok(), "a tuple domain lifts");
    }

    // What is NOT liftable keeps the diagnostic it had, because `apply` would
    // need a parameter this subset cannot store -- or, for an undeclared name,
    // because an abstract member's parameter types are resolved by nothing and
    // it would compile in silence.
    //
    // A `()` CODOMAIN IS STILL REFUSED, AND THAT IS MEASURED. Allowing it
    // gained ZERO files -- `Zeepf.fsi:16`'s `foo: String -> ()` is refused a
    // second time further along -- and ACCEPTED
    // `ProjectFortress/parser_tests/XXXArrowType.fss`, an XXX must-fail, whose
    // `f: ZZ32 -> () -> ()` starts qualifying the moment the inner `() -> ()`
    // does.
    for source in ["f(g: ZZ32 -> ()): ZZ32 = 1"] {
        match body_error(source) {
            TypeError::TypeNotImplemented { form, .. } => assert_eq!(form, "an arrow type"),
            other => panic!("expected TypeNotImplemented for `{source}`, got {other:?}"),
        }
    }
}

#[test]
fn the_unit_expression_has_type_void() {
    let b = body("f(): () = ()");
    assert_eq!(b.ty, Type::Void);
    assert!(matches!(b.kind, TypedExprKind::Unit), "got {:?}", b.kind);
}

#[test]
fn a_unit_binding_is_refused() {
    match body_error("f(): ZZ32 = do\n  x: () = ()\n  0\nend") {
        TypeError::VoidNotStorable { position, .. } => assert_eq!(position, "a binding"),
        other => panic!("expected VoidNotStorable, got {other:?}"),
    }
}

#[test]
fn a_tuple_expression_is_refused_with_a_diagnostic() {
    match body_error("f(): ZZ32 = do\n  x = (1, 2)\n  0\nend") {
        TypeError::TypeNotImplemented { form, .. } => assert_eq!(form, "a tuple expression"),
        other => panic!("expected TypeNotImplemented, got {other:?}"),
    }
}

#[test]
fn an_entry_point_with_parameters_is_refused() {
    // Codegen's generated `main` calls `run` with no arguments, so this was
    // "LLVM rejected the generated module", exit 70, on two corpus files.
    match type_error("component t\nrun(args: String): () = ()\nend\n") {
        TypeError::EntryPointTakesArguments { found, .. } => assert_eq!(found, 1),
        other => panic!("expected EntryPointTakesArguments, got {other:?}"),
    }
}

// ------------------------------- M3j: functional and generic methods

/// The name exists; the lifting does not. `unknown name` about a declaration
/// written three lines up sends the file into the wrong blocker bucket, which
/// is how milestones get chosen off the wrong histogram.
#[test]
fn a_generic_functional_method_is_refused_by_its_own_name() {
    let source = "component t\n\
                  object O\n  foo[\\T\\](self, x: T): T = x\nend\n\
                  run(): () = do println(foo[\\ZZ32\\](O, 17)) end\n\
                  end\n";
    match type_error(source) {
        TypeError::GenericFunctionalMethodUnsupported { name, .. } => assert_eq!(name, "foo"),
        other => panic!("expected GenericFunctionalMethodUnsupported, got {other:?}"),
    }
}

/// A functional method and a top-level declaration of the same name are ONE
/// overload set, so declaring the same signature twice is a duplicate rather
/// than two namespaces quietly holding one each.
#[test]
fn a_functional_method_collides_with_an_identical_top_level_declaration() {
    let source = "component t\n\
                  object O\n  f(self): ZZ32 = 1\nend\n\
                  f(x: O): ZZ32 = 2\n\
                  run(): () = ()\n\
                  end\n";
    match type_error(source) {
        TypeError::DuplicateOverload {
            name, arguments, ..
        } => {
            assert_eq!(name, "f");
            // The types are asserted too: with overloading the NAME is shared
            // by design, so the name alone does not say what collided.
            assert_eq!(arguments, "O");
        }
        other => panic!("expected DuplicateOverload, got {other:?}"),
    }
}

/// `self` is lifted where it was written. A declaration that puts it second is
/// a different signature from one that puts it first, and both are members of
/// the same set.
#[test]
fn a_functional_method_keeps_the_position_self_was_written_in() {
    let source = "component t\n\
                  object O\n  f(k: ZZ32, self): ZZ32 = k\nend\n\
                  run(): () = do println(f(2, O)) end\n\
                  end\n";
    let c = typed(source);
    assert!(
        c.functions.iter().any(|f| f.name == "O$f$f"),
        "self written second must lift to (ZZ32, O): {:?}",
        c.functions
            .iter()
            .map(|f| f.name.clone())
            .collect::<Vec<_>>()
    );
}

// ----------------------------- M3k: operators and builtins

/// The operand is named against the OPERATOR. Reporting it as an `if` whose
/// condition is wrong is what desugaring in the parser would have produced,
/// and it is how files land in the wrong blocker bucket.
#[test]
fn a_non_boolean_and_operand_is_named_against_the_operator() {
    match body_error("f(): Boolean = do\n  n: ZZ32 = 1\n  n AND true\nend") {
        TypeError::LogicalOperandNotBoolean { op, found, .. } => {
            assert_eq!(op, "AND");
            assert_eq!(found, Type::ZZ32);
        }
        other => panic!("expected LogicalOperandNotBoolean, got {other:?}"),
    }
}

/// Equality is defined on Boolean; ordering is not, and inventing one would be
/// a silently wrong answer rather than a missing feature.
#[test]
fn ordering_on_boolean_is_refused_and_equality_is_not() {
    match body_error("f(): Boolean = true < false") {
        TypeError::BooleanNotOrdered { op, .. } => assert_eq!(op, "<"),
        other => panic!("expected BooleanNotOrdered, got {other:?}"),
    }
    let b = body("f(): Boolean = true = false");
    assert_eq!(b.ty, Type::Boolean);
}

/// `^` is the one operator whose operands may disagree: 1.0 declares it on
/// every base-exponent pair, and the result is real if either side is.
#[test]
fn exponentiation_takes_a_mixed_pair_and_widens_the_result() {
    let b = body("f(): RR64 = do\n  x: RR64 = 2.0\n  n: ZZ32 = 3\n  x^n\nend");
    assert_eq!(b.ty, Type::RR64);
    let i = body("f(): ZZ32 = do\n  n: ZZ32 = 3\n  n^n\nend");
    assert_eq!(i.ty, Type::ZZ32);
}

/// An assert is an `if` over `=`, so it can only compare what `=` compares.
#[test]
fn assert_is_only_as_strong_as_equality_is() {
    match body_error("f(): () = assert(\"a\", \"b\")") {
        TypeError::NotComparable { found, .. } => assert_eq!(found, Type::String),
        other => panic!("expected NotComparable, got {other:?}"),
    }
}

/// AND becomes an `if`, which is where the branch comes from. Asserting the
/// SHAPE here is what makes the codegen-unchanged claim checkable.
#[test]
fn and_becomes_an_if_so_that_codegen_needs_no_new_node() {
    let b = body("f(a: Boolean, b: Boolean): Boolean = a AND b");
    assert!(
        matches!(b.kind, TypedExprKind::If { .. }),
        "AND must lower to an If: {:?}",
        b.kind
    );
}

// ------------------------------------------------------ SPIKE-COMPOSITE-TYPE

/// The interner is what lets `Type` carry a composite and stay `Copy`: a shared
/// reference is `Copy` whatever it points at. Two equal element lists must come
/// back as the SAME pointer, or `Type`'s derived `PartialEq` compares slices
/// that happen to be equal today and stops being a cheap identity tomorrow.
#[test]
fn interning_dedups_and_type_stays_copy() {
    let a = intern_types(&[Type::ZZ32, Type::String]);
    let b = intern_types(&[Type::ZZ32, Type::String]);
    let c = intern_types(&[Type::ZZ32, Type::ZZ64]);
    assert!(
        std::ptr::eq(a, b),
        "equal element lists must intern to one slice"
    );
    assert!(!std::ptr::eq(a, c));
    assert_eq!(Type::Tuple(a), Type::Tuple(b));
    assert_ne!(Type::Tuple(a), Type::Tuple(c));

    // `Copy` by use: if `Type` ever stops being `Copy` this line stops
    // compiling, which is the same guard `types.rs`'s `assert_copy` makes at
    // the definition. Two independent statements of one invariant on purpose.
    let t = Type::Tuple(a);
    let moved = t;
    assert_eq!(t, moved);
}

/// `name` and `symbol` BUILD the answer rather than returning a placeholder.
/// One string for every tuple would give two different tuples the same symbol,
/// which is a collision in the emitted object rather than a diagnostic.
#[test]
fn a_tuple_type_names_and_symbolises_its_elements() {
    let t = Type::Tuple(intern_types(&[Type::ZZ32, Type::String]));
    assert_eq!(t.name(), "(ZZ32, String)");
    assert_eq!(t.symbol(), "tuple_zz32_string");
    let u = Type::Tuple(intern_types(&[Type::String, Type::ZZ32]));
    assert_ne!(t.symbol(), u.symbol());
}

/// A tuple is NOT an array element and is NOT a reference, and both answers are
/// written down rather than defaulted.
#[test]
fn a_tuple_is_neither_an_array_element_nor_a_reference() {
    let t = Type::Tuple(intern_types(&[Type::ZZ32, Type::ZZ32]));
    assert!(fortress_types::Elem::of(t).is_none());
    assert!(!t.is_reference());
    assert!(!t.is_numeric());
}

/// AND THE VARIANT IS STILL UNCONSTRUCTABLE FROM SOURCE. `registry.rs`'s
/// `resolve` is the single gate and it still refuses -- that is what makes the
/// twenty non-exhaustive sites safe today rather than merely unexercised.
#[test]
fn a_tuple_parameter_on_a_function_with_a_body_is_refused() {
    match body_error("f(x: (ZZ32, ZZ32)): () = ()") {
        TypeError::TupleNotStorable { position, .. } => assert_eq!(position, "a parameter"),
        other => panic!("expected TupleNotStorable, got {other:?}"),
    }
}

/// THE OTHER SIDE, and it is the whole point of the split: an `api` is checked
/// and NEVER LOWERED, so a signature may name a tuple it has no representation
/// for. `Library/FortressLibrary.fsi:1730` needs exactly this -- a tuple as a
/// STATIC ARGUMENT -- and :2347 needs it as a parameter.
#[test]
fn an_api_signature_may_name_a_tuple() {
    let source = "api t\n                  f(x: (ZZ32, ZZ32)): ZZ32\n                  g(): (ZZ32, String)\n                  end\n";
    let tokens = fortress_lexer::lex(source).expect("lex");
    let ast = fortress_parser::parse(&tokens).expect("parse");
    let checked = check(&ast);
    assert!(
        checked.is_ok(),
        "an api has no body to lower: {:?}",
        checked.err()
    );
}

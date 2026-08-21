// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules; an
// integration test is its own crate, so the workspace denies apply here.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use fortress_ast::{
    BinOp, BlockItem, Component, Decl, Expr, Fixity, ImportItems, ImportedName, TypeRef, UnOp,
};
use fortress_parser::{parse, ParseError};

fn component(src: &str) -> Component {
    let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    parse(&tokens).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{src}"))
}

/// Wraps a bare expression in the smallest legal component so the expression
/// grammar can be tested without repeating boilerplate.
fn expr(src: &str) -> Expr {
    let wrapped = format!("component t\nf() = {src}\nend\n");
    let c = component(&wrapped);
    match c.decls.into_iter().next() {
        Some(Decl::Function(f)) => f.body.expect("a body"),
        other => panic!("expected a function, got {other:?}"),
    }
}

fn expr_error(src: &str) -> ParseError {
    let wrapped = format!("component t\nf() = {src}\nend\n");
    let tokens = fortress_lexer::lex(&wrapped).unwrap_or_else(|e| panic!("lex failed: {e}"));
    match parse(&tokens) {
        Ok(_) => panic!("expected {src:?} to fail to parse"),
        Err(e) => e,
    }
}

// ------------------------------------------------- the byte-span math itself

#[test]
fn tight_minus_is_subtraction() {
    match expr("x-1") {
        Expr::Infix {
            op: BinOp::Sub,
            fixity: Fixity::Tight,
            ..
        } => {}
        other => panic!("expected a tight infix subtraction, got {other:?}"),
    }
}

#[test]
fn loose_minus_is_also_subtraction_but_loose() {
    match expr("x - 1") {
        Expr::Infix {
            op: BinOp::Sub,
            fixity: Fixity::Loose,
            ..
        } => {}
        other => panic!("expected a loose infix subtraction, got {other:?}"),
    }
}

#[test]
fn minus_glued_only_on_the_right_is_a_prefix_juxtaposed_with_the_left() {
    match expr("x -1") {
        Expr::Juxt { items, .. } => {
            assert_eq!(
                items.len(),
                2,
                "expected two juxtaposed operands: {items:?}"
            );
            assert!(matches!(items.first(), Some(Expr::Var { .. })));
            assert!(
                matches!(items.get(1), Some(Expr::Prefix { op: UnOp::Neg, .. })),
                "second operand should be a negation: {items:?}"
            );
        }
        other => panic!("expected a juxtaposition, got {other:?}"),
    }
}

#[test]
fn minus_glued_only_on_the_left_is_postfix_and_out_of_subset() {
    assert!(matches!(
        expr_error("x- 1"),
        ParseError::PostfixOperatorUnsupported { .. }
    ));
}

#[test]
fn the_three_spacings_produce_three_different_trees() {
    let tight = expr("x-1");
    let loose = expr("x - 1");
    let prefix = expr("x -1");
    assert_ne!(tight, loose, "tight and loose must differ in fixity");
    assert_ne!(
        loose, prefix,
        "loose infix and prefix juxtaposition must differ in shape"
    );
    assert_ne!(tight, prefix);
}

// ------------------------------------------------------------ juxtaposition

#[test]
fn juxtaposition_stays_flat_because_its_meaning_depends_on_types() {
    match expr("x f(y)") {
        Expr::Juxt { items, .. } => {
            assert_eq!(items.len(), 2);
            assert!(matches!(items.get(1), Some(Expr::Call { .. })));
        }
        other => panic!("expected a flat juxtaposition, got {other:?}"),
    }
}

#[test]
fn a_glued_paren_is_application_and_a_spaced_one_is_juxtaposition() {
    assert!(matches!(expr("f(y)"), Expr::Call { .. }));
    match expr("f (y)") {
        Expr::Juxt { items, .. } => assert_eq!(items.len(), 2),
        other => panic!("expected juxtaposition for a spaced paren, got {other:?}"),
    }
}

#[test]
fn juxtaposition_binds_tighter_than_a_loose_infix_operator() {
    // `a b + c d` is `(a b) + (c d)`.
    match expr("a b + c d") {
        Expr::Infix {
            op: BinOp::Add,
            lhs,
            rhs,
            ..
        } => {
            assert!(
                matches!(*lhs, Expr::Juxt { .. }),
                "lhs should be a juxtaposition: {lhs:?}"
            );
            assert!(
                matches!(*rhs, Expr::Juxt { .. }),
                "rhs should be a juxtaposition: {rhs:?}"
            );
        }
        other => panic!("expected an addition of two juxtapositions, got {other:?}"),
    }
}

// ------------------------------------------------------------------ newlines

#[test]
fn a_newline_may_follow_a_loose_infix_operator() {
    // Library/String.fss:129-131 depends on this: one statement, not two.
    let src = "component t\nf() = do\n  a +\n  b\nend\nend\n";
    let c = component(src);
    match c.decls.into_iter().next() {
        Some(Decl::Function(f)) => match f.body.expect("a body") {
            Expr::Block { items, .. } => {
                assert_eq!(
                    items.len(),
                    1,
                    "a trailing operator continues the line: {items:?}"
                );
            }
            other => panic!("expected a block, got {other:?}"),
        },
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn a_newline_may_not_precede_a_loose_infix_operator() {
    // `a` newline `+ b` is two statements, so the block has two items.
    let src = "component t\nf() = do\n  a\n  + b\nend\nend\n";
    let c = component(src);
    match c.decls.into_iter().next() {
        Some(Decl::Function(f)) => match f.body.expect("a body") {
            Expr::Block { items, .. } => assert_eq!(items.len(), 2, "expected two statements"),
            other => panic!("expected a block, got {other:?}"),
        },
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn blank_lines_between_statements_are_not_extra_statements() {
    let src = "component t\nf() = do\n  a\n\n\n  b\nend\nend\n";
    let c = component(src);
    match c.decls.into_iter().next() {
        Some(Decl::Function(f)) => match f.body.expect("a body") {
            Expr::Block { items, .. } => assert_eq!(items.len(), 2),
            other => panic!("expected a block, got {other:?}"),
        },
        other => panic!("expected a function, got {other:?}"),
    }
}

// ------------------------------------------------------------------ bindings

#[test]
fn a_typed_local_binding_parses() {
    let src = "component t\nf() = do\n  j:ZZ64 = widen(20)\n  j\nend\nend\n";
    let c = component(src);
    match c.decls.into_iter().next() {
        Some(Decl::Function(f)) => match f.body.expect("a body") {
            Expr::Block { items, .. } => match items.first() {
                Some(BlockItem::Binding(b)) => {
                    assert_eq!(b.name, "j");
                    assert_eq!(b.ty.as_ref().map(TypeRef::written), Some("ZZ64".to_owned()));
                }
                other => panic!("expected a binding, got {other:?}"),
            },
            other => panic!("expected a block, got {other:?}"),
        },
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn a_newline_before_the_equals_means_it_is_not_a_binding() {
    // LocalDecl.rats:159 writes `s` before `=`, which forbids a newline there.
    let src = "component t\nf() = do\n  j:ZZ64\n  = widen(20)\nend\nend\n";
    let tokens = fortress_lexer::lex(src).expect("lex");
    assert!(
        parse(&tokens).is_err(),
        "a split binding must not parse as a binding"
    );
}

// ------------------------------------------------------------------ reserved

#[test]
fn a_reserved_word_is_rejected_by_name() {
    // Was `atomic` until M5 implemented it. `for` and `atomic` are both still
    // in the lexer's reserved list -- that is what keeps them out of the
    // identifier namespace -- and the parser intercepts them by name before
    // this arm is reached.
    match expr_error("spawn") {
        ParseError::ReservedWord { word, .. } => assert_eq!(word, "spawn"),
        other => panic!("expected a reserved word error, got {other:?}"),
    }
}

// -------------------------------------------------------- acceptance program

#[test]
fn the_m1_acceptance_program_parses() {
    let src = concat!(
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
    let c = component(src);
    assert_eq!(c.name, "fact");
    assert_eq!(c.exports, vec!["Executable".to_owned()]);
    assert_eq!(c.decls.len(), 2, "expected f and run");

    let Some(Decl::Function(f)) = c.decls.first() else {
        panic!("no f")
    };
    assert_eq!(f.name, "f");
    assert_eq!(f.params.len(), 1);
    assert_eq!(
        f.params.first().map(|p| p.ty.written()),
        Some("ZZ64".to_owned())
    );
    assert_eq!(
        f.return_type.as_ref().map(TypeRef::written),
        Some("ZZ64".to_owned())
    );

    // The body is the if, whose else branch is the recursive juxtaposition.
    let Expr::If {
        else_branch: Some(else_branch),
        ..
    } = f.body.as_ref().expect("a body")
    else {
        panic!("expected an if with an else, got {:?}", f.body)
    };
    let Expr::Juxt { items, .. } = else_branch.as_ref() else {
        panic!("else branch should be a juxtaposition, got {else_branch:?}")
    };
    assert_eq!(items.len(), 2, "`x f(x-1)` is two juxtaposed operands");

    // And the argument of that call is the tight subtraction.
    let Some(Expr::Call { args, .. }) = items.get(1) else {
        panic!("expected a call")
    };
    assert!(
        matches!(
            args.first(),
            Some(Expr::Infix {
                op: BinOp::Sub,
                fixity: Fixity::Tight,
                ..
            })
        ),
        "`x-1` must be a tight infix subtraction: {args:?}"
    );

    let Some(Decl::Function(run)) = c.decls.get(1) else {
        panic!("no run")
    };
    assert_eq!(run.name, "run");
    assert!(run.params.is_empty());
    let Expr::Block { items, .. } = run.body.as_ref().expect("a body") else {
        panic!("run should be a block")
    };
    assert_eq!(items.len(), 2, "a binding and a println: {items:?}");
}

// ------------------------------------------------ M3b: arrays and iteration

fn block_items(src: &str) -> Vec<BlockItem> {
    match expr(src) {
        Expr::Block { items, .. } => items,
        other => panic!("expected a block, got {other:?}"),
    }
}

#[test]
fn an_array_literal_keeps_its_elements_in_order() {
    match expr("[1, 2, 3]") {
        Expr::ArrayLit { items, .. } => assert_eq!(items.len(), 3),
        other => panic!("expected an array literal, got {other:?}"),
    }
}

#[test]
fn an_empty_array_literal_parses() {
    match expr("[]") {
        Expr::ArrayLit { items, .. } => assert!(items.is_empty()),
        other => panic!("expected an empty array literal, got {other:?}"),
    }
}

#[test]
fn a_glued_bracket_is_a_subscript() {
    match expr("a[0]") {
        Expr::Index { .. } => {}
        other => panic!("expected an index, got {other:?}"),
    }
}

/// `f (x)` is already a juxtaposition rather than a call, and a bracket obeys
/// the same rule: gluing is what makes it a subscript.
#[test]
fn a_spaced_bracket_is_a_juxtaposition_not_a_subscript() {
    match expr("a [0]") {
        Expr::Juxt { items, .. } => assert_eq!(items.len(), 2),
        other => panic!("expected a juxtaposition, got {other:?}"),
    }
}

#[test]
fn a_static_argument_is_parsed_as_the_type_it_names() {
    let src = "component t\nf(a:Array[\\ZZ64\\]):ZZ64 = 0\nend\n";
    let c = component(src);
    let Some(Decl::Function(f)) = c.decls.into_iter().next() else {
        panic!("no decl")
    };
    let param = f.params.into_iter().next().expect("a parameter");
    assert_eq!(
        param.ty.written(),
        "Array[\\ZZ64\\]",
        "the element type must survive parsing"
    );
}

#[test]
fn a_while_loop_parses_its_condition_and_body() {
    match expr("do\n   while x < 3 do\n      println(x)\n   end\nend") {
        Expr::Block { items, .. } => match items.into_iter().next() {
            Some(BlockItem::Expr(Expr::While { .. })) => {}
            other => panic!("expected a while, got {other:?}"),
        },
        other => panic!("expected a block, got {other:?}"),
    }
}

#[test]
fn colon_equals_declares_a_mutable_binding() {
    match block_items("do\n   i:ZZ64 := 0\nend").into_iter().next() {
        Some(BlockItem::Binding(b)) => {
            assert!(b.mutable, "`:=` declares a mutable binding");
            assert_eq!(b.name, "i");
        }
        other => panic!("expected a binding, got {other:?}"),
    }
}

#[test]
fn equals_still_declares_an_immutable_binding() {
    match block_items("do\n   i:ZZ64 = 0\nend").into_iter().next() {
        Some(BlockItem::Binding(b)) => assert!(!b.mutable),
        other => panic!("expected a binding, got {other:?}"),
    }
}

#[test]
fn colon_equals_on_a_bare_name_is_an_assignment_not_a_declaration() {
    match block_items("do\n   i:ZZ64 := 0\n   i := 1\nend")
        .into_iter()
        .nth(1)
    {
        Some(BlockItem::Assign(a)) => match a.target {
            Expr::Var { name, .. } => assert_eq!(name, "i"),
            other => panic!("expected a variable target, got {other:?}"),
        },
        other => panic!("expected an assignment, got {other:?}"),
    }
}

#[test]
fn an_element_can_be_assigned() {
    match block_items("do\n   a[0] := 1\nend").into_iter().next() {
        Some(BlockItem::Assign(a)) => match a.target {
            Expr::Index { .. } => {}
            other => panic!("expected an index target, got {other:?}"),
        },
        other => panic!("expected an assignment, got {other:?}"),
    }
}

// ------------------------------- M3d lexer pass: imports and headerless files

#[test]
fn imports_are_recorded_and_the_brace_group_is_not_interpreted() {
    let src = "component t\n\
               import List.{...}\n\
               import Set.{Set, set}\n\
               import a.b.NestedOne.{...} except {ShellTrait}\n\
               import AliasTest.{ opr OPLUS => MYPLUS }\n\
               export Executable\n\
               run() = 1\n\
               end\n";
    let c = component(src);
    let names: Vec<&str> = c.imports.iter().map(|i| i.api_name.as_str()).collect();
    assert_eq!(names, vec!["List", "Set", "a.b.NestedOne", "AliasTest"]);
    assert_eq!(c.exports, vec!["Executable"]);
    assert_eq!(c.decls.len(), 1);
}

#[test]
fn import_api_is_a_different_form() {
    let c = component("component t\nimport api Collection\nrun() = 1\nend\n");
    let first = c.imports.first().expect("an import");
    assert!(first.is_api);
    assert_eq!(first.api_name, "Collection");
}

/// `Compilation.rats:14-19`: a file may be exports, imports and declarations
/// straight to end of file, with no `component` wrapper and no `end`.
#[test]
fn a_headerless_file_parses_to_end_of_input() {
    let c = component("export Executable\nimport List.{...}\nrun() = 1\n");
    assert!(c.name.is_empty(), "a headerless file has no component name");
    assert_eq!(c.exports, vec!["Executable"]);
    assert_eq!(c.imports.len(), 1);
    assert_eq!(c.decls.len(), 1);
}

#[test]
fn a_wrapped_component_still_requires_its_end() {
    let src = "component t\nrun() = 1\n";
    let tokens = fortress_lexer::lex(src).expect("lex");
    assert!(
        parse(&tokens).is_err(),
        "dropping `end` from a wrapped component must not silently become a headerless file"
    );
}

#[test]
fn imports_and_exports_may_come_in_either_order() {
    // The reference grammar has an error production for exports first, and the
    // corpus uses both.
    let a = component("component t\nimport L.{...}\nexport E\nrun() = 1\nend\n");
    let b = component("component t\nexport E\nimport L.{...}\nrun() = 1\nend\n");
    assert_eq!(a.imports.len(), b.imports.len());
    assert_eq!(a.exports, b.exports);
}

// ------------------------------------------------------------- type syntax

fn return_type(decl: &str) -> TypeRef {
    let src = format!("component t\n{decl}\nend\n");
    match component(&src).decls.into_iter().next() {
        Some(Decl::Function(f)) => f.return_type.expect("a declared return type"),
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn a_plain_name_is_a_named_type() {
    match return_type("f(): ZZ32 = 1") {
        TypeRef::Named { name, args, .. } => {
            assert_eq!(name, "ZZ32");
            assert!(args.is_empty());
        }
        other => panic!("expected a named type, got {other:?}"),
    }
}

#[test]
fn a_named_type_renders_its_static_arguments() {
    let written = return_type("f(): Array[\\ZZ64\\] = 1").written();
    assert_eq!(written, "Array[\\ZZ64\\]");
}

#[test]
fn empty_parentheses_are_the_unit_type() {
    match return_type("f(): () = println(\"hi\")") {
        TypeRef::Unit { .. } => {}
        other => panic!("expected the unit type, got {other:?}"),
    }
}

#[test]
fn a_parenthesised_type_is_the_type_itself() {
    match return_type("f(): (ZZ32) = 1") {
        TypeRef::Named { name, .. } => assert_eq!(name, "ZZ32"),
        other => panic!("expected the inner named type, got {other:?}"),
    }
}

#[test]
fn two_or_more_types_in_parentheses_are_a_tuple() {
    match return_type("f(): (ZZ32, String) = 1") {
        TypeRef::Tuple { elems, .. } => {
            assert_eq!(elems.len(), 2);
            assert_eq!(elems.first().map(TypeRef::written), Some("ZZ32".to_owned()));
            assert_eq!(
                elems.get(1).map(TypeRef::written),
                Some("String".to_owned())
            );
        }
        other => panic!("expected a tuple type, got {other:?}"),
    }
}

/// The parse error from a whole declaration, for grammar cases that cannot be
/// expressed as a bare expression.
fn decl_error(decl: &str) -> ParseError {
    let src = format!("component t\n{decl}\nend\n");
    let tokens = fortress_lexer::lex(&src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    match parse(&tokens) {
        Ok(_) => panic!("expected {decl:?} to fail to parse"),
        Err(e) => e,
    }
}

#[test]
fn a_glued_minus_greater_is_an_arrow_type() {
    match return_type("f(): ZZ32 -> String = 1") {
        TypeRef::Arrow { from, to, .. } => {
            assert_eq!(from.written(), "ZZ32");
            assert_eq!(to.written(), "String");
        }
        other => panic!("expected an arrow type, got {other:?}"),
    }
}

#[test]
fn arrow_types_are_right_associative() {
    assert_eq!(
        return_type("f(): ZZ32 -> String -> Boolean = 1").written(),
        "ZZ32 -> String -> Boolean"
    );
    match return_type("f(): ZZ32 -> String -> Boolean = 1") {
        TypeRef::Arrow { to, .. } => match *to {
            TypeRef::Arrow { .. } => {}
            other => panic!("expected the right side to be the nested arrow, got {other:?}"),
        },
        other => panic!("expected an arrow type, got {other:?}"),
    }
}

#[test]
fn a_spaced_minus_greater_is_not_an_arrow() {
    let e = decl_error("f(): ZZ32 - > String = 1");
    assert!(matches!(e, ParseError::UnexpectedToken { .. }), "got {e:?}");
}

#[test]
fn an_arrow_may_appear_inside_parentheses() {
    match return_type("f(): (ZZ32 -> String) = 1") {
        TypeRef::Arrow { .. } => {}
        other => panic!("expected an arrow type, got {other:?}"),
    }
}

#[test]
fn a_comma_separated_parenthesised_expression_is_a_tuple() {
    match expr("(1, 2)") {
        Expr::Tuple { items, .. } => assert_eq!(items.len(), 2),
        other => panic!("expected a tuple expression, got {other:?}"),
    }
}

#[test]
fn a_single_parenthesised_expression_is_not_a_tuple() {
    assert!(
        !matches!(expr("(1)"), Expr::Tuple { .. }),
        "a one-element parenthesised expression is not a tuple"
    );
}

// --------------------------------------------------- `=` and chained comparison

#[test]
fn a_bare_equals_in_expression_position_is_equality() {
    match expr("1 = 1") {
        Expr::Infix { op: BinOp::Eq, .. } => {}
        other => panic!("expected an equality, got {other:?}"),
    }
}

/// `f(x) = e` in block position is a local function declaration, which this
/// subset does not implement. Without this it would parse as a discarded
/// equality: 236 corpus files carry 572 such lines. The body here is itself an
/// equality, which is why the guard is on tokens and not on the parsed tree.
#[test]
fn a_local_function_declaration_is_refused() {
    match expr_error("do\n  isZero(x) = x = 0\n  isZero(1)\nend") {
        ParseError::LocalFunctionDeclarationUnsupported { .. } => {}
        other => panic!("expected a local function declaration diagnostic, got {other:?}"),
    }
}

/// Pinned deliberately: `try_binding` takes the first `=`, so this is a binding
/// whose value is a comparison. It was a parse error before `=` became an
/// operator, and this is the reading the design argues for.
#[test]
fn a_binding_of_a_comparison_binds_the_comparison() {
    match expr("do\n  b = 3 = 4\n  b\nend") {
        Expr::Block { items, .. } => match items.first() {
            Some(BlockItem::Binding(b)) => assert!(
                matches!(b.value, Expr::Infix { op: BinOp::Eq, .. }),
                "expected the binding's value to be an equality, got {:?}",
                b.value
            ),
            other => panic!("expected a binding, got {other:?}"),
        },
        other => panic!("expected a block, got {other:?}"),
    }
}

#[test]
fn a_two_operator_chain_desugars_to_a_block() {
    match expr("a < b < c") {
        // three temporaries and the nested if
        Expr::Block { items, .. } => assert_eq!(items.len(), 4, "got {items:?}"),
        other => panic!("a chain must desugar to a block, got {other:?}"),
    }
}

/// A single comparison is untouched: no block, no temporaries, byte-identical
/// to what every earlier milestone produced.
#[test]
fn a_single_comparison_is_not_a_chain() {
    match expr("0 < 1") {
        Expr::Infix { op: BinOp::Lt, .. } => {}
        other => panic!("a single comparison must stay a bare Infix, got {other:?}"),
    }
}

#[test]
fn a_chain_may_mix_equivalence_with_one_ordering_sense() {
    match expr("a <= b < c = d") {
        Expr::Block { items, .. } => assert_eq!(items.len(), 5, "got {items:?}"),
        other => panic!("expected a block, got {other:?}"),
    }
}

#[test]
fn a_chain_may_not_mix_two_ordering_senses() {
    match expr_error("1 <= 2 > 0") {
        ParseError::ChainedOperatorsDiffer { first, second, .. } => {
            assert_eq!((first, second), ("<=", ">"));
        }
        other => panic!("expected a mixed-sense diagnostic, got {other:?}"),
    }
}

/// A literal operand is not hoisted. Behind a binding it loses the type it
/// would take from the other operand, and `0 < mid(1)` became a ZZ32 against a
/// ZZ64 -- measured, on the evaluate-once fixture.
#[test]
fn a_chain_does_not_hoist_literals() {
    match expr("0 < b < 2") {
        Expr::Block { items, .. } => {
            assert_eq!(items.len(), 2, "only `b` needs a temporary: {items:?}");
        }
        other => panic!("expected a block, got {other:?}"),
    }
}

// ------------------------------------------------------------------- M3h

/// A component-level value binding parses into a nullary `FnDecl` carrying the
/// marker. The marker is the whole point: without it the checker cannot tell a
/// value from a function, and a value carried as a nullary function compiles a
/// program whose initializer never runs.
#[test]
fn a_component_level_value_declaration_is_marked_as_one() {
    for src in ["pi: ZZ32 = 3", "v = 1", "x := 0"] {
        match &component(src).decls[..] {
            [Decl::Function(f)] => {
                assert!(f.value_binding, "`{src}` is a value binding");
                assert!(f.params.is_empty(), "`{src}` takes no parameters");
            }
            other => panic!("expected one function declaration from `{src}`, got {other:?}"),
        }
    }
}

/// The branch that recognises a value must not steal a function. A function is
/// always an identifier then `[\` or `(`, so the two cannot collide -- but the
/// lookahead is one token and this is what pins it.
#[test]
fn a_value_declaration_does_not_steal_a_function() {
    for src in ["f(x: ZZ32): ZZ32 = x", "id[\\T\\](x: T): T = x"] {
        match &component(src).decls[..] {
            [Decl::Function(f)] => {
                assert!(!f.value_binding, "`{src}` is a function, not a value");
            }
            other => panic!("expected one function declaration from `{src}`, got {other:?}"),
        }
    }
}

/// `getter`, `setter` and a `self` parameter all parse inside a trait body.
/// None of them is checked -- dotted method dispatch is not implemented -- so
/// this pins the parse and nothing further.
#[test]
fn getters_setters_and_self_parameters_parse_in_a_trait_body() {
    let src = "trait Shape\n  \
                 getter size(): ZZ32\n  \
                 setter size(n: ZZ32): ()\n  \
                 area(self, k: ZZ32): ZZ32\n\
               end";
    match &component(src).decls[..] {
        [Decl::Trait(t)] => assert_eq!(t.members.len(), 3, "three members: {:?}", t.members),
        other => panic!("expected one trait declaration, got {other:?}"),
    }
}

// ------------------------------------------------------------------------ M5

/// `+=` is two tokens joined by adjacency, the same trade `<-` and `for` take.
/// Both spellings the corpus uses: glued on both sides, and spaced.
#[test]
fn a_compound_assignment_is_read_from_adjacency() {
    for src in ["do count+= 1 end", "do count += 1 end", "do count -= 1 end"] {
        let e = expr(src);
        let Expr::Block { items, .. } = &e else {
            panic!("expected a block, got {e:?}");
        };
        match items.first() {
            Some(BlockItem::Assign(a)) => assert!(
                a.op.is_some(),
                "`{src}` did not read as a compound assignment"
            ),
            other => panic!("`{src}` produced {other:?}"),
        }
    }
}

/// A plain `:=` keeps `op: None`, so nothing about M4's assignments moved.
#[test]
fn a_plain_assignment_carries_no_operator() {
    let e = expr("do count := 1 end");
    let Expr::Block { items, .. } = &e else {
        panic!("expected a block");
    };
    match items.first() {
        Some(BlockItem::Assign(a)) => assert!(a.op.is_none()),
        other => panic!("{other:?}"),
    }
}

/// 1.0 spells a mutable local two ways and the corpus uses both. The modifier
/// is what makes the `=` form unambiguous, so it needs no type annotation.
#[test]
fn var_declares_a_mutable_local() {
    for (src, mutable) in [
        ("do var count : ZZ32 = 0 end", true),
        ("do var count = 0 end", true),
        ("do count : ZZ32 := 0 end", true),
        ("do count : ZZ32 = 0 end", false),
    ] {
        let e = expr(src);
        let Expr::Block { items, .. } = &e else {
            panic!("expected a block for `{src}`");
        };
        match items.first() {
            Some(BlockItem::Binding(b)) => {
                assert_eq!(b.mutable, mutable, "`{src}`");
            }
            other => panic!("`{src}` produced {other:?}"),
        }
    }
}

/// `atomic` is intercepted at statement level, so `atomic do ... end` and a
/// bare `atomic sum += a[i]` become the same node.
#[test]
fn atomic_wraps_a_statement_or_a_block() {
    for src in ["do atomic do x := 1 end end", "do atomic x += 1 end"] {
        let e = expr(src);
        let Expr::Block { items, .. } = &e else {
            panic!("expected a block for `{src}`");
        };
        match items.first() {
            Some(BlockItem::Expr(Expr::Atomic { .. })) => {}
            other => panic!("`{src}` produced {other:?}"),
        }
    }
}

// ------------------------------------------------------- operator declarations

/// Every operator declaration is lifted to the ordinary declaration node whose
/// name is the operator's own text. `opr` used to be refused as a reserved word
/// wherever it appeared, so nothing that parsed before can reach these branches.
fn opr_names(src: &str) -> Vec<String> {
    let c = component(src);
    let mut names: Vec<String> = Vec::new();
    for decl in &c.decls {
        match decl {
            Decl::Function(f) => names.push(f.name.clone()),
            Decl::Trait(t) => names.extend(method_names(&t.members)),
            Decl::Object(o) => names.extend(method_names(&o.members)),
        }
    }
    names
}

fn method_names(members: &[fortress_ast::Member]) -> Vec<String> {
    members
        .iter()
        .filter_map(|m| match m {
            fortress_ast::Member::Method(m) => Some(m.name.clone()),
            fortress_ast::Member::Field(_) => None,
        })
        .collect()
}

#[test]
fn a_symbolic_operator_declaration_is_named_by_its_characters() {
    let names = opr_names(
        "api t\n\
         opr =(a: Any, b: Any): Boolean\n\
         opr =/=(a: Any, b: Any): Boolean\n\
         opr ||(a: String, b: String): String\n\
         opr |||(a: String, b: String): String\n\
         opr ///(a: Any, b: Any): Any\n\
         opr <->(a: Any, b: Any): Any\n\
         end\n",
    );
    assert_eq!(names, ["=", "=/=", "||", "|||", "///", "<->"]);
}

/// The run is glued, which is the same span-adjacency rule `->`, `+=` and `**`
/// are decided by. `<->` is three tokens and one name for that reason, and no
/// operator needed a lexer token of its own.
#[test]
fn an_operator_name_stops_at_its_parameter_list() {
    let names = opr_names(
        "api t\n\
         opr -(x: ZZ32): ZZ32\n\
         opr #(): ZZ32\n\
         opr :[\\I\\](r: I): I\n\
         opr CMP[\\A, B\\](t1: A, t2: B): ZZ32\n\
         opr juxtaposition(a: Any, b: Any): Any\n\
         end\n",
    );
    assert_eq!(names, ["-", "#", ":", "CMP", "juxtaposition"]);
}

/// `BIG` is a modifier and not the operator, so it folds into the name rather
/// than being dropped -- `opr BIG SQCAP` and `opr SQCAP` are two declarations.
#[test]
fn big_folds_into_the_operator_name() {
    let names = opr_names(
        "api t\n\
         opr BIG SQCAP[\\T\\](g: T): T\n\
         opr SQCAP[\\T\\](a: T, b: T): T\n\
         opr BIG ||(g: Any): String\n\
         end\n",
    );
    assert_eq!(names, ["BIG SQCAP", "SQCAP", "BIG ||"]);
}

/// An enclosing operator writes its operand INSIDE the brackets, so there is no
/// parameter list in the ordinary place. `_` marks where the operand goes, which
/// is what stops `|self|` being given the name `||` -- a real, different infix
/// operator that `FortressLibrary.fsi` also declares.
#[test]
fn an_enclosing_operator_is_named_around_its_operand() {
    let names = opr_names(
        "component t\n\
         object O\n\
         opr |self| : ZZ32 = 0\n\
         opr |\\self/| : ZZ32 = 1\n\
         opr |/self\\| : ZZ32 = 2\n\
         opr [i: ZZ32]: ZZ32 = i\n\
         end\n\
         end\n",
    );
    assert_eq!(names, ["|_|", "|\\_/|", "|/_\\|", "[_]"]);
}

/// The operand written BEFORE the operator. Both lists flatten into one, because
/// what comes out is a function and a function has one parameter list.
#[test]
fn a_leading_operand_joins_the_trailing_parameters() {
    let c = component("api t\nopr (l: ZZ32)::[\\I\\](s: ZZ32): ZZ32\nend\n");
    let Some(Decl::Function(f)) = c.decls.into_iter().next() else {
        panic!("expected a function");
    };
    assert_eq!(f.name, "::");
    assert_eq!(
        f.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["l", "s"]
    );
    assert_eq!(f.static_params.len(), 1);
}

/// `opr [i: ZZ32]: E throws NotFound` -- `Library/FortressLibrary.fsi:777`. The
/// clause is skipped and recorded nowhere: the language has no exceptions, so
/// there is nothing for it to mean yet.
#[test]
fn a_throws_clause_is_skipped_rather_than_refused() {
    let names = opr_names("api t\nopr [i: ZZ32]: ZZ32 throws NotFound\nend\n");
    assert_eq!(names, ["[_]"]);
}

/// The one lexer change the spike needed. `[\` and `\]` keep winning the longest
/// match, so no static-parameter list lexes differently for it existing.
#[test]
fn a_bare_backslash_does_not_swallow_a_static_parameter_list() {
    let names = opr_names("api t\nopr |\\self/|: ZZ32\nend\n");
    assert_eq!(names, ["|\\_/|"]);
}

// -------------------------------------------- modifiers and topology clauses

/// All four modifiers are already RESERVED words, so every one of these tests
/// failed on the pre-M6 parser with `reserved word ... is not in the
/// implemented subset` -- the branch is reachable only on a token that was
/// always an error.
#[test]
fn declaration_modifiers_are_recorded_on_the_declaration() {
    let c = component(
        "component t\n\
         value trait L[\\T\\] end\n\
         value object V(s: ZZ32) end\n\
         private scale(x: ZZ32): ZZ32 = x\n\
         end\n",
    );
    let mods: Vec<fortress_ast::Modifiers> = c
        .decls
        .iter()
        .map(|d| match d {
            Decl::Trait(t) => t.modifiers,
            Decl::Object(o) => o.modifiers,
            Decl::Function(f) => f.modifiers,
        })
        .collect();
    let at = |i: usize| *mods.get(i).expect("three declarations");
    assert!(at(0).value && !at(0).private);
    assert!(at(1).value);
    assert!(at(2).private && !at(2).value);
}

/// `abstract opr <(self, other: T): Boolean` -- `Library/CompilerAlgebra.fss:18`,
/// which is a member, an operator and a modifier at once. The flag is recorded
/// and NOT read: M3c decides abstractness from `body.is_none()`, and giving one
/// fact two sources is how they come to disagree.
#[test]
fn a_member_takes_the_same_modifiers_a_declaration_does() {
    let c = component(
        "component t\n\
         trait T\n\
         abstract opr <(self, other: T): Boolean\n\
         private Min_W: ZZ32 = -1\n\
         end\n\
         end\n",
    );
    let Some(Decl::Trait(t)) = c.decls.into_iter().next() else {
        panic!("expected a trait");
    };
    match t.members.first().expect("a member") {
        fortress_ast::Member::Method(m) => {
            assert_eq!(m.name, "<");
            assert!(m.modifiers.abstract_);
            assert!(m.body.is_none(), "the flag is stored, not consulted");
        }
        other => panic!("expected a method, got {other:?}"),
    }
}

/// The 22 library files. The clause is on the line BELOW the header, and the
/// diagnostic used to come from the wrong side -- `expected a field or method
/// name, found KwExtends` on something that is not a member at all.
#[test]
fn a_topology_clause_may_sit_on_the_line_below_its_header() {
    let c = component(
        "component t\n\
         trait A end\n\
         trait B end\n\
         object KeyOverlap(k: ZZ32)\n\
         \x20       extends A\n\
         end\n\
         trait L\n\
         \x20       extends { A, B }\n\
         \x20       excludes { A }\n\
         end\n\
         end\n",
    );
    let objects: Vec<&fortress_ast::ObjectDecl> = c
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Object(o) => Some(o),
            _ => None,
        })
        .collect();
    assert_eq!(objects.first().expect("an object").extends.len(), 1);
    let Some(Decl::Trait(l)) = c
        .decls
        .iter()
        .find(|d| matches!(d, Decl::Trait(t) if t.name == "L"))
    else {
        panic!("expected trait L");
    };
    assert_eq!(l.extends.len(), 2);
    assert_eq!(l.excludes.len(), 1);
}

/// `ProjectFortress/parser_tests/XXXtraitClauses.fss:17` writes them backwards.
/// Reading the three in a loop rather than in a fixed order costs nothing and
/// removes a second way to be wrong.
#[test]
fn topology_clauses_may_come_in_any_order() {
    let c = component(
        "component t\n\
         trait W end\n\
         trait S end\n\
         trait T excludes W extends S comprises {U, V} end\n\
         end\n",
    );
    let Some(Decl::Trait(t)) = c
        .decls
        .iter()
        .find(|d| matches!(d, Decl::Trait(t) if t.name == "T"))
    else {
        panic!("expected trait T");
    };
    assert_eq!(t.extends.len(), 1);
    assert_eq!(t.excludes.len(), 1);
    assert_eq!(t.comprises.len(), 2);
}

/// `comprises { ... }`, 32 corpus sites. There is no `...` token, so it is three
/// `Dot`s, and the open-set marker is DROPPED -- honest while nothing reads
/// `comprises`, because an open set and an unwritten one are the same empty
/// list today.
#[test]
fn an_open_comprises_set_parses_to_an_empty_list() {
    let c = component("api t\ntrait T comprises { ... } end\nend\n");
    let Some(Decl::Trait(t)) = c.decls.into_iter().next() else {
        panic!("expected a trait");
    };
    assert!(t.comprises.is_empty());
}

/// `native component File` -- the modifier belongs to the COMPONENT, which is
/// not the `native f()` shape the feature is usually described by and is the
/// only one the corpus writes. Read and dropped: a native body lives in C, and
/// that is a milestone rather than a flag.
#[test]
fn a_component_header_may_carry_a_modifier() {
    let c = component("native component AnyType\ntrait Any end\nend\n");
    assert_eq!(c.name, "AnyType");
    assert_eq!(c.decls.len(), 1);
}

/// An object may `excludes`, so both clause lists are carried on `ObjectDecl`
/// as well -- one loop reads them for both declaration kinds.
#[test]
fn an_object_carries_the_clause_lists_a_trait_does() {
    let c = component("api t\ntrait A end\nobject O extends A excludes A end\nend\n");
    let Some(Decl::Object(o)) = c.decls.iter().find(|d| matches!(d, Decl::Object(_))) else {
        panic!("expected an object");
    };
    assert_eq!(o.extends.len(), 1);
    assert_eq!(o.excludes.len(), 1);
    assert!(o.comprises.is_empty());
}

// ------------------------------------------------------------------ varargs

/// `Parameter.rats:88` is `BindId w colon w Type w ellipses`. There is no `...`
/// token -- it is three `Dot`s -- so what makes them one is that they are glued
/// to EACH OTHER, the same adjacency reading `->`, `<-` and `+=` already take.
#[test]
fn three_glued_dots_after_a_parameter_type_are_varargs() {
    let c = component("api t\nassert(x: ZZ32, failMsg: String...): ()\nend\n");
    let Some(Decl::Function(f)) = c.decls.into_iter().next() else {
        panic!("expected a function");
    };
    assert_eq!(
        f.params.iter().map(|p| p.varargs).collect::<Vec<_>>(),
        [false, true]
    );
}

/// The grammar's `w` before `ellipses` permits whitespace, so the run is not
/// required to be glued to the type it follows.
#[test]
fn a_spaced_ellipsis_is_the_same_declaration_as_a_glued_one() {
    let c = component("api t\nf(x: ZZ32 ...): ()\nend\n");
    let Some(Decl::Function(f)) = c.decls.into_iter().next() else {
        panic!("expected a function");
    };
    assert!(f.params.first().expect("a parameter").varargs);
}

/// Three dots that are not glued to each other are three `Dot`s, and a `Dot`
/// after a parameter type is what it always was: not a parameter list.
#[test]
fn unglued_dots_are_not_an_ellipsis() {
    let src = "api t\nf(x: ZZ32 . . .): ()\nend\n";
    let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    assert!(parse(&tokens).is_err(), "`. . .` must not read as varargs");
}

/// `objects.tex:100` spells an object's varargs parameter `transient Varargs`,
/// so the bare form is a static error. Two corpus files write it and both are
/// must-FAIL tests -- accepting them would have grown the must-fail set by two
/// while the corpus count went up by two, which is the exact trade this project
/// refuses.
#[test]
fn an_object_value_parameter_may_not_be_varargs() {
    let src = "component t\nobject O(x: ZZ32...) end\nrun() = ()\nend\n";
    let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    match parse(&tokens) {
        Err(ParseError::ObjectVarargsParameter { name, .. }) => assert_eq!(name, "x"),
        other => panic!("expected an object-varargs refusal, got {other:?}"),
    }
}

/// `Library/List.fsi:108`. An enclosing operator carries its static parameters
/// BETWEEN the opener and the operand, which is nowhere `opr_tail` looks.
#[test]
fn an_enclosing_operator_carries_static_parameters_before_its_operand() {
    let c = component("api t\nopr <|[\\E\\] xs: E... |>: List[\\E\\]\nend\n");
    let Some(Decl::Function(f)) = c.decls.into_iter().next() else {
        panic!("expected a function");
    };
    assert_eq!(f.name, "<|_|>");
    assert_eq!(f.static_params.len(), 1);
    assert!(f.params.first().expect("a parameter").varargs);
}

/// `Library/Set.fsi:56`. The comprehension bracket has static parameters and NO
/// operand, so what identifies the form is the opener closing again.
#[test]
fn an_enclosing_operator_may_have_no_operand_at_all() {
    let names = opr_names("api t\nopr BIG {[\\T\\]} : ZZ32\nend\n");
    assert_eq!(names, ["BIG {_}"]);
}

/// `Library/Map.fsi:100`: four characters open it and one closes it. Reading the
/// closer with the opener's length as a limit cannot parse that, and reading it
/// unbounded would eat the `:` of the return type.
#[test]
fn the_closing_half_of_an_encloser_need_not_match_the_opening_half() {
    let c = component("api t\nopr {|->[\\K,V\\] xs: K... }: Map[\\K,V\\]\nend\n");
    let Some(Decl::Function(f)) = c.decls.into_iter().next() else {
        panic!("expected a function");
    };
    assert_eq!(f.name, "{|->_}");
    assert!(f.return_type.is_some(), "the return type must survive");
}

/// The three tokens the closing run stops at are the ones that END a
/// declaration. `=` is the one that matters in a component: without it the
/// closer swallows the `=` and the body becomes the operator's name.
#[test]
fn a_closing_run_does_not_swallow_the_body_marker() {
    let c = component("component t\nobject O\nopr |self| = 0\nend\nend\n");
    let Some(Decl::Object(o)) = c.decls.into_iter().next() else {
        panic!("expected an object");
    };
    assert_eq!(method_names(&o.members), ["|_|"]);
}

// -------------------------------------------------------------- named `end`

/// `TraitObject.rats:13` writes the tail as `((s "trait")? s Id)?`. All four
/// spellings close the same declaration.
#[test]
fn a_declaration_may_be_closed_by_its_own_name() {
    for src in [
        "component t\ntrait A end\nend\n",
        "component t\ntrait A end A\nend\n",
        "component t\ntrait A end trait A\nend\n",
        "component t\nobject O end O\nend\n",
        "component t\nobject O end object O\nend\n",
        "component t\ntrait A end\nend t\n",
        "component t\ntrait A end\nend component t\n",
        "api t.u\ntrait A end\nend t.u\n",
    ] {
        let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
        assert!(parse(&tokens).is_ok(), "should parse:\n{src}");
    }
}

/// `s`, not `w`. `end` then a NEWLINE then a name is the end of one declaration
/// followed by the next, and reading the name would silently merge them.
#[test]
fn a_name_on_the_next_line_does_not_close_the_declaration() {
    let c = component("component t\ntrait A end\nB() = 0\nend\n");
    assert_eq!(c.decls.len(), 2, "`B` is a declaration, not a closing name");
}

/// `ProjectFortress/parser_tests/XXXending.Name.fss` writes
/// `end XxXending.Name` for a component called `XXXending.Name` and is a
/// must-FAIL test. Accepting the tail without comparing it would have turned
/// that file green.
#[test]
fn a_closing_name_that_differs_is_refused() {
    let src = "component t\ntrait A end B\nend\n";
    let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    match parse(&tokens) {
        Err(ParseError::ClosingNameDiffers {
            found, expected, ..
        }) => {
            assert_eq!(found, "B");
            assert_eq!(expected, "A");
        }
        other => panic!("expected a closing-name refusal, got {other:?}"),
    }
}

/// A block's `end` is a different production. `end out` and `end loop` in the
/// corpus close a LABELLED BLOCK, so reading a name there would consume a
/// juxtaposed operand.
#[test]
fn a_block_end_takes_no_name() {
    let c = component("component t\nf() = do 1 end\ng() = 2\nend\n");
    assert_eq!(c.decls.len(), 2);
}

// -------------------------------------------- continuation-line declarations

/// `NamedFnHeaderFront = Id (w StaticParams)? w ValParam` and
/// `FnHeaderClause = (w NoNewlineIsType)? FnClauses`. Every `w` there is
/// may-newline, and the library writes long headers across lines --
/// `Library/FortressLibrary.fsi:305` breaks before the parameter list,
/// `Library/RangeInternals.fsi:576` before the static parameters,
/// `Library/Set.fsi:63` before the return type.
#[test]
fn a_declaration_header_may_break_across_lines() {
    for src in [
        "api t\nf\n(x: ZZ32): ZZ32\nend\n",
        "api t\nf[\\T\\]\n(x: T): T\nend\n",
        "api t\nf\n[\\T\\](x: T): T\nend\n",
        "api t\nf(x: ZZ32):\n    ZZ32\nend\n",
        "api t\nf(x: ZZ32)\n    : ZZ32\nend\n",
        "api t\nopr juxtaposition\n    (self, b: ZZ32): ZZ32\nend\n",
        "api t\ntrait A\n[\\T\\] end\nend\n",
        "api t\ntrait A\n    f(x: ZZ32):\n        ZZ32\nend\nend\n",
    ] {
        let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
        assert!(parse(&tokens).is_ok(), "should parse:\n{src}");
    }
}

/// The newlines may only be eaten when the optional clause is really there.
/// Without that test the separator disappears and two declarations become one.
#[test]
fn a_missing_optional_clause_leaves_the_separator_alone() {
    let c = component("api t\nf(x: ZZ32)\ng(y: ZZ32)\nend\n");
    assert_eq!(c.decls.len(), 2);
}

/// `FnClause = w Where / w Throws`. The diagnostic before this was
/// `expected a field or method name, found KwWhere`, which names a mechanism a
/// `where` clause is not: it is not a member at all.
#[test]
fn where_and_throws_may_sit_on_the_line_below_the_header() {
    for src in [
        "api t\ntrait A end\nf[\\T\\](x: T): T\n    where { T extends A }\nend\n",
        "api t\nf(x: ZZ32): ZZ32\n    throws NotFound\nend\n",
        "api t\ntrait C\n    getter get(): ZZ32 throws NotFound\nend\nend\n",
    ] {
        let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
        assert!(parse(&tokens).is_ok(), "should parse:\n{src}");
    }
}

/// `NoNewlineHeader.rats:48-52` gives `where` two shapes, and D6 section 1 cuts
/// one of them: a v1 where clause CONSTRAINS declared static parameters, so the
/// binder form -- which introduces fresh ones -- is refused BY NAME. It used to
/// be skipped, which is how `Library/PrefixMap.fsi` reached the terminus with a
/// clause nothing had read.
#[test]
fn a_where_clause_binder_form_is_refused_by_name() {
    for src in [
        "api t\ntrait A end\nf[\\T\\](x: T): T where [\\ T \\]\nend\n",
        "api t\ntrait A end\nf[\\T\\](x: T): T where [\\ T \\] { T extends A }\nend\n",
    ] {
        let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
        match parse(&tokens) {
            Err(ParseError::WhereClauseFormUnsupported { form, .. }) => {
                assert!(form.contains("fresh static variables"), "{form}");
            }
            other => panic!("expected a where-form refusal, got {other:?}\n{src}"),
        }
    }
}

/// One rule, one answer. `[\nat n\]` is refused by M3d at the parser and
/// `where [\nat n\]` must not be the hole it slips through -- which it was
/// while the clause was a token skip. The binder form is refused whole now, so
/// the acceptance is closed from the outside; the KIND refusal is still what
/// answers the bracket list, and both are asserted here because it is the pair
/// that matters, not either one alone.
#[test]
fn a_where_binding_refuses_the_kinds_a_static_parameter_list_refuses() {
    let src = "api t\ntrait T where [\\ nat n \\]\nend\nend\n";
    let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    match parse(&tokens) {
        Err(ParseError::WhereClauseFormUnsupported { .. }) => {}
        other => panic!("`where [\\nat n\\]` must be refused, got {other:?}"),
    }

    let src = "api t\ntrait T[\\ nat n \\]\nend\nend\n";
    let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    // Either refusal keeps the rule this test exists for: a static-parameter
    // kind the compiler locks out may not slip through a `where`. D6 section 1
    // cut where-VARIABLES from v1 outright, so the merged parser may refuse the
    // whole form before it reads the kind, and the D7 scaffold splits the kind
    // refusal in two. What must never happen is that it parses.
    match parse(&tokens) {
        Err(
            ParseError::StaticParameterKindUnsupported { .. }
            | ParseError::StaticParameterKindPendingDecision { .. }
            | ParseError::WhereClauseFormUnsupported { .. },
        ) => {}
        other => panic!("a `nat` binding must not slip through a `where`, got {other:?}"),
    }
}

// ------------------------------------------------------------ the `=` guard

/// `Symbol.rats:201` is `equals = "=" (!op)` and the reference grammar reaches
/// it only from a binding or a keyword argument. `Library/RangeInternals.fss:453`
/// writes `ex=-1` INSIDE the body of `opr =`, where it is an equality and not a
/// definition -- which is the whole reason the guard exists and the whole reason
/// it cannot live in the lexer.
#[test]
fn an_equals_glued_to_an_operator_is_not_a_definition() {
    match expr("ex=-1") {
        Expr::Infix { op: BinOp::Eq, .. } => {}
        other => panic!("expected an equality, got {other:?}"),
    }
}

/// The spaced form still binds, and so does a glued one whose right-hand side
/// starts with a bracket: `Symbol.rats:175-177` keeps enclosers out of `op`.
#[test]
fn a_definition_equals_still_binds() {
    let c = component("component t\nf() = do\n  x = -1\n  y =[1, 2]\n  x\nend\nend\n");
    let Some(Decl::Function(f)) = c.decls.into_iter().next() else {
        panic!("expected a function");
    };
    let Some(Expr::Block { items, .. }) = f.body else {
        panic!("expected a block");
    };
    let bindings = items
        .iter()
        .filter(|i| matches!(i, BlockItem::Binding(_)))
        .count();
    assert_eq!(bindings, 2, "both are definitions");
}

/// `Library/QuickCheck.fsi:409`. The longest match splits `==>` into `=` then
/// `=>`; the operator run re-glues them by span adjacency, which is the same
/// mechanism `|||` and `<->` already rest on.
#[test]
fn a_declared_operator_may_be_named_out_of_equals_signs() {
    let names = opr_names("api t\nopr ==>(p: Boolean, q: Boolean): Boolean\nend\n");
    assert_eq!(names, ["==>"]);
}

/// `SpecData/examples/advanced/OprDecl.Nofix.fss:23` and
/// `ProjectFortress/BirdyLib/Bazaar.fsi:22`. Each of these was a LEXER death
/// before the six characters had tokens.
#[test]
fn an_operator_may_be_named_out_of_the_six_new_characters() {
    let names = opr_names(
        "api t\n\
         opr !(a: ZZ32): ZZ32\n\
         opr @(a: ZZ32): ZZ32\n\
         opr ~(a: ZZ32): ZZ32\n\
         opr $(a: ZZ32): ZZ32\n\
         opr %(a: ZZ32): ZZ32\n\
         opr ?(a: ZZ32): ZZ32\n\
         end\n",
    );
    assert_eq!(names, ["!", "@", "~", "$", "%", "?"]);
}

/// `Library/FortressLibrary.fsi:1991`. The name came out right BEFORE the run
/// was one token, because the operator run re-glues `BarBar` and `Bar` by span
/// adjacency -- so this test pins that one token produces the same name, which
/// is what makes the lexer change safe in declaration position.
#[test]
fn a_three_bar_operator_is_named_the_same_as_one_token_as_it_was_as_two() {
    assert_eq!(
        opr_names("api t\nopr |||(a: ZZ32, b: ZZ32): ZZ32\nend\n"),
        ["|||"]
    );
    assert_eq!(
        opr_names("api t\nopr ||(a: ZZ32, b: ZZ32): ZZ32\nend\n"),
        ["||"]
    );
}

// -------------------------------------------- the operator expression level

/// `operator-app.tex:28-33` makes an all-capitals word an operator, and
/// `opr-fixity.tex:28-32` makes the consequence binding: "the Fortress language
/// dictates only the rules of syntax; whether an operator has a meaning when
/// used in a particular way depends only on whether there is a definition".
///
/// So this must PARSE as an application and only then fail to resolve. Before
/// the rule it was a three-element juxtaposition that folded with
/// multiplication: `SUBSET: ZZ64 = 2` then `println(3 SUBSET 4)` printed 24.
#[test]
fn a_named_infix_operator_applies_the_function_of_that_name() {
    match expr("a SUBSET b") {
        Expr::Call { callee, args, .. } => {
            assert!(matches!(*callee, Expr::Var { ref name, .. } if name == "SUBSET"));
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected a call to `SUBSET`, got {other:?}"),
    }
}

/// Infix `||` was the largest single first-blocker FEATURE in the corpus, filed
/// under aggregate literals because the marker regex could not see a bare `||`.
/// A run of three or more is one operator (`lexical-structure.tex:1174-1177`).
#[test]
fn the_vertical_line_operators_apply_infix() {
    match expr("a || b") {
        Expr::Call { callee, .. } => {
            assert!(matches!(*callee, Expr::Var { ref name, .. } if name == "||"));
        }
        other => panic!("expected a call to `||`, got {other:?}"),
    }
    match expr("a ||| b") {
        Expr::Call { callee, .. } => {
            assert!(matches!(*callee, Expr::Var { ref name, .. } if name == "|||"));
        }
        other => panic!("expected a call to `|||`, got {other:?}"),
    }
}

/// `precedence.tex:20-31`: "if there is no specific precedence relationship
/// between two operators, then parentheses must be used". A total ladder can
/// only ACCEPT, so the alternative to this refusal is a silent grouping.
#[test]
fn operators_from_unrelated_groups_must_be_parenthesised() {
    for src in [
        "a + b SUBSET c",
        "a SUBSET b + c",
        "a * b SUBSET c",
        "a SUBSET b UNION c",
        "a AND b SUBSET c",
        "a < b SUBSET c",
    ] {
        match expr_error(src) {
            ParseError::OperatorsUnrelated { .. } => {}
            other => panic!("{src} should need parentheses, got {other:?}"),
        }
    }
}

/// And the parenthesis is what makes it legal, which is the whole point of the
/// rule. The mark cannot be read off the tree -- `primary` returns a
/// parenthesised expression unchanged, so `(a SUBSET b) + c` and
/// `a SUBSET b + c` are the same node.
#[test]
fn parentheses_relate_what_precedence_does_not() {
    for src in [
        "(a SUBSET b) + c",
        "a + (b SUBSET c)",
        "(a SUBSET b) UNION c",
        "f(a SUBSET b) + c",
        "a[b SUBSET c] + d",
    ] {
        let wrapped = format!("component t\ng() = {src}\nend\n");
        let tokens = fortress_lexer::lex(&wrapped).unwrap_or_else(|e| panic!("lex failed: {e}"));
        assert!(parse(&tokens).is_ok(), "should parse: {src}");
    }
}

/// The same operator twice is a chain of itself and needs no parentheses.
#[test]
fn one_operator_repeated_is_left_associative() {
    match expr("a SUBSET b SUBSET c") {
        Expr::Call { callee, args, .. } => {
            assert!(matches!(*callee, Expr::Var { ref name, .. } if name == "SUBSET"));
            assert!(
                matches!(args.first(), Some(Expr::Call { .. })),
                "left associative"
            );
        }
        other => panic!("expected a call, got {other:?}"),
    }
}

/// `opr-fixity.tex:100-102`: an infix operator may be loose or tight but not
/// LOPSIDED. The table calls that row a static error outright.
#[test]
fn a_lopsided_infix_operator_is_refused() {
    match expr_error("a SUBSET-b") {
        ParseError::LopsidedOperator { name, .. } => assert_eq!(name, "SUBSET"),
        other => panic!("expected a lopsided refusal, got {other:?}"),
    }
    // Tight on both sides is legal.
    let wrapped = "component t\ng() = a SUBSET b\nend\n";
    let tokens = fortress_lexer::lex(wrapped).unwrap_or_else(|e| panic!("lex failed: {e}"));
    assert!(parse(&tokens).is_ok());
}

/// The reason the twelve-row table exists rather than `fixity_at`. After a left
/// encloser the table reads `|` as PREFIX, so the operator level leaves it --
/// and the enclosing-application production is what then picks it up. With the
/// table saying `infix` there instead, `f(|x|)` would be an infix `|` looking
/// for a right operand and finding `)`.
#[test]
fn a_bar_after_a_left_encloser_opens_an_encloser_rather_than_an_infix() {
    match expr("f(|x|)") {
        Expr::Call { args, .. } => match args.first() {
            Some(Expr::Call { callee, .. }) => {
                assert!(matches!(**callee, Expr::Var { ref name, .. } if name == "|_|"));
            }
            other => panic!("expected an enclosed argument, got {other:?}"),
        },
        other => panic!("expected a call, got {other:?}"),
    }
}

/// `AND`, `OR` and `NOT` are operator words under the same lexical rule and
/// keep their own paths: they have real codegen through `BinOp` and `UnOp`, and
/// routing them through a call to an undeclared function would break every
/// program that uses them. The acceptance test is the IR of the corpus, and
/// this is the shape assertion under it.
#[test]
fn the_three_logical_operator_words_keep_their_own_nodes() {
    assert!(matches!(
        expr("a AND b"),
        Expr::Infix { op: BinOp::And, .. }
    ));
    assert!(matches!(expr("a OR b"), Expr::Infix { op: BinOp::Or, .. }));
    assert!(matches!(expr("NOT a"), Expr::Prefix { op: UnOp::Not, .. }));
}

/// `seq` is LOWERCASE and so an ordinary identifier, not an operator word. It
/// shared a helper with `AND` and `OR` and stopped being recognised the moment
/// that helper moved to the operator-word token -- eleven fixtures and nine
/// corpus files, every one of them a `for` loop.
#[test]
fn a_sequential_generator_is_still_recognised() {
    match expr("for i <- seq(0#5) do i end") {
        Expr::For { sequential, .. } => assert!(sequential),
        other => panic!("expected a sequential for, got {other:?}"),
    }
}

// ------------------------------------------------ enclosing operator application

/// The declaration side already spells the pair `|_|`, `<|_|>`, `{_}` with `_`
/// where the operand goes. The application is the same name applied, so it is
/// an ordinary `Call` and nothing downstream learns a node.
#[test]
fn an_enclosing_operator_applies_the_function_of_its_paired_name() {
    for (src, name, arity) in [
        ("|3|", "|_|", 1),
        ("<|1|>", "<|_|>", 1),
        ("<|1, 2, 3|>", "<|_|>", 3),
        ("{1, 2}", "{_}", 2),
        ("||3||", "||_||", 1),
    ] {
        match expr(src) {
            Expr::Call { callee, args, .. } => {
                assert!(
                    matches!(*callee, Expr::Var { name: ref n, .. } if n == name),
                    "{src} should apply `{name}`"
                );
                assert_eq!(args.len(), arity, "{src}");
            }
            other => panic!("{src}: expected a call, got {other:?}"),
        }
    }
}

/// An empty encloser has no operand to stop the opening run, so the run
/// swallows the closing half -- `<|` is glued to `|>`. When the run is of even
/// length and nothing that could begin an expression follows it, the halves ARE
/// the pair.
#[test]
fn an_empty_encloser_is_one_run_split_in_half() {
    for (src, name) in [("<||>", "<|_|>"), ("{}", "{_}")] {
        match expr(src) {
            Expr::Call { callee, args, .. } => {
                assert!(matches!(*callee, Expr::Var { name: ref n, .. } if n == name));
                assert!(args.is_empty(), "{src} encloses nothing");
            }
            other => panic!("{src}: expected a call, got {other:?}"),
        }
    }
}

/// The closer is read with the OPENER'S length as its limit. Without that the
/// closing run walks on into the `+`, and `|a| + |b|` becomes one encloser
/// whose name ends `|+|`.
#[test]
fn a_closing_run_stops_at_the_openers_length() {
    match expr("|1| + |2|") {
        Expr::Infix { op: BinOp::Add, .. } => {}
        other => panic!("expected an addition of two enclosers, got {other:?}"),
    }
}

/// `[` is DELIBERATELY not an enclosing operator here. `[1, 2, 3]` already has
/// its own node and its own codegen, and reading it as an application of `[_]`
/// would change what every array-literal program means.
#[test]
fn a_bracket_literal_is_still_an_array_literal() {
    assert!(matches!(expr("[1, 2, 3]"), Expr::ArrayLit { .. }));
}

// ------------------------------------------------------ imports and exports

/// `Compilation.rats` gives the export the same APIName the component header
/// takes. The header read a dotted name and the export, fourteen lines later,
/// read an identifier -- so `component Compiled5.a` parsed and
/// `export Compiled5.a` did not.
#[test]
fn an_export_takes_a_dotted_or_braced_name() {
    let c = component("component t\nexport Compiled5.a\nexport {A, B}\nf() = 0\nend\n");
    assert_eq!(c.exports, ["Compiled5.a", "A", "B"]);
}

/// The brace group used to be consumed as a balanced token run and thrown
/// away. A resolver cannot answer `source-code.tex:280-287`'s question --
/// which of two apis a name came from -- without knowing which names were
/// asked for.
#[test]
fn an_import_records_what_it_names() {
    let c = component(
        "component t\n\
         import List.{...}\n\
         import Map.{a, b as c}\n\
         import FlatString.FlatString\n\
         import api Foo\n\
         import Set.{...} except { emptyList, opr BIG UNION }\n\
         f() = 0\n\
         end\n",
    );
    let names: Vec<&str> = c.imports.iter().map(|i| i.api_name.as_str()).collect();
    assert_eq!(names, ["List", "Map", "FlatString", "Foo", "Set"]);
    let at = |n: usize| c.imports.get(n).expect("an import");
    assert_eq!(at(0).items, ImportItems::OnDemand);
    assert_eq!(
        at(1).items,
        ImportItems::Named(vec![
            ImportedName {
                name: "a".to_owned(),
                alias: None
            },
            ImportedName {
                name: "b".to_owned(),
                alias: Some("c".to_owned())
            },
        ])
    );
    // `import FlatString.FlatString` is the api `FlatString` and one name in
    // it; only the file system can say where the api name ends, so both
    // readings are carried.
    assert_eq!(
        at(2).items,
        ImportItems::Named(vec![ImportedName {
            name: "FlatString".to_owned(),
            alias: None
        }])
    );
    assert!(at(3).is_api);
    assert_eq!(at(4).except, ["emptyList", "BIG UNION"]);
}

/// `simpleNameTest.fsi:15` imports an ENCLOSING operator, whose two halves are
/// written with a space between. What says a second half follows is the
/// opener's own MIRROR and nothing weaker: `opr BIG SYMDIFF }` ends an except
/// set, and `opr <| => ||}` glues the alias to the list's closing brace.
#[test]
fn an_imported_operator_may_be_an_enclosing_pair() {
    let c = component("component t\nimport Set.{ opr { } }\nf() = 0\nend\n");
    assert_eq!(
        c.imports.first().expect("an import").items,
        ImportItems::Named(vec![ImportedName {
            name: "{_}".to_owned(),
            alias: None
        }])
    );
    let c = component("component t\nimport List.{Cons => CC, opr <| => ||}\nf() = 0\nend\n");
    assert_eq!(
        c.imports.first().expect("an import").items,
        ImportItems::Named(vec![
            ImportedName {
                name: "Cons".to_owned(),
                alias: Some("CC".to_owned())
            },
            ImportedName {
                name: "<|".to_owned(),
                alias: Some("||".to_owned())
            },
        ])
    );
}

/// `source-code.tex:280-287` disambiguates "the type `List` declared in the API
/// `List` or the type `List` declared in the API `PureList`" with a qualified
/// name, and with ten api names duplicated across the source path the collision
/// is not hypothetical. It parses; it resolves nowhere, which is honest.
#[test]
fn a_type_name_may_be_qualified() {
    let c = component("api t\nf(x: List.List): ZZ32\nend\n");
    let Some(Decl::Function(f)) = c.decls.into_iter().next() else {
        panic!("expected a function");
    };
    match f.params.first().map(|p| &p.ty) {
        Some(TypeRef::Named { name, .. }) => assert_eq!(name, "List.List"),
        other => panic!("expected a qualified name, got {other:?}"),
    }
}

/// 39 corpus files reach a JVM implementation this way, and three of them are
/// bootstrap files whose bodies have no other implementation in the tree --
/// which is C-shim work, not import work. What phase 3 owes the construct is a
/// diagnostic that names it, in place of
/// `expected a newline or `;`, found Ident("com")`.
#[test]
fn a_foreign_import_is_refused_by_name() {
    let src = "component t\nimport java com.sun.x.{y}\nf() = 0\nend\n";
    let tokens = fortress_lexer::lex(src).unwrap_or_else(|e| panic!("lex failed: {e}"));
    match parse(&tokens) {
        Err(ParseError::ForeignImportUnsupported { .. }) => {}
        other => panic!("expected a foreign-import refusal, got {other:?}"),
    }
}

/// `lexical-structure.tex:1216-1222`: an operator immediately followed by `=`
/// is ONE token, a compound assignment operator. Reading only the operator half
/// reports `x ||= e` as a LOPSIDED infix -- a real rule, and not the one the
/// program broke. `||=` alone is 37 corpus uses.
#[test]
fn a_compound_assignment_operator_is_refused_by_its_own_name() {
    for (src, op) in [("x ||= 1", "||"), ("x MAX= 1", "MAX"), ("x @= 1", "@")] {
        let wrapped = format!("component t\nf() = do\n  x = 1\n  {src}\n  x\nend\nend\n");
        let tokens = fortress_lexer::lex(&wrapped).unwrap_or_else(|e| panic!("lex failed: {e}"));
        match parse(&tokens) {
            Err(ParseError::CompoundAssignmentUnsupported { op: found, .. }) => {
                assert_eq!(found, op, "{src}");
            }
            other => panic!("{src}: expected a compound-assignment refusal, got {other:?}"),
        }
    }
    // And the operator alone still applies.
    assert!(matches!(expr("a || b"), Expr::Call { .. }));
}

// ------------------------------------------------- the `nat` scaffold (D7)

/// ALL SIX STILL REFUSE. `2026-08-21-d7-reconcile-nat.md` §3.4 is explicit that
/// the decision comes BEFORE the parser change -- "doing (2) before (1) means
/// the parser accepts a shape nobody has decided the meaning of" -- and D7's
/// own header says drafted, not adopted. This test is the proof that the
/// scaffold changed diagnostics and nothing else.
#[test]
fn every_static_parameter_kind_still_refuses() {
    for kind in ["nat", "int", "bool", "unit", "dim", "opr"] {
        let src = format!("api t\ntrait T[\\{kind} n\\] end\nend\n");
        let tokens = fortress_lexer::lex(&src).unwrap_or_else(|e| panic!("lex failed: {e}"));
        match parse(&tokens) {
            Err(ParseError::StaticParameterKindPendingDecision { kind: k, .. })
            | Err(ParseError::StaticParameterKindUnsupported { kind: k, .. }) => {
                assert_eq!(k, kind);
            }
            other => panic!("`{kind}` must still refuse, got {other:?}"),
        }
    }
}

/// The three groups answer to different owners, and the diagnostic says which:
/// `nat`/`int`/`bool` are waiting on a DECISION that is already written,
/// `unit`/`dim` on sub-phase 4d, `opr` on the operator-property traits. A
/// reader who hits one needs to know which door to knock on.
#[test]
fn the_static_parameter_kinds_split_into_the_three_groups_d7_names() {
    for kind in ["nat", "int", "bool"] {
        let src = format!("api t\ntrait T[\\{kind} n\\] end\nend\n");
        let tokens = fortress_lexer::lex(&src).unwrap_or_else(|e| panic!("lex failed: {e}"));
        assert!(
            matches!(
                parse(&tokens),
                Err(ParseError::StaticParameterKindPendingDecision { .. })
            ),
            "`{kind}` is D7's group"
        );
    }
    for kind in ["unit", "dim", "opr"] {
        let src = format!("api t\ntrait T[\\{kind} n\\] end\nend\n");
        let tokens = fortress_lexer::lex(&src).unwrap_or_else(|e| panic!("lex failed: {e}"));
        assert!(
            matches!(
                parse(&tokens),
                Err(ParseError::StaticParameterKindUnsupported { .. })
            ),
            "`{kind}` stays refused when D7's group opens"
        );
    }
}

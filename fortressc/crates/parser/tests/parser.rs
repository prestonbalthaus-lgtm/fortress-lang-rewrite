// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules; an
// integration test is its own crate, so the workspace denies apply here.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use fortress_ast::{BinOp, BlockItem, Component, Decl, Expr, Fixity, TypeRef, UnOp};
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

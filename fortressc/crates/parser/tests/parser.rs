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
    match expr_error("atomic") {
        ParseError::ReservedWord { word, .. } => assert_eq!(word, "atomic"),
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

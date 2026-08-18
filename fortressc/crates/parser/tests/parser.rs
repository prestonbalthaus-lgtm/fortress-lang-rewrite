// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules; an
// integration test is its own crate, so the workspace denies apply here.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use fortress_ast::{BinOp, BlockItem, Component, Decl, Expr, Fixity, UnOp};
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
        Some(Decl::Function(f)) => f.body,
        None => panic!("no decl parsed from {src:?}"),
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
        Some(Decl::Function(f)) => match f.body {
            Expr::Block { items, .. } => {
                assert_eq!(
                    items.len(),
                    1,
                    "a trailing operator continues the line: {items:?}"
                );
            }
            other => panic!("expected a block, got {other:?}"),
        },
        None => panic!("no decl"),
    }
}

#[test]
fn a_newline_may_not_precede_a_loose_infix_operator() {
    // `a` newline `+ b` is two statements, so the block has two items.
    let src = "component t\nf() = do\n  a\n  + b\nend\nend\n";
    let c = component(src);
    match c.decls.into_iter().next() {
        Some(Decl::Function(f)) => match f.body {
            Expr::Block { items, .. } => assert_eq!(items.len(), 2, "expected two statements"),
            other => panic!("expected a block, got {other:?}"),
        },
        None => panic!("no decl"),
    }
}

#[test]
fn blank_lines_between_statements_are_not_extra_statements() {
    let src = "component t\nf() = do\n  a\n\n\n  b\nend\nend\n";
    let c = component(src);
    match c.decls.into_iter().next() {
        Some(Decl::Function(f)) => match f.body {
            Expr::Block { items, .. } => assert_eq!(items.len(), 2),
            other => panic!("expected a block, got {other:?}"),
        },
        None => panic!("no decl"),
    }
}

// ------------------------------------------------------------------ bindings

#[test]
fn a_typed_local_binding_parses() {
    let src = "component t\nf() = do\n  j:ZZ64 = widen(20)\n  j\nend\nend\n";
    let c = component(src);
    match c.decls.into_iter().next() {
        Some(Decl::Function(f)) => match f.body {
            Expr::Block { items, .. } => match items.first() {
                Some(BlockItem::Binding(b)) => {
                    assert_eq!(b.name, "j");
                    assert_eq!(b.ty.as_ref().map(|t| t.name.as_str()), Some("ZZ64"));
                }
                other => panic!("expected a binding, got {other:?}"),
            },
            other => panic!("expected a block, got {other:?}"),
        },
        None => panic!("no decl"),
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
    assert_eq!(f.params.first().map(|p| p.ty.name.as_str()), Some("ZZ64"));
    assert_eq!(
        f.return_type.as_ref().map(|t| t.name.as_str()),
        Some("ZZ64")
    );

    // The body is the if, whose else branch is the recursive juxtaposition.
    let Expr::If {
        else_branch: Some(else_branch),
        ..
    } = &f.body
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
    let Expr::Block { items, .. } = &run.body else {
        panic!("run should be a block")
    };
    assert_eq!(items.len(), 2, "a binding and a println: {items:?}");
}

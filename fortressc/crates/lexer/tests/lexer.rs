// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules; an
// integration test is its own crate, so the workspace denies apply here and a
// failing assertion could not panic. Test code is exempt on purpose.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use fortress_lexer::{lex, Kind, LexErrorKind};

fn kinds(src: &str) -> Vec<Kind<'_>> {
    match lex(src) {
        Ok(tokens) => tokens.into_iter().map(|t| t.kind).collect(),
        Err(e) => panic!("expected {src:?} to lex, got {e}"),
    }
}

fn spans(src: &str) -> Vec<(usize, usize)> {
    match lex(src) {
        Ok(tokens) => tokens
            .into_iter()
            .map(|t| (t.span.start, t.span.end))
            .collect(),
        Err(e) => panic!("expected {src:?} to lex, got {e}"),
    }
}

fn err(src: &str) -> LexErrorKind {
    match lex(src) {
        Ok(t) => panic!("expected {src:?} to fail, got {t:?}"),
        Err(e) => e.kind,
    }
}

fn int(text: &str, digits: &str) -> Kind<'static> {
    // Leaked so the helper can hand back a 'static Kind for comparison.
    Kind::IntLit {
        text: Box::leak(text.to_owned().into_boxed_str()),
        digits: digits.to_owned(),
    }
}

// ---------------------------------------------------------------- operators

#[test]
fn equality_is_three_equals_and_there_is_no_double_equals() {
    assert_eq!(
        kinds("a === b"),
        vec![Kind::Ident("a"), Kind::EqEqEq, Kind::Ident("b"), Kind::Eof]
    );
    assert_eq!(
        kinds("a =/= b"),
        vec![Kind::Ident("a"), Kind::NotEq, Kind::Ident("b"), Kind::Eof]
    );
    assert_eq!(err("a == b"), LexErrorKind::MalformedEquals);
}

#[test]
fn solidus_operators_are_not_comments() {
    assert_eq!(
        kinds("a // b"),
        vec![
            Kind::Ident("a"),
            Kind::SlashSlash,
            Kind::Ident("b"),
            Kind::Eof
        ]
    );
    assert_eq!(
        kinds("a /// b"),
        vec![
            Kind::Ident("a"),
            Kind::SlashSlashSlash,
            Kind::Ident("b"),
            Kind::Eof
        ]
    );
}

#[test]
fn comparison_and_arithmetic_operators() {
    assert_eq!(
        kinds("a <= b >= c < d > e"),
        vec![
            Kind::Ident("a"),
            Kind::Le,
            Kind::Ident("b"),
            Kind::Ge,
            Kind::Ident("c"),
            Kind::Lt,
            Kind::Ident("d"),
            Kind::Gt,
            Kind::Ident("e"),
            Kind::Eof,
        ]
    );
    assert_eq!(
        kinds("a + b - c * d / e"),
        vec![
            Kind::Ident("a"),
            Kind::Plus,
            Kind::Ident("b"),
            Kind::Minus,
            Kind::Ident("c"),
            Kind::Star,
            Kind::Ident("d"),
            Kind::Slash,
            Kind::Ident("e"),
            Kind::Eof,
        ]
    );
}

#[test]
fn colon_equals_is_one_token() {
    assert_eq!(
        kinds("j:=x"),
        vec![Kind::Ident("j"), Kind::ColonEq, Kind::Ident("x"), Kind::Eof]
    );
    assert_eq!(
        kinds("j:x"),
        vec![Kind::Ident("j"), Kind::Colon, Kind::Ident("x"), Kind::Eof]
    );
}

#[test]
fn sub_token_guards_reject_double_star_and_trailing_plus() {
    assert_eq!(err("a ** b"), LexErrorKind::DoubleStar);
    assert_eq!(err("a++b"), LexErrorKind::OperatorFollowedByPlus);
    // Spaced is fine: the guard is adjacency only.
    assert_eq!(kinds("a + +b").len(), 5);
}

// ----------------------------------------------------------------- literals

#[test]
fn integer_literals_strip_group_separators() {
    assert_eq!(kinds("1000000"), vec![int("1000000", "1000000"), Kind::Eof]);
    assert_eq!(
        kinds("1'000'000"),
        vec![int("1'000'000", "1000000"), Kind::Eof]
    );
    assert_eq!(
        kinds("1\u{202F}000"),
        vec![int("1\u{202F}000", "1000"), Kind::Eof]
    );
}

#[test]
fn zz64_scale_literals_are_not_truncated() {
    let big = "2432902008176640000";
    assert_eq!(kinds(big), vec![int(big, big), Kind::Eof]);
}

#[test]
fn float_literals_split_at_the_single_dot() {
    assert_eq!(
        kinds("3.14"),
        vec![
            Kind::FloatLit {
                text: "3.14",
                int_digits: "3".to_owned(),
                frac_digits: "14".to_owned()
            },
            Kind::Eof
        ]
    );
    assert_eq!(err("12.52.23"), LexErrorKind::MultipleDecimalPoints);
}

#[test]
fn a_numeral_with_letters_is_one_erroring_token_not_a_juxtaposition() {
    // `2x` must never lex as 2 juxtaposed with x.
    assert_eq!(err("2x"), LexErrorKind::NumeralWithLetters);
    assert_eq!(err("1e10"), LexErrorKind::NumeralWithLetters);
    // Loose is the only legal way to write it.
    assert_eq!(
        kinds("2 x"),
        vec![int("2", "2"), Kind::Ident("x"), Kind::Eof]
    );
}

#[test]
fn strings_decode_escapes_and_treat_comment_marks_as_inert() {
    assert_eq!(
        kinds(r#""hi""#),
        vec![Kind::StrLit("hi".to_owned()), Kind::Eof]
    );
    assert_eq!(
        kinds(r#""a\nb""#),
        vec![Kind::StrLit("a\nb".to_owned()), Kind::Eof]
    );
    // demos/Cfa.fss:119 shape: comment delimiters inside a string are content.
    assert_eq!(
        kinds(r#""(* not a comment *)""#),
        vec![Kind::StrLit("(* not a comment *)".to_owned()), Kind::Eof]
    );
    assert_eq!(
        err("\"unterminated"),
        LexErrorKind::UnterminatedStringLiteral
    );
    assert_eq!(
        err("\"spans\nlines\""),
        LexErrorKind::RawLineTerminatorInString
    );
}

// ----------------------------------------------------------------- keywords

#[test]
fn keyword_prefixes_do_not_steal_identifiers() {
    assert_eq!(kinds("if"), vec![Kind::KwIf, Kind::Eof]);
    assert_eq!(kinds("iffy"), vec![Kind::Ident("iffy"), Kind::Eof]);
    assert_eq!(kinds("end"), vec![Kind::KwEnd, Kind::Eof]);
    assert_eq!(kinds("endian"), vec![Kind::Ident("endian"), Kind::Eof]);
}

#[test]
fn the_acceptance_programs_vocabulary_is_not_reserved() {
    for word in [
        "run",
        "Executable",
        "widen",
        "println",
        "ZZ32",
        "ZZ64",
        "RR64",
        "Boolean",
    ] {
        assert_eq!(
            kinds(word),
            vec![Kind::Ident(word), Kind::Eof],
            "{word} should be an identifier"
        );
    }
    // But `widens` is reserved, and `widen` is not.
    assert_eq!(kinds("widens"), vec![Kind::Reserved("widens"), Kind::Eof]);
}

#[test]
fn the_other_reserved_words_are_reserved_not_identifiers() {
    for word in [
        "atomic", "opr", "grammar", "for", "abstract", "syntax", "value",
    ] {
        assert_eq!(
            kinds(word),
            vec![Kind::Reserved(word), Kind::Eof],
            "{word} should be reserved"
        );
    }
}

/// M3c promoted nine of them out of the reserved list and gave them to the
/// parser. Being reserved and being a keyword are different states, and this is
/// the line between them.
#[test]
fn the_declaration_vocabulary_is_a_keyword_rather_than_a_reserved_word() {
    for (word, kind) in [
        ("api", Kind::KwApi),
        ("trait", Kind::KwTrait),
        ("object", Kind::KwObject),
        ("extends", Kind::KwExtends),
        ("comprises", Kind::KwComprises),
        ("excludes", Kind::KwExcludes),
        ("where", Kind::KwWhere),
        ("var", Kind::KwVar),
        ("self", Kind::KwSelf),
        ("import", Kind::KwImport),
        ("except", Kind::KwExcept),
    ] {
        assert_eq!(
            kinds(word),
            vec![kind, Kind::Eof],
            "{word} should be a keyword"
        );
    }
}

#[test]
fn braces_lex() {
    assert_eq!(
        kinds("{a, b}"),
        vec![
            Kind::LBrace,
            Kind::Ident("a"),
            Kind::Comma,
            Kind::Ident("b"),
            Kind::RBrace,
            Kind::Eof
        ]
    );
}

#[test]
fn booleans_are_literals_rather_than_keywords() {
    assert_eq!(
        kinds("true false"),
        vec![Kind::True, Kind::False, Kind::Eof]
    );
}

// ------------------------------------------------------- newlines and gaps

#[test]
fn a_newline_terminates_a_statement() {
    assert_eq!(
        kinds("a\nb"),
        vec![Kind::Ident("a"), Kind::Newline, Kind::Ident("b"), Kind::Eof]
    );
}

#[test]
fn blank_line_runs_collapse_to_one_newline() {
    assert_eq!(
        kinds("a\n\n\n\nb"),
        vec![Kind::Ident("a"), Kind::Newline, Kind::Ident("b"), Kind::Eof]
    );
}

#[test]
fn leading_and_trailing_breaks_are_suppressed() {
    assert_eq!(kinds("\n\na\n\n"), vec![Kind::Ident("a"), Kind::Eof]);
}

#[test]
fn ampersand_cancels_the_line_terminator() {
    // The whole point of the continuation rule: no Newline token survives.
    assert_eq!(
        kinds("a&\nb"),
        vec![Kind::Ident("a"), Kind::Ident("b"), Kind::Eof]
    );
    // Without it, the same source is two statements.
    assert_eq!(
        kinds("a\nb"),
        vec![Kind::Ident("a"), Kind::Newline, Kind::Ident("b"), Kind::Eof]
    );
}

#[test]
fn ampersand_matches_the_reference_test_file() {
    // ProjectFortress/tests/ampersand.fss:19-20, which asserts 9 from x = 3,
    // so `3&` newline `x` must become the loose juxtaposition `3 x`.
    assert_eq!(
        kinds("3&\nx"),
        vec![int("3", "3"), Kind::Ident("x"), Kind::Eof]
    );
}

#[test]
fn ampersand_absorbs_spacing_and_a_spacing_comment_before_the_terminator() {
    assert_eq!(
        kinds("a&   \nb"),
        vec![Kind::Ident("a"), Kind::Ident("b"), Kind::Eof]
    );
    assert_eq!(
        kinds("a&(* c *)\nb"),
        vec![Kind::Ident("a"), Kind::Ident("b"), Kind::Eof]
    );
}

#[test]
fn a_dangling_ampersand_is_an_error() {
    assert_eq!(err("a & b"), LexErrorKind::DanglingContinuation);
    assert_eq!(err("a&b"), LexErrorKind::DanglingContinuation);
}

// ----------------------------------------------------------------- comments

#[test]
fn block_comments_nest() {
    assert_eq!(kinds("a (* outer (* inner *) still *) b").len(), 3);
    assert_eq!(err("a (* unbalanced"), LexErrorKind::UnterminatedComment);
}

#[test]
fn a_line_comment_ends_at_the_line_and_does_not_break_it_twice() {
    assert_eq!(
        kinds("a (*) trailing\nb"),
        vec![Kind::Ident("a"), Kind::Newline, Kind::Ident("b"), Kind::Eof]
    );
}

#[test]
fn repeated_line_comment_openers_on_one_line_are_inert() {
    // Library/CompilerLibrary.fsi:168 shape. Treating the second `(*` as a
    // nested opener errors on a shipped library file.
    assert_eq!(kinds("a (*) one (*) two\nb").len(), 4);
}

#[test]
fn apostrophes_and_semicolons_inside_comments_are_inert() {
    // demos/GenomeUtil2a.fss:126 shape.
    assert_eq!(kinds("a (*) don't; really\nb").len(), 4);
}

#[test]
fn a_block_comment_containing_a_terminator_still_breaks_the_line() {
    assert_eq!(
        kinds("a (* multi\nline *) b"),
        vec![Kind::Ident("a"), Kind::Newline, Kind::Ident("b"), Kind::Eof]
    );
}

// ------------------------------------------------------------- whitespace

#[test]
fn tabs_are_rejected_outside_comments_and_accepted_inside_them() {
    assert_eq!(err("a\tb"), LexErrorKind::InvalidWhitespace);
    assert_eq!(kinds("a (* \t *) b").len(), 3);
}

#[test]
fn non_ascii_is_rejected_outside_comments_and_strings() {
    assert_eq!(err("a \u{2208} b"), LexErrorKind::NonAsciiCharacter);
    // U+202F is the one exception, and only inside a numeral.
    assert_eq!(kinds("1\u{202F}000").len(), 2);
    assert_eq!(err("a \u{202F} b"), LexErrorKind::NonAsciiCharacter);
    // Inside a comment or a string it is content.
    assert_eq!(kinds("a (* \u{2208} *) b").len(), 3);
    assert_eq!(kinds("\"\u{2208}\"").len(), 2);
}

#[test]
fn out_of_subset_literals_fail_with_specific_errors() {
    assert_eq!(err("'x'"), LexErrorKind::CharacterLiteralUnsupported);
    assert_eq!(
        err("\u{201C}hi\u{201D}"),
        LexErrorKind::CurlyQuoteStringUnsupported
    );
    assert_eq!(err("7FFF_16"), LexErrorKind::RadixNumeralUnsupported);
}

// -------------------------------------- unary minus: shape same, spans differ

#[test]
fn minus_spacings_produce_identically_shaped_streams() {
    let tight = kinds("x-1");
    let loose = kinds("x - 1");
    let prefix = kinds("x -1");

    let expect = vec![Kind::Ident("x"), Kind::Minus, int("1", "1"), Kind::Eof];
    assert_eq!(tight, expect);
    assert_eq!(loose, expect);
    assert_eq!(prefix, expect);
}

#[test]
fn minus_spacings_are_separated_only_by_byte_spans() {
    // The parser decides fixity from adjacency, so the spans must differ even
    // though the kinds do not.
    let tight = spans("x-1");
    let loose = spans("x - 1");
    let prefix = spans("x -1");

    // tight: x[0,1) -[1,2) 1[2,3)  -> glued on both sides
    assert_eq!(tight.first(), Some(&(0, 1)));
    assert_eq!(tight.get(1), Some(&(1, 2)));
    assert_eq!(tight.get(2), Some(&(2, 3)));

    // loose: gaps on both sides
    assert_eq!(loose.get(1), Some(&(2, 3)));
    assert_eq!(loose.get(2), Some(&(4, 5)));

    // prefix: gap on the left only, glued on the right
    assert_eq!(prefix.get(1), Some(&(2, 3)));
    assert_eq!(prefix.get(2), Some(&(3, 4)));

    assert_ne!(tight, loose);
    assert_ne!(loose, prefix);
    assert_ne!(tight, prefix);
}

// ------------------------------------------------------- acceptance program

#[test]
fn the_m1_acceptance_program_lexes() {
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
    let tokens = lex(src).unwrap_or_else(|e| panic!("acceptance program failed to lex: {e}"));

    assert_eq!(tokens.first().map(|t| &t.kind), Some(&Kind::KwComponent));
    assert_eq!(tokens.last().map(|t| &t.kind), Some(&Kind::Eof));

    // The two juxtapositions that make this the acceptance program are both
    // loose, so both operands carry a gap and neither is glued.
    let glued_pairs = tokens
        .windows(2)
        .filter(|w| {
            w.first()
                .is_some_and(|a| w.get(1).is_some_and(|b| a.span.end == b.span.start))
        })
        .count();
    assert!(glued_pairs > 0, "expected some tight adjacency, e.g. f(");

    // No Newline may lead or trail.
    assert_ne!(tokens.first().map(|t| &t.kind), Some(&Kind::Newline));
}

#[test]
fn fat_arrow_is_one_token_rather_than_a_malformed_equals() {
    assert_eq!(
        kinds("x => y"),
        vec![
            Kind::Ident("x"),
            Kind::FatArrow,
            Kind::Ident("y"),
            Kind::Eof
        ]
    );
    // A genuinely malformed `=` still reports as one.
    assert_eq!(err("x =: y"), LexErrorKind::MalformedEquals);
}

/// The characters that were sending 319 of the 737 bracket files to a lexer
/// death. Tokenising them is not implementing them: `<| ... |>` and `|x|` still
/// have no parse, and that is deliberate.
#[test]
fn the_enclosing_operator_characters_lex() {
    assert_eq!(
        kinds("<|a|>"),
        vec![Kind::LeftBar, Kind::Ident("a"), Kind::RightBar, Kind::Eof]
    );
    // Longest match: `||` is one token, and `|>` beats `|` followed by `>`.
    assert_eq!(kinds("||"), vec![Kind::BarBar, Kind::Eof]);
    assert_eq!(
        kinds("|self|"),
        vec![Kind::Bar, Kind::KwSelf, Kind::Bar, Kind::Eof]
    );
    assert_eq!(
        kinds("10^(2) 0#1"),
        vec![
            Kind::IntLit {
                text: "10",
                digits: "10".to_owned()
            },
            Kind::Caret,
            Kind::LParen,
            Kind::IntLit {
                text: "2",
                digits: "2".to_owned()
            },
            Kind::RParen,
            Kind::IntLit {
                text: "0",
                digits: "0".to_owned()
            },
            Kind::Hash,
            Kind::IntLit {
                text: "1",
                digits: "1".to_owned()
            },
            Kind::Eof
        ]
    );
}

// ------------------------------------------------------- M3b: brackets

#[test]
fn square_brackets_are_their_own_tokens() {
    assert_eq!(
        kinds("a[0]"),
        vec![
            Kind::Ident("a"),
            Kind::LBracket,
            int("0", "0"),
            Kind::RBracket,
            Kind::Eof
        ]
    );
}

/// `[\` and `\]` are one token each, not a bracket glued to a backslash. They
/// have to win the longest match or `Array[\ZZ64\]` lexes as garbage.
#[test]
fn the_static_argument_brackets_are_single_tokens() {
    assert_eq!(
        kinds("Array[\\ZZ64\\]"),
        vec![
            Kind::Ident("Array"),
            Kind::LGeneric,
            Kind::Ident("ZZ64"),
            Kind::RGeneric,
            Kind::Eof
        ]
    );
}

#[test]
fn an_array_literal_lexes_as_brackets_and_commas() {
    assert_eq!(
        kinds("[1, 2]"),
        vec![
            Kind::LBracket,
            int("1", "1"),
            Kind::Comma,
            int("2", "2"),
            Kind::RBracket,
            Kind::Eof
        ]
    );
}

#[test]
fn while_is_a_keyword_now_that_the_loop_exists() {
    assert_eq!(kinds("while"), vec![Kind::KwWhile, Kind::Eof]);
    // And still not a prefix of an identifier.
    assert_eq!(kinds("whiled"), vec![Kind::Ident("whiled"), Kind::Eof]);
}

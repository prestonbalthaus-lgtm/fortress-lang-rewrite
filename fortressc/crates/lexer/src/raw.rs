use logos::{FilterResult, Lexer, Logos};

use crate::error::LexErrorKind;

/// Accumulated classification of one whitespace run, in the sense of
/// `Spacing.rats:92-96`. Reset by the driver each time a real token is produced.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Gap {
    pub space: bool,
    pub brk: bool,
    /// Byte offset of the first surviving line terminator in the run.
    pub brk_at: usize,
}

impl Gap {
    fn mark_break(&mut self, at: usize) {
        if !self.brk {
            self.brk_at = at;
        }
        self.brk = true;
    }
}

type Skip = FilterResult<(), LexErrorKind>;

/// `clippy::indexing_slicing` is denied workspace-wide, so every scan walks the
/// source through this instead of `s[i..]`.
fn tail(s: &str, i: usize) -> &str {
    s.get(i..).unwrap_or("")
}

const fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Legal inside a comment, an error outside one (`Spacing.rats:34-42`, :68-72).
const fn is_comment_legal_control(c: char) -> bool {
    matches!(c, '\t' | '\u{000B}' | '\u{000C}' | '\u{001C}'..='\u{001F}')
}

/// How a comment scan ended.
enum CommentEnd {
    /// Consumed `n` bytes; `broke` records whether it contained a terminator.
    Done {
        bytes: usize,
        broke: bool,
    },
    Failed(LexErrorKind),
}

/// Scans a block comment whose opening `(*` has already been consumed.
/// Block comments nest, and `(*)` inside one is inert (`Spacing.rats:74-80`).
fn scan_block_comment(rest: &str) -> CommentEnd {
    let mut depth = 1usize;
    let mut broke = false;
    let bytes = rest.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if tail(rest, i).starts_with("(*)") {
            i += 3;
            continue;
        }
        if tail(rest, i).starts_with("(*") {
            depth += 1;
            i += 2;
            continue;
        }
        if tail(rest, i).starts_with("*)") {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return CommentEnd::Done { bytes: i, broke };
            }
            continue;
        }
        let Some(c) = tail(rest, i).chars().next() else {
            break;
        };
        if is_line_terminator(c) {
            broke = true;
        } else if c.is_control() && !is_comment_legal_control(c) {
            return CommentEnd::Failed(LexErrorKind::UnrecognizedCharacter);
        }
        i += c.len_utf8();
    }
    CommentEnd::Failed(LexErrorKind::UnterminatedComment)
}

/// Scans a `(*)` line comment whose opening has already been consumed. Never
/// breaks a line: it stops before the terminator without consuming it. A `(*`
/// opened inside must close on the same line (`Spacing.rats:82-88`).
fn scan_line_comment(rest: &str) -> CommentEnd {
    let mut depth = 0usize;
    let mut i = 0usize;

    while i < rest.len() {
        if tail(rest, i).starts_with("(*)") {
            i += 3;
            continue;
        }
        if tail(rest, i).starts_with("(*") {
            depth += 1;
            i += 2;
            continue;
        }
        if tail(rest, i).starts_with("*)") {
            if depth == 0 {
                return CommentEnd::Done {
                    bytes: i,
                    broke: false,
                };
            }
            depth -= 1;
            i += 2;
            continue;
        }
        let Some(c) = tail(rest, i).chars().next() else {
            break;
        };
        if is_line_terminator(c) {
            return if depth == 0 {
                CommentEnd::Done {
                    bytes: i,
                    broke: false,
                }
            } else {
                CommentEnd::Failed(LexErrorKind::UnclosedCommentInLineComment)
            };
        }
        if c.is_control() && !is_comment_legal_control(c) {
            return CommentEnd::Failed(LexErrorKind::UnrecognizedCharacter);
        }
        i += c.len_utf8();
    }
    if depth == 0 {
        CommentEnd::Done {
            bytes: i,
            broke: false,
        }
    } else {
        CommentEnd::Failed(LexErrorKind::UnclosedCommentInLineComment)
    }
}

fn space_run(lex: &mut Lexer<Raw>) -> Skip {
    lex.extras.space = true;
    FilterResult::Skip
}

fn line_break(lex: &mut Lexer<Raw>) -> Skip {
    let at = lex.span().start;
    lex.extras.mark_break(at);
    FilterResult::Skip
}

fn invalid_whitespace(_lex: &mut Lexer<Raw>) -> Skip {
    FilterResult::Error(LexErrorKind::InvalidWhitespace)
}

fn block_comment(lex: &mut Lexer<Raw>) -> Skip {
    let start = lex.span().end;
    match scan_block_comment(lex.remainder()) {
        CommentEnd::Done { bytes, broke } => {
            lex.bump(bytes);
            lex.extras.space = true;
            if broke {
                lex.extras.mark_break(start);
            }
            FilterResult::Skip
        }
        CommentEnd::Failed(kind) => FilterResult::Error(kind),
    }
}

fn line_comment(lex: &mut Lexer<Raw>) -> Skip {
    match scan_line_comment(lex.remainder()) {
        CommentEnd::Done { bytes, .. } => {
            lex.bump(bytes);
            lex.extras.space = true;
            FilterResult::Skip
        }
        CommentEnd::Failed(kind) => FilterResult::Error(kind),
    }
}

/// The `&` line continuation. `Space = ... / "&" s Whitespace` with possessive
/// repetition is exactly `"&" Space* Newline`, so this cancels one statement
/// terminator and never sets `brk`. Witness: `tests/ampersand.fss:19-20`.
fn continuation(lex: &mut Lexer<Raw>) -> Skip {
    let rest = lex.remainder();
    let mut i = 0usize;

    loop {
        let Some(c) = tail(rest, i).chars().next() else {
            return FilterResult::Error(LexErrorKind::DanglingContinuation);
        };

        if c == ' ' || c == '\u{000C}' {
            i += c.len_utf8();
            continue;
        }
        if is_line_terminator(c) {
            // Consume the terminator. This is the cancellation.
            i += c.len_utf8();
            if c == '\r' && tail(rest, i).starts_with('\n') {
                i += 1;
            }
            lex.bump(i);
            lex.extras.space = true;
            return FilterResult::Skip;
        }
        if tail(rest, i).starts_with("(*)") {
            match scan_line_comment(tail(rest, i + 3)) {
                CommentEnd::Done { bytes, .. } => {
                    i += 3 + bytes;
                    continue;
                }
                CommentEnd::Failed(kind) => return FilterResult::Error(kind),
            }
        }
        if tail(rest, i).starts_with("(*") {
            match scan_block_comment(tail(rest, i + 2)) {
                CommentEnd::Done { bytes, broke } => {
                    i += 2 + bytes;
                    if broke {
                        lex.bump(i);
                        lex.extras.space = true;
                        return FilterResult::Skip;
                    }
                    continue;
                }
                CommentEnd::Failed(kind) => return FilterResult::Error(kind),
            }
        }
        return FilterResult::Error(LexErrorKind::DanglingContinuation);
    }
}

/// `Op ... !(Symbol)` where `Symbol = [+]` (`Symbol.rats:120-121`, :136). `+` is
/// the only character with this adjacency restriction; the wider class is
/// commented out at :137.
fn reject_trailing_plus(lex: &Lexer<Raw>) -> bool {
    lex.remainder().starts_with('+')
}

macro_rules! guarded_op {
    ($name:ident) => {
        fn $name(lex: &mut Lexer<Raw>) -> Skip {
            if reject_trailing_plus(lex) {
                FilterResult::Error(LexErrorKind::OperatorFollowedByPlus)
            } else {
                FilterResult::Emit(())
            }
        }
    };
}

guarded_op!(op_plus);
guarded_op!(op_minus);
guarded_op!(op_star);
guarded_op!(op_slash);
guarded_op!(op_lt);
guarded_op!(op_gt);

macro_rules! always_error {
    ($name:ident, $kind:ident) => {
        fn $name(_lex: &mut Lexer<Raw>) -> Skip {
            FilterResult::Error(LexErrorKind::$kind)
        }
    };
}

always_error!(err_double_star, DoubleStar);
always_error!(err_char_literal, MalformedCharacterLiteral);
always_error!(err_unmatched_close, MismatchedStringMarks);
always_error!(err_non_ascii, NonAsciiCharacter);

/// The four shapes `lexical-structure.tex:862-877` accepts, and a diagnostic
/// naming which of the two remaining ones was written.
///
/// A LONE BACKSLASH IS A STATIC ERROR (:851-852) and so is an unescaped string
/// delimiter (:853-859), which is there to stop ASCII conversion moving the
/// boundaries of a string literal.
fn character_literal(lex: &mut Lexer<Raw>) -> FilterResult<(), LexErrorKind> {
    let slice = lex.slice();
    let body = slice
        .get(1..slice.len().saturating_sub(1))
        .unwrap_or_default();
    if body.chars().any(char::is_control) {
        return FilterResult::Error(LexErrorKind::MalformedCharacterLiteral);
    }
    let mut chars = body.chars();
    match (chars.next(), chars.next()) {
        (Some(BACKSLASH), None) => FilterResult::Error(LexErrorKind::MalformedCharacterLiteral),
        (Some(BACKSLASH), Some(escape)) => {
            let listed = matches!(escape, 'b' | 't' | 'n' | 'f' | 'r' | QUOTE | BACKSLASH);
            if chars.next().is_some() || !listed {
                return FilterResult::Error(LexErrorKind::MalformedCharacterLiteral);
            }
            FilterResult::Emit(())
        }
        (Some(QUOTE), None) => FilterResult::Error(LexErrorKind::MalformedCharacterLiteral),
        (Some(_), None) => FilterResult::Emit(()),
        (Some(_), Some(_)) => {
            if matches!(body, "TAB" | "NEWLINE" | "RETURN") {
                return FilterResult::Emit(());
            }
            // FOUR OR MORE, and the floor is the specification's: fewer digits
            // would collide with the Unicode names, which are also words.
            if body.len() >= HEX_CODE_POINT_DIGITS && body.chars().all(|c| c.is_ascii_hexdigit()) {
                return FilterResult::Emit(());
            }
            FilterResult::Error(LexErrorKind::CharacterNameUnsupported)
        }
        (None, _) => FilterResult::Error(LexErrorKind::MalformedCharacterLiteral),
    }
}

/// `lexical-structure.tex:864-866`: "a sequence of FOUR OR MORE hexadecimal
/// digits". The floor is the specification's and not a tuning knob -- fewer
/// digits would collide with the Unicode names, which are also words. Named so
/// that a mutation can move it in one line.
const HEX_CODE_POINT_DIGITS: usize = 4;

const BACKSLASH: char = '\\';
const QUOTE: char = '"';

fn numeral(lex: &mut Lexer<Raw>) -> FilterResult<(), LexErrorKind> {
    let text = lex.slice();
    if lex.remainder().starts_with('_') {
        return FilterResult::Error(LexErrorKind::RadixNumeralUnsupported);
    }
    if text.chars().filter(|c| *c == '.').count() > 1 {
        return FilterResult::Error(LexErrorKind::MultipleDecimalPoints);
    }
    if text.chars().any(|c| c.is_ascii_alphabetic()) {
        return FilterResult::Error(LexErrorKind::NumeralWithLetters);
    }
    FilterResult::Emit(())
}

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(extras = Gap, error = LexErrorKind)]
pub(crate) enum Raw {
    #[regex(r"[ \u{000C}]+", space_run)]
    #[regex(r"\r\n|[\r\n\u{2028}\u{2029}]", line_break)]
    #[regex(r"[\t\u{000B}\u{001C}-\u{001F}]", invalid_whitespace)]
    #[token("&", continuation)]
    #[token("(*)", line_comment)]
    #[token("(*", block_comment)]
    Trivia,

    #[regex(r"[A-Za-z_][A-Za-z0-9_']*")]
    Word,

    #[regex(r"[0-9][0-9A-Za-z]*(?:['\u{202F}.][0-9A-Za-z]+)*", numeral)]
    Numeral,

    // `Literal.rats:151-155` gives a string literal TWO delimiter pairs, and
    // `ProjectFortress/tests/matchingStringMarks.fss` is a positive test that
    // prints through the curly-quoted one. Decision 3's wording does not reach
    // them either -- a string delimiter is neither an identifier nor an
    // operator, so no library alias can carry it.
    #[token("\"", string_literal)]
    #[token("\u{201C}", curly_string_literal)]
    Str,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    // `[\` and `\]` are listed first so logos prefers them over the bare
    // bracket followed by a backslash.
    // U+27E6/U+27E7 are the Unicode SPELLING of `[\` and `\]`. They are
    // BRACKETS rather than operators, so no library alias can carry them --
    // an alias is a declaration and a bracket is not a name. They are the
    // same token, spelled differently.
    #[token("[\\")]
    #[token("\u{27E6}")]
    LGeneric,
    #[token("\\]")]
    #[token("\u{27E7}")]
    RGeneric,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    /// M3c: `extends {A, B}`. Set and map literals use the same pair, which is
    /// why adding them moves the lexer's corpus number as well.
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(";")]
    Semi,
    // `Symbol.rats:200`: `colonequals = ":=" / "\u2254"`.
    #[token(":=")]
    #[token("\u{2254}")]
    ColonEq,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,

    // `=>` is one token and must be listed before `=`; logos prefers the longer
    // match, which is what stops it lexing as `=` followed by `>`.
    #[token("=>")]
    #[token("\u{21D2}")]
    FatArrow,
    // The enclosing-operator characters. Tokenising them is not the same as
    // implementing enclosing operators: `<| ... |>` and `|x|` still have no
    // parse. What this buys is that a file using them reaches the parser at all.
    #[token("<|")]
    LeftBar,
    #[token("|>")]
    RightBar,
    #[token("||")]
    BarBar,
    #[token("|")]
    Bar,
    /// The bare backslash, which exists ONLY so the enclosing-operator names
    /// `|\self/|` and `|/self\|` reach the parser. `[\` and `\]` stay ahead of
    /// it by logos's longest match, exactly as `=>` stays ahead of `=`, so no
    /// static-parameter list lexes differently for this being here.
    #[token("\\")]
    Backslash,
    #[token("^")]
    Caret,
    #[token("#")]
    Hash,

    /// The six ordinary operator characters of `operator-app.tex:24` that had
    /// no arm at all and fell to `UnrecognizedCharacter`. Every one of them is
    /// a hard lex error today, so a file can only move forward for their
    /// existing. Real declaration sites:
    /// `Library/incomplete/basic/Fortress.Number.fsi:136` `opr (self)! : NN`,
    /// `ProjectFortress/BirdyLib/Bazaar.fsi:22` `opr BIG $()`,
    /// `SpecData/examples/advanced/OprDecl.Nofix.fss:23` `opr @()`.
    #[token("!")]
    Bang,
    #[token("?")]
    Question,
    #[token("~")]
    Tilde,
    #[token("$")]
    Dollar,
    #[token("%")]
    Percent,
    #[token("@")]
    At,

    /// `lexical-structure.tex:1174-1177`: a contiguous sequence of TWO OR MORE
    /// vertical lines is ONE base operator. `||` already was; three or more
    /// split into `BarBar` + `Bar`, which is why `opr |||`
    /// (`Library/FortressLibrary.fsi:1991`) survives in declaration position --
    /// the parser re-glues by span adjacency -- and cannot survive in
    /// expression position. Logos prefers the longest match, so this arm takes
    /// the run and `Bar`, `BarBar`, `LeftBar` and `RightBar` keep every match
    /// they had.
    #[regex(r"\|{3,}")]
    BarRun,

    /// `Symbol.rats:197`: `leftarrow = "<-" / "\u2190"`. The ASCII spelling is
    /// two tokens joined by span adjacency, so the Unicode one cannot reuse it
    /// and needs a token of its own.
    #[token("\u{2190}")]
    LeftArrow,
    /// `->`, the arrow of a function type, whose ASCII spelling is likewise
    /// `Minus` glued to `Gt`.
    #[token("\u{2192}")]
    RightArrow,

    /// THE UNICODE OPERATOR ALLOWLIST. Ten codepoints, and one token for all of
    /// them: 02-stack's decision 3 says mathematical symbols "resolve through
    /// standard library symbol aliasing, never through new lexer tokens", so
    /// each carries its own text and takes its meaning from an `opr`
    /// declaration exactly as `!`, `@` and `SUBSET` do. What is NOT here is
    /// every codepoint the reference grammar lists as an alternative SPELLING
    /// of a token -- those are the same token and are written above.
    ///
    /// Measured, not guessed: these are the ten, and there are no others. Over
    /// all 136 `Library/` and `CompilerLibrary/` files with comments and
    /// strings stripped there are exactly 18 distinct non-ASCII codepoints and
    /// ZERO of them are letters.
    #[regex(r"[\u{00AC}\u{2208}\u{2228}\u{2229}\u{226A}\u{226B}\u{2286}\u{2287}\u{27E8}\u{27E9}]")]
    UniOp,

    #[token("===")]
    EqEqEq,
    #[token("=/=")]
    #[token("\u{2260}")]
    NotEq,
    // `Symbol.rats:214-216` lists each of these as an alternative spelling
    // of the SAME token, which is why they are here and not in the operator
    // allowlist below.
    #[token("<=")]
    #[token("\u{2264}")]
    Le,
    #[token(">=")]
    #[token("\u{2265}")]
    Ge,
    /// `Symbol.rats` has TWO productions for `=`. `equalsOp` is the equality
    /// operator and carries no restriction; `equals = "=" (!op)` at :201 is the
    /// one that introduces a DEFINITION, and the reference grammar reaches it
    /// only from a binding or a keyword-argument position. The guard used to
    /// live here, where it applied to every `=` in the file and made `opr ==>`
    /// and `ex=-1` hard lex errors. It is now `definition_equals_at` in the
    /// parser.
    #[token("=")]
    Eq,
    #[token("<", op_lt)]
    Lt,
    #[token(">", op_gt)]
    Gt,

    #[token("+", op_plus)]
    Plus,
    #[token("-", op_minus)]
    Minus,
    #[token("**", err_double_star)]
    #[token("*", op_star)]
    Star,
    #[token("///")]
    SlashSlashSlash,
    #[token("//")]
    SlashSlash,
    #[token("/", op_slash)]
    Slash,

    /// `lexical-structure.tex:800-877`. THE BODY IS SCANNED HERE AND JUDGED IN
    /// THE CALLBACK, because the four shapes the specification accepts -- one
    /// character, an escape, FOUR OR MORE hex digits, and `TAB`/`NEWLINE`/
    /// `RETURN` -- cannot be told from the two it refuses by a regex that
    /// still names which was written.
    ///
    /// ONE CHARACTER BEFORE THE CLOSER IS THE SPECIFICATION'S OWN RULE, :836-842:
    /// the literal ends at the nearest apostrophe AFTER the first enclosed
    /// character, so ''' is one literal holding an apostrophe. A line
    /// terminator may not be enclosed at all (:844-846), which is what bounds
    /// the scan.
    #[regex(
        r"'[^\n\r\u{2028}\u{2029}][^'\n\r\u{2028}\u{2029}]*'",
        character_literal
    )]
    CharLit,

    #[token("'", err_char_literal)]
    #[token("\u{2018}", err_char_literal)]
    #[token("\u{2019}", err_char_literal)]
    // A closing curly quote with no opener. The opener is a string literal
    // above, so reaching this one means the marks do not match.
    #[token("\u{201D}", err_unmatched_close)]
    // Excludes every non-ASCII character handled by a specific rule above,
    // otherwise logos reports the patterns as ambiguous.
    #[regex(
        r"[^\x00-\x7F\u{00AC}\u{2018}\u{2019}\u{201C}\u{201D}\u{2028}\u{2029}\u{2190}\u{2192}\u{2208}\u{21D2}\u{2228}\u{2229}\u{2254}\u{2260}\u{2264}\u{2265}\u{226A}\u{226B}\u{2286}\u{2287}\u{27E6}\u{27E7}\u{27E8}\u{27E9}]",
        err_non_ascii
    )]
    Rejected,
}

/// Scans a string literal whose opening `"` has already been consumed. A string
/// never spans a source line and `&` continuation does not apply inside one
/// (`Literal.rats:169-196`).
fn string_literal(lex: &mut Lexer<Raw>) -> Skip {
    scan_string(lex, '"', '\u{201D}')
}

/// The curly-quoted spelling, `Literal.rats:151-155`. The marks must MATCH: the
/// grammar has an explicit error production for each mixed pair, and the corpus
/// has a positive test for the matched form and a must-fail twin for the other.
fn curly_string_literal(lex: &mut Lexer<Raw>) -> Skip {
    scan_string(lex, '\u{201D}', '"')
}

fn scan_string(lex: &mut Lexer<Raw>, closer: char, wrong_closer: char) -> Skip {
    let rest = lex.remainder();
    let mut i = 0usize;

    while let Some(c) = tail(rest, i).chars().next() {
        match c {
            c if c == closer => {
                lex.bump(i + c.len_utf8());
                return FilterResult::Emit(());
            }
            // `InvalidStringLiteralContent` (`Literal.rats:174-175`) makes an
            // unescaped curly quote illegal inside ANY string, so the only
            // thing this can be is the wrong closing mark.
            c if c == wrong_closer || c == '\u{201C}' => {
                return FilterResult::Error(LexErrorKind::MismatchedStringMarks)
            }
            '\\' => {
                let Some(e) = tail(rest, i + 1).chars().next() else {
                    return FilterResult::Error(LexErrorKind::UnterminatedStringLiteral);
                };
                if !matches!(
                    e,
                    'b' | 't' | 'n' | 'f' | 'r' | '"' | '\\' | '\u{201C}' | '\u{201D}'
                ) {
                    return FilterResult::Error(LexErrorKind::InvalidEscape);
                }
                i += 1 + e.len_utf8();
            }
            c if is_line_terminator(c) => {
                return FilterResult::Error(LexErrorKind::RawLineTerminatorInString)
            }
            c if c.is_control() => {
                return FilterResult::Error(LexErrorKind::RawLineTerminatorInString)
            }
            c => i += c.len_utf8(),
        }
    }
    FilterResult::Error(LexErrorKind::UnterminatedStringLiteral)
}

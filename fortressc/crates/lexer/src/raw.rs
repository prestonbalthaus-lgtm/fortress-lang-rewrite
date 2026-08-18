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
always_error!(err_char_literal, CharacterLiteralUnsupported);
always_error!(err_curly_quote, CurlyQuoteStringUnsupported);
always_error!(err_non_ascii, NonAsciiCharacter);

/// `equals = "=" (!op)` (`Symbol.rats:201`). `===`, `=/=`, `<=` and `>=` are
/// matched as longer tokens first, so a bare `=` glued to an operator character
/// is malformed rather than two tokens.
fn op_equals(lex: &mut Lexer<Raw>) -> Skip {
    match lex.remainder().chars().next() {
        Some('+') => FilterResult::Error(LexErrorKind::OperatorFollowedByPlus),
        Some('>') => FilterResult::Error(LexErrorKind::FatArrowUnsupported),
        Some('-' | '*' | '/' | '<' | '=' | ':' | '!') => {
            FilterResult::Error(LexErrorKind::MalformedEquals)
        }
        _ => FilterResult::Emit(()),
    }
}

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

    #[token("\"", string_literal)]
    Str,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    // `[\` and `\]` are listed first so logos prefers them over the bare
    // bracket followed by a backslash.
    #[token("[\\")]
    LGeneric,
    #[token("\\]")]
    RGeneric,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(";")]
    Semi,
    #[token(":=")]
    ColonEq,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,

    #[token("===")]
    EqEqEq,
    #[token("=/=")]
    NotEq,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("=", op_equals)]
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

    #[token("'", err_char_literal)]
    #[token("\u{2018}", err_char_literal)]
    #[token("\u{2019}", err_char_literal)]
    #[token("\u{201C}", err_curly_quote)]
    #[token("\u{201D}", err_curly_quote)]
    // Excludes every non-ASCII character handled by a specific rule above,
    // otherwise logos reports the patterns as ambiguous.
    #[regex(
        r"[^\x00-\x7F\u{2018}\u{2019}\u{201C}\u{201D}\u{2028}\u{2029}]",
        err_non_ascii
    )]
    Rejected,
}

/// Scans a string literal whose opening `"` has already been consumed. A string
/// never spans a source line and `&` continuation does not apply inside one
/// (`Literal.rats:169-196`).
fn string_literal(lex: &mut Lexer<Raw>) -> Skip {
    let rest = lex.remainder();
    let mut i = 0usize;

    while let Some(c) = tail(rest, i).chars().next() {
        match c {
            '"' => {
                lex.bump(i + 1);
                return FilterResult::Emit(());
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

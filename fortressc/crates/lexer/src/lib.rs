//! Tokenizer plus the newline and layout layer.
//!
//! Whitespace is not uniformly ignorable in Fortress: juxtaposition carries
//! meaning, so the parser recovers adjacency from byte spans, and newlines
//! terminate statements, so they are emitted as tokens.
//!
//! Rules here are derived from `ProjectFortress/src/com/sun/fortress/parser/*.rats`;
//! the plan document cites file and line for each.

mod error;
mod raw;
mod token;

pub use error::{LexError, LexErrorKind};
pub use token::{Kind, Token};

use fortress_ast::Span;
use logos::Logos as _;

const NUMERAL_SEPARATORS: [char; 2] = ['\'', '\u{202F}'];

/// Tokenizes `source`. Fails on the first error rather than recovering, per the
/// M1 design.
///
/// The returned stream always ends in [`Kind::Eof`] and never contains a
/// [`Kind::Newline`] in leading or trailing position.
pub fn lex(source: &str) -> Result<Vec<Token<'_>>, LexError> {
    let mut raw = raw::Raw::lexer(source);
    let mut out: Vec<Token<'_>> = Vec::new();
    let mut have_previous = false;

    while let Some(result) = raw.next() {
        let span = Span::new(raw.span().start, raw.span().end);
        let kind_raw = result.map_err(|kind| LexError::new(kind, span))?;
        let gap = core::mem::take(&mut raw.extras);

        if gap.brk && have_previous {
            out.push(Token::new(Kind::Newline, Span::new(gap.brk_at, gap.brk_at)));
        }

        let slice = raw.slice();
        let kind = match kind_raw {
            raw::Raw::Trivia | raw::Raw::Rejected => continue,
            raw::Raw::Word => token::classify_word(slice),
            raw::Raw::Numeral => numeral_kind(slice),
            raw::Raw::Str => Kind::StrLit(decode_string(slice)),
            raw::Raw::LParen => Kind::LParen,
            raw::Raw::RParen => Kind::RParen,
            raw::Raw::LBracket => Kind::LBracket,
            raw::Raw::RBracket => Kind::RBracket,
            raw::Raw::LGeneric => Kind::LGeneric,
            raw::Raw::RGeneric => Kind::RGeneric,
            raw::Raw::Comma => Kind::Comma,
            raw::Raw::Semi => Kind::Semi,
            raw::Raw::Colon => Kind::Colon,
            raw::Raw::ColonEq => Kind::ColonEq,
            raw::Raw::Dot => Kind::Dot,
            raw::Raw::Eq => Kind::Eq,
            raw::Raw::EqEqEq => Kind::EqEqEq,
            raw::Raw::NotEq => Kind::NotEq,
            raw::Raw::Lt => Kind::Lt,
            raw::Raw::Gt => Kind::Gt,
            raw::Raw::Le => Kind::Le,
            raw::Raw::Ge => Kind::Ge,
            raw::Raw::Plus => Kind::Plus,
            raw::Raw::Minus => Kind::Minus,
            raw::Raw::Star => Kind::Star,
            raw::Raw::Slash => Kind::Slash,
            raw::Raw::SlashSlash => Kind::SlashSlash,
            raw::Raw::SlashSlashSlash => Kind::SlashSlashSlash,
        };

        out.push(Token::new(kind, span));
        have_previous = true;
    }

    out.push(Token::new(Kind::Eof, Span::new(source.len(), source.len())));
    Ok(out)
}

/// Group separators are deleted before the value is computed, so `1'000'000`
/// and `1000000` are the same numeral (`ExprFactory.java:612-613`).
fn numeral_kind(text: &str) -> Kind<'_> {
    let stripped: String = text
        .chars()
        .filter(|c| !NUMERAL_SEPARATORS.contains(c))
        .collect();
    match stripped.split_once('.') {
        None => Kind::IntLit {
            text,
            digits: stripped,
        },
        Some((int_part, frac_part)) => Kind::FloatLit {
            text,
            int_digits: int_part.to_owned(),
            frac_digits: frac_part.to_owned(),
        },
    }
}

/// `slice` still carries its delimiters. Escapes are exactly the seven ASCII
/// forms plus the two curly quotes (`Literal.rats:182-196`); the scanner has
/// already rejected anything else.
fn decode_string(slice: &str) -> String {
    let body = slice.strip_prefix('"').unwrap_or(slice);
    let body = body.strip_suffix('"').unwrap_or(body);

    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('b') => out.push('\u{0008}'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('f') => out.push('\u{000C}'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => break,
        }
    }
    out
}

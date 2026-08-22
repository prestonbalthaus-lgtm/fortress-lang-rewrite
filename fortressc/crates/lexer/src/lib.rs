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

pub(crate) const NUMERAL_SEPARATORS: [char; 2] = ['\'', '\u{202F}'];

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
            raw::Raw::CharLit => Kind::CharLit(decode_char(slice)),
            raw::Raw::Str => Kind::StrLit(decode_string(slice)),
            raw::Raw::LParen => Kind::LParen,
            raw::Raw::RParen => Kind::RParen,
            raw::Raw::LBracket => Kind::LBracket,
            raw::Raw::RBracket => Kind::RBracket,
            raw::Raw::LBrace => Kind::LBrace,
            raw::Raw::RBrace => Kind::RBrace,
            raw::Raw::LGeneric => Kind::LGeneric,
            raw::Raw::RGeneric => Kind::RGeneric,
            raw::Raw::Comma => Kind::Comma,
            raw::Raw::Semi => Kind::Semi,
            raw::Raw::Colon => Kind::Colon,
            raw::Raw::ColonEq => Kind::ColonEq,
            raw::Raw::Dot => Kind::Dot,
            raw::Raw::Eq => Kind::Eq,
            raw::Raw::FatArrow => Kind::FatArrow,
            raw::Raw::LeftBar => Kind::LeftBar,
            raw::Raw::RightBar => Kind::RightBar,
            raw::Raw::BarBar => Kind::BarBar,
            raw::Raw::Bar => Kind::Bar,
            raw::Raw::Backslash => Kind::Backslash,
            raw::Raw::Caret => Kind::Caret,
            raw::Raw::Hash => Kind::Hash,
            raw::Raw::Bang => Kind::Bang,
            raw::Raw::Question => Kind::Question,
            raw::Raw::Tilde => Kind::Tilde,
            raw::Raw::Dollar => Kind::Dollar,
            raw::Raw::Percent => Kind::Percent,
            raw::Raw::At => Kind::At,
            raw::Raw::BarRun => Kind::BarRun(slice),
            raw::Raw::LeftArrow => Kind::LeftArrow,
            raw::Raw::RightArrow => Kind::RightArrow,
            raw::Raw::UniOp => Kind::UniOp(slice),
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
/// A radix numeral to the DECIMAL DIGIT STRING every other literal carries.
///
/// ARBITRARY PRECISION, AND THAT IS NOT GOLD PLATING. Accumulating into a
/// `u128` panicked the LEXER on a forty-digit hexadecimal literal -- exit 101 on
/// user-supplied source, which this compiler's rules forbid outright -- and in a
/// release build it would have WRAPPED instead, silently. The plain decimal path
/// hands its digits through untouched and lets the checker say `integer literal
/// does not fit in ZZ32`; this makes the radix path reach the same diagnostic
/// rather than a different fate.
///
/// `raw::digit_value` and NOT `from_str_radix`: `X` is ten and `E` is eleven at
/// radix twelve, which no standard parser knows. The scanner has already refused
/// every digit at or above the radix, so `None` here is unreachable.
fn to_decimal(clean: &str, radix: u32) -> Option<String> {
    if clean.is_empty() {
        return None;
    }
    // Little-endian decimal digits, multiplied and added one source digit at a
    // time. No allocation per digit and no ceiling but memory.
    let mut out: Vec<u32> = vec![0];
    for c in clean.chars() {
        let mut carry = raw::digit_value(c, radix)?;
        for place in &mut out {
            let n = *place * radix + carry;
            *place = n.rem_euclid(10);
            carry = n.div_euclid(10);
        }
        while carry > 0 {
            out.push(carry.rem_euclid(10));
            carry = carry.div_euclid(10);
        }
    }
    while out.len() > 1 && out.last() == Some(&0) {
        out.pop();
    }
    Some(
        out.iter()
            .rev()
            .map(|d| char::from_digit(*d, 10).unwrap_or('0'))
            .collect(),
    )
}

fn numeral_kind(text: &str) -> Kind<'_> {
    // A RADIX LITERAL IS DECODED HERE AND CARRIED AS DECIMAL DIGITS, so nothing
    // downstream needs to know a base existed: the parser turns `digits` into a
    // value and `7FFF_16` and `32767` reach it identically.
    if let Some((digits, specifier)) = text.rsplit_once('_') {
        if let Some(radix) = raw::radix_of(specifier) {
            let clean = digits.replace(NUMERAL_SEPARATORS, "");
            if let Some(decimal) = to_decimal(&clean, radix) {
                return Kind::IntLit {
                    text,
                    digits: decimal,
                };
            }
        }
    }
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
/// `'a'` to `a`, sharing the escape table with a string literal because
/// `Literal.rats` gives them the same one. The regex has already guaranteed
/// exactly one character or one escape, so the fallback is unreachable rather
/// than a guess.
fn decode_char(slice: &str) -> char {
    let body = slice
        .get(1..slice.len().saturating_sub(1))
        .unwrap_or_default();
    let mut chars = body.chars();
    match (chars.next(), chars.next()) {
        (Some('\\'), Some(escape)) => match escape {
            'b' => '\u{0008}',
            't' => '\t',
            'n' => '\n',
            'f' => '\u{000C}',
            'r' => '\r',
            other => other,
        },
        (Some(c), None) => c,
        // `character_literal` in the raw lexer has already refused every body
        // that is not one of these, so the fallbacks below are unreachable
        // rather than a guess.
        _ => match body {
            "TAB" => '\t',
            "NEWLINE" => '\n',
            "RETURN" => '\r',
            digits => u32::from_str_radix(digits, 16)
                .ok()
                .and_then(char::from_u32)
                .unwrap_or('\u{FFFD}'),
        },
    }
}

fn decode_string(slice: &str) -> String {
    // Either delimiter pair; `Literal.rats:151-155` gives the two the same
    // content and the scanner has already refused a mixed pair.
    let body = slice
        .strip_prefix('"')
        .or_else(|| slice.strip_prefix('\u{201C}'))
        .unwrap_or(slice);
    let body = body
        .strip_suffix('"')
        .or_else(|| body.strip_suffix('\u{201D}'))
        .unwrap_or(body);

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

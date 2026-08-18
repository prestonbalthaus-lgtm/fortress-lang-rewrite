use fortress_ast::Span;

/// `logos` requires the error type to be `Default`; the default variant is what
/// an unmatched character produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LexErrorKind {
    #[default]
    UnrecognizedCharacter,
    /// Tab, vertical tab, or U+001C..U+001F outside a comment.
    InvalidWhitespace,
    /// `&` not followed, after spacing only, by a line terminator.
    DanglingContinuation,
    UnterminatedComment,
    /// A `(*` opened inside a `(*)` line comment and never closed on that line.
    UnclosedCommentInLineComment,
    UnterminatedStringLiteral,
    RawLineTerminatorInString,
    InvalidEscape,
    /// A radix-less numeral containing letters, e.g. `2x` or `1e10`.
    NumeralWithLetters,
    MultipleDecimalPoints,
    /// `**` is not two `*` tokens.
    DoubleStar,
    /// Any operator immediately followed by `+`.
    OperatorFollowedByPlus,
    /// `=` immediately followed by an operator character that formed no known token.
    MalformedEquals,
    NonAsciiCharacter,
    CharacterLiteralUnsupported,
    RadixNumeralUnsupported,
    CurlyQuoteStringUnsupported,
}

impl LexErrorKind {
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::UnrecognizedCharacter => "unrecognized character",
            Self::InvalidWhitespace => {
                "tab characters are not allowed in Fortress programs except in comments"
            }
            Self::DanglingContinuation => {
                "`&` must be followed, after spacing only, by a line terminator"
            }
            Self::UnterminatedComment => "unbalanced comment: `*)` expected",
            Self::UnclosedCommentInLineComment => {
                "`(*` opened inside a `(*)` line comment must close on the same line"
            }
            Self::UnterminatedStringLiteral => "unterminated string literal",
            Self::RawLineTerminatorInString => "a string literal may not span a source line",
            Self::InvalidEscape => "unknown escape sequence",
            Self::NumeralWithLetters => {
                "a numeral contains letters and does not have a radix specifier"
            }
            Self::MultipleDecimalPoints => "a numeral contains more than one `.` character",
            Self::DoubleStar => "`**` is not a valid operator in Fortress",
            Self::OperatorFollowedByPlus => "an operator may not be immediately followed by `+`",
            Self::MalformedEquals => "`=` is followed by an operator character",
            Self::NonAsciiCharacter => {
                "non-ASCII characters are not in the M1 subset outside comments and strings"
            }
            Self::CharacterLiteralUnsupported => "character literals are not in the M1 subset",
            Self::RadixNumeralUnsupported => "radix numerals are not in the M1 subset",
            Self::CurlyQuoteStringUnsupported => {
                "curly-quote string delimiters are not in the M1 subset; use `\"`"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexError {
    pub kind: LexErrorKind,
    pub span: Span,
}

impl LexError {
    #[must_use]
    pub const fn new(kind: LexErrorKind, span: Span) -> Self {
        Self { kind, span }
    }
}

impl core::fmt::Display for LexError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}..{}: {}",
            self.span.start,
            self.span.end,
            self.kind.message()
        )
    }
}

impl std::error::Error for LexError {}

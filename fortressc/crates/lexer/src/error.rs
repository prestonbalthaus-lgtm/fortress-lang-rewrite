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
    NonAsciiCharacter,
    /// A `'` that does not open a well-formed character literal: unterminated,
    /// empty, or holding a character the specification forbids there.
    /// `lexical-structure.tex:844-853` makes a line terminator, a forbidden
    /// character and a lone backslash static errors, and
    /// `ProjectFortress/parser_tests/XXXforbiddenCharacters.fss` writes a raw
    /// tab for exactly that reason.
    MalformedCharacterLiteral,
    /// A character literal NAMING a character rather than writing one:
    /// `'PLUS-MINUS SIGN'`, or an ASCII sequence ASCII conversion would fold.
    /// `lexical-structure.tex:869-877` -- a PREPROCESSING feature with a table
    /// of Unicode names behind it, refused by name rather than guessed at.
    CharacterNameUnsupported,
    RadixNumeralUnsupported,
    /// A string opened with one mark and closed with the other.
    /// `Literal.rats:158-167` has an explicit error production for each
    /// mixed pair, and the corpus has a must-fail test for it.
    MismatchedStringMarks,
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
            Self::NonAsciiCharacter => {
                "non-ASCII characters are not in the M1 subset outside comments and strings"
            }
            Self::MalformedCharacterLiteral => {
                "a character literal holds one character, an escape, four or more hex digits, \
                 or TAB, NEWLINE or RETURN"
            }
            Self::CharacterNameUnsupported => {
                "naming a character inside a character literal is not in the M1 subset"
            }
            Self::RadixNumeralUnsupported => "radix numerals are not in the M1 subset",
            Self::MismatchedStringMarks => {
                "the opening and closing marks of a string literal must match"
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

    /// The same shape `ParseError::span` has, so the driver's renderer takes
    /// all three error types through one call.
    #[must_use]
    pub const fn span(&self) -> Option<Span> {
        Some(self.span)
    }
}

impl core::fmt::Display for LexError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.kind.message())
    }
}

impl std::error::Error for LexError {}

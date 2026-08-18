use fortress_ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    UnexpectedToken {
        span: Span,
        expected: &'static str,
        found: String,
    },
    UnexpectedEndOfInput {
        expected: &'static str,
    },
    /// `x- 1`: glued on the left, spaced on the right. A postfix operator
    /// followed by a juxtaposition, which is real Fortress and outside M1.
    PostfixOperatorUnsupported {
        span: Span,
    },
    /// One of the 70 reserved words the parser does not act on.
    ReservedWord {
        span: Span,
        word: String,
    },
    /// `trait Foo[\T\]`. Generics are M3d; refusing here is better than
    /// parsing a static parameter list the rest of the compiler ignores.
    StaticParametersUnsupported {
        span: Span,
    },
}

impl ParseError {
    #[must_use]
    pub const fn span(&self) -> Option<Span> {
        match self {
            Self::UnexpectedToken { span, .. }
            | Self::PostfixOperatorUnsupported { span }
            | Self::ReservedWord { span, .. }
            | Self::StaticParametersUnsupported { span } => Some(*span),
            Self::UnexpectedEndOfInput { .. } => None,
        }
    }
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedToken {
                span,
                expected,
                found,
            } => {
                write!(
                    f,
                    "{}..{}: expected {expected}, found {found}",
                    span.start, span.end
                )
            }
            Self::UnexpectedEndOfInput { expected } => {
                write!(f, "unexpected end of input, expected {expected}")
            }
            Self::PostfixOperatorUnsupported { span } => write!(
                f,
                "{}..{}: a postfix operator followed by a juxtaposition is not in the M1 subset",
                span.start, span.end
            ),
            Self::ReservedWord { span, word } => {
                write!(
                    f,
                    "{}..{}: reserved word `{word}` is not in the implemented subset",
                    span.start, span.end
                )
            }
            Self::StaticParametersUnsupported { span } => write!(
                f,
                "{}..{}: static parameters `[\\...\\]` on a declaration are not implemented; generics are M3d",
                span.start, span.end
            ),
        }
    }
}

impl std::error::Error for ParseError {}

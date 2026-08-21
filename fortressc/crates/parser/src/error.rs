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
    /// `[\nat n\]`. M3d is type parameters only: mixing static integers with
    /// type parameters is a dependent type system, and this is not one.
    StaticParameterKindUnsupported {
        span: Span,
        kind: String,
    },
    /// `f(x) = e` in block position. `=` is an equality operator in expression
    /// position, so without this the declaration would parse as a discarded
    /// comparison rather than fail.
    LocalFunctionDeclarationUnsupported {
        span: Span,
    },
    /// `a <= b > c`. `chained-multifix.tex:16-34` restricts a chain to a
    /// mixture of equivalence operators and ordering operators of one sense.
    ChainedOperatorsDiffer {
        span: Span,
        first: &'static str,
        second: &'static str,
    },
}

impl ParseError {
    #[must_use]
    pub const fn span(&self) -> Option<Span> {
        match self {
            Self::UnexpectedToken { span, .. }
            | Self::PostfixOperatorUnsupported { span }
            | Self::ReservedWord { span, .. }
            | Self::StaticParameterKindUnsupported { span, .. }
            | Self::LocalFunctionDeclarationUnsupported { span }
            | Self::ChainedOperatorsDiffer { span, .. } => Some(*span),
            Self::UnexpectedEndOfInput { .. } => None,
        }
    }
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedToken {
                expected, found, ..
            } => {
                write!(f, "expected {expected}, found {found}")
            }
            Self::UnexpectedEndOfInput { expected } => {
                write!(f, "unexpected end of input, expected {expected}")
            }
            Self::PostfixOperatorUnsupported { .. } => f.write_str(
                "a postfix operator followed by a juxtaposition is not in the M1 subset",
            ),
            Self::ReservedWord { word, .. } => {
                write!(f, "reserved word `{word}` is not in the implemented subset")
            }
            Self::StaticParameterKindUnsupported { kind, .. } => write!(
                f,
                "`{kind}` static parameters are not implemented; M3d is type parameters only"
            ),
            Self::LocalFunctionDeclarationUnsupported { .. } => f.write_str(
                "a local function declaration is not implemented; declare it at component level",
            ),
            Self::ChainedOperatorsDiffer { first, second, .. } => write!(
                f,
                "a chain mixes `{first}` with `{second}`; \
                 chained ordering operators must have the same sense"
            ),
        }
    }
}

impl std::error::Error for ParseError {}

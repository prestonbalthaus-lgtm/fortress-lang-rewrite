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
    /// `object O(x: ZZ32...)`. `objects.tex:100` is
    /// `ObjectVarargs ::= transient Varargs`, so an object's varargs parameter
    /// must carry `transient`; :66 eliminates both from Basic Fortress
    /// outright. Two corpus files write the modifier-less form and both are
    /// must-FAIL tests.
    ObjectVarargsParameter {
        span: Span,
        name: String,
    },
    /// `trait Stream ... end WriteStream`. `TraitObject.rats:13` permits the
    /// declaration's own name after `end`; a DIFFERENT name is a static error,
    /// and accepting one silently would be a new wrong acceptance rather than
    /// a new feature.
    ClosingNameDiffers {
        span: Span,
        found: String,
        expected: String,
    },
    /// `a + b CUP c`. `precedence.tex:20-31` makes Fortress precedence a
    /// PARTIAL relation: "if there is no specific precedence relationship
    /// between two operators, then parentheses must be used". A total ladder
    /// can only ever accept, so the alternative to this diagnostic is a silent
    /// grouping the program never asked for.
    OperatorsUnrelated {
        span: Span,
        first: String,
        second: String,
    },
    /// `a SUBSET-b`. `opr-fixity.tex:34-55` calls an infix operator with
    /// whitespace on one side and not the other a static error outright; the
    /// rule of thumb at :100-102 is that an infix operator may be loose or
    /// tight but not LOPSIDED.
    LopsidedOperator {
        span: Span,
        name: String,
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
            | Self::ChainedOperatorsDiffer { span, .. }
            | Self::ObjectVarargsParameter { span, .. }
            | Self::ClosingNameDiffers { span, .. }
            | Self::OperatorsUnrelated { span, .. }
            | Self::LopsidedOperator { span, .. } => Some(*span),
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
            Self::StaticParameterKindUnsupported { span, kind } => write!(
                f,
                "{}..{}: `{kind}` static parameters are not implemented; \
                 M3d is type parameters only",
                span.start, span.end
            ),
            Self::LocalFunctionDeclarationUnsupported { span } => write!(
                f,
                "{}..{}: a local function declaration is not implemented; \
                 declare it at component level",
                span.start, span.end
            ),
            Self::ChainedOperatorsDiffer {
                span,
                first,
                second,
            } => write!(
                f,
                "{}..{}: a chain mixes `{first}` with `{second}`; \
                 chained ordering operators must have the same sense",
                span.start, span.end
            ),
            Self::ObjectVarargsParameter { span, name } => write!(
                f,
                "{}..{}: the object value parameter `{name}` is varargs; an \
                 object's varargs parameter must be declared `transient`",
                span.start, span.end
            ),
            Self::ClosingNameDiffers {
                span,
                found,
                expected,
            } => write!(
                f,
                "{}..{}: `end {found}` closes a declaration named `{expected}`",
                span.start, span.end
            ),
            Self::OperatorsUnrelated {
                span,
                first,
                second,
            } => write!(
                f,
                "{}..{}: `{first}` and `{second}` have no precedence relationship; \
                 write the parentheses",
                span.start, span.end
            ),
            Self::LopsidedOperator { span, name } => write!(
                f,
                "{}..{}: `{name}` has whitespace on one side and not the other; \
                 an infix operator must be loose or tight, not lopsided",
                span.start, span.end
            ),
        }
    }
}

impl std::error::Error for ParseError {}

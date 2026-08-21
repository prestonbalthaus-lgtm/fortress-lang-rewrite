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
    /// `case most > of` and `case z IN of`. Both replace `=` as the comparison
    /// the arms are matched with, and both need an operator table to look the
    /// replacement up in.
    CaseFormUnsupported {
        span: Span,
        form: &'static str,
    },
    /// `fn n => e` and `fn(a, b) => e`. A lambda whose parameters carry no
    /// written type: they would have to come from the arrow the lambda lands
    /// in, which is a fact the checker holds and the parser does not.
    LambdaFormUnsupported {
        span: Span,
        form: &'static str,
    },
    /// A BIG reduction this lowering does not reach: an operator other than
    /// SUM, PROD, MAX and MIN, or a generator that is not a range. Recognised
    /// so that it is refused by name rather than read as a subscript.
    BigReductionUnsupported {
        span: Span,
        name: String,
        reason: &'static str,
    },
    /// An `also` block form outside the subset. `at` is the only one: regions
    /// are shelved with the cluster work, and a lowering that silently dropped
    /// the prefix would be the open-set mistake `comprises { ... }` already
    /// records.
    AlsoFormUnsupported {
        span: Span,
        form: &'static str,
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
            | Self::CaseFormUnsupported { span, .. }
            | Self::LambdaFormUnsupported { span, .. }
            | Self::BigReductionUnsupported { span, .. }
            | Self::AlsoFormUnsupported { span, .. } => Some(*span),
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
            Self::LambdaFormUnsupported { span, form } => write!(
                f,
                "{}..{}: `fn` with {form} is not implemented; write \
                 `fn (x: T): R => ...` with every parameter typed",
                span.start, span.end
            ),
            Self::BigReductionUnsupported { span, name, reason } => {
                write!(f, "{}..{}: `{name}` {reason}", span.start, span.end)
            }
            Self::AlsoFormUnsupported { span, form } => write!(
                f,
                "{}..{}: {form} is not implemented; regions are shelved, and \
                 dropping the prefix would change where the block runs without \
                 saying so",
                span.start, span.end
            ),
            Self::CaseFormUnsupported { span, form } => write!(
                f,
                "{}..{}: {form} replaces the `=` a case arm is matched with, \
                 and there is no operator table to look the replacement up in",
                span.start, span.end
            ),
        }
    }
}

impl std::error::Error for ParseError {}

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
    /// `[\unit u\]`, `[\dim d\]`, `[\opr PLUS\]`. THE THREE KINDS D7 DID NOT
    /// OPEN -- `nat`, `int` and `bool` parse now, so this variant no longer
    /// speaks for them. `unit` and `dim` are D7 §3.3, sub-phase 4d, gated on
    /// SPIKE-COMPOSITE-TYPE; `opr` is D7 §4 and is SPIKE-OPEXPR's, because a
    /// name in OPERATOR position is not arithmetic.
    StaticParameterKindUnsupported {
        span: Span,
        kind: String,
    },
    /// A bound on a `nat`/`int`/`bool` parameter. D7 leaves the constraint
    /// solver out of v1 and its own census is the reason: NOT ONE
    /// `where { k < n }` exists in 1956 files.
    StaticValueParameterBound {
        span: Span,
        name: String,
    },
    /// An integer literal in static-argument position that does not fit `i64`.
    /// Arbitrary-precision `ZZ` is its own spike -- it needs a heap
    /// representation and runtime shims -- so this is a diagnostic and not a
    /// silent wrap.
    StaticExpressionOutOfRange {
        span: Span,
        digits: String,
    },
    /// `f(x) = e` in block position. `=` is an equality operator in expression
    /// position, so without this the declaration would parse as a discarded
    /// comparison rather than fail.
    LocalFunctionDeclarationUnsupported {
        span: Span,
    },
    /// A `where` clause form outside v1. The clause used to be a TOKEN SKIP --
    /// brace-matched and thrown away -- so `where { this is total garbage }`
    /// compiled, linked and ran, and a bound written in a where clause was a
    /// silent no-op while the identical bound in the bracket list was enforced.
    ///
    /// v1 restricts a where clause to constraints over the static parameters
    /// the declaration WRITES. The form that introduces fresh static variables
    /// (`where [\ ... \]`, `trait-parameters.tex:312-316`) is refused by name:
    /// it binds statics SEMANTICALLY, and M3d's locked rule is that static
    /// arguments are written and never inferred, with expansion running to a
    /// fixpoint before `Checker::new`. That phase split is load bearing.
    WhereClauseFormUnsupported {
        span: Span,
        form: String,
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
    /// `import java com.sun.fortress.nativeHelpers.{...}`. 39 corpus files
    /// write it. Three are bootstrap files whose bodies reach the JVM this way
    /// and have no other implementation in the tree -- and those three are
    /// C-shim work, not import work. What phase 3 owes the construct is a
    /// diagnostic that names it.
    ForeignImportUnsupported {
        span: Span,
    },
    /// `x ||= e`, `x MAX= e`. `lexical-structure.tex:1216-1222` makes an
    /// operator immediately followed by `=` ONE token, a compound assignment
    /// operator. `||=` alone is 37 corpus uses. Without this the operator level
    /// reads the `||` and reports it as a LOPSIDED infix -- a real rule, but
    /// not the one the program broke, and a diagnostic that names the wrong
    /// mechanism is a defect class this project tracks.
    CompoundAssignmentUnsupported {
        span: Span,
        op: String,
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
            | Self::StaticValueParameterBound { span, .. }
            | Self::StaticExpressionOutOfRange { span, .. }
            | Self::LocalFunctionDeclarationUnsupported { span }
            | Self::WhereClauseFormUnsupported { span, .. }
            | Self::ChainedOperatorsDiffer { span, .. }
            | Self::CaseFormUnsupported { span, .. }
            | Self::LambdaFormUnsupported { span, .. }
            | Self::BigReductionUnsupported { span, .. }
            | Self::AlsoFormUnsupported { span, .. }
            | Self::ObjectVarargsParameter { span, .. }
            | Self::ClosingNameDiffers { span, .. }
            | Self::OperatorsUnrelated { span, .. }
            | Self::LopsidedOperator { span, .. }
            | Self::ForeignImportUnsupported { span }
            | Self::CompoundAssignmentUnsupported { span, .. } => Some(*span),
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
                "`{kind}` static parameters are not implemented; `nat`, `int` and `bool` \
                 are, and `unit`/`dim` wait on composite types while `opr` waits on the \
                 operator grammar"
            ),
            Self::StaticValueParameterBound { name, .. } => write!(
                f,
                "`{name}` is a value static parameter and carries a bound; there is no \
                 constraint solver, and no corpus file writes one"
            ),
            Self::StaticExpressionOutOfRange { digits, .. } => write!(
                f,
                "the static argument `{digits}` does not fit a 64 bit integer; \
                 arbitrary-precision `ZZ` is a separate milestone"
            ),
            Self::LocalFunctionDeclarationUnsupported { .. } => f.write_str(
                "a local function declaration is not implemented; declare it at component level",
            ),
            Self::WhereClauseFormUnsupported { form, .. } => write!(
                f,
                "a `where` clause may only constrain a static parameter this declaration \
                 declares; {form}"
            ),
            Self::ChainedOperatorsDiffer { first, second, .. } => write!(
                f,
                "a chain mixes `{first}` with `{second}`; \
                 chained ordering operators must have the same sense"
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
            Self::ForeignImportUnsupported { span } => write!(
                f,
                "{}..{}: a foreign import reaches a JVM implementation and this \
                 compiler emits native code; the body belongs in a C shim",
                span.start, span.end
            ),
            Self::CompoundAssignmentUnsupported { span, op } => write!(
                f,
                "{}..{}: the compound assignment operator `{op}=` is not in the \
                 implemented subset",
                span.start, span.end
            ),
        }
    }
}

impl std::error::Error for ParseError {}

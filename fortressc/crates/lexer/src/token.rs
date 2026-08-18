use fortress_ast::Span;

/// The 90 reserved words of `Keyword.rats:21-49`. Eighteen are acted on by the
/// parser, `true`/`false` become literals, and the remaining 70 are reserved so
/// the namespace stays closed: reserving late would be a breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind<'a> {
    /// Statement terminator. Carries a zero-width span at the first line
    /// terminator of the whitespace run that produced it.
    Newline,
    Eof,

    KwComponent,
    KwExport,
    KwEnd,
    KwDo,
    KwIf,
    KwThen,
    KwElse,
    KwElif,
    KwWhile,

    /// M3c. `api` is the separate-compilation form, parsed so the corpus
    /// metric moves; the checker refuses to compile one as a program.
    KwApi,
    KwTrait,
    KwObject,
    KwExtends,
    KwComprises,
    KwExcludes,
    KwWhere,
    KwVar,
    /// The receiver of a dotted method. Method bodies are parsed and not
    /// checked, so this never reaches the type checker.
    KwSelf,

    /// One of the other 80 reserved words. The parser rejects these with
    /// "not in the M1 subset".
    Reserved(&'a str),
    Ident(&'a str),

    True,
    False,
    /// `digits` has every group separator removed; `text` is the source slice.
    IntLit {
        text: &'a str,
        digits: String,
    },
    /// Split at the single `.`, separators removed. The value is exact, not an
    /// IEEE double: there is no exponent syntax in Fortress.
    FloatLit {
        text: &'a str,
        int_digits: String,
        frac_digits: String,
    },
    /// Escapes already decoded.
    StrLit(String),

    LParen,
    RParen,
    /// `[` and `]`: an array literal, or an index glued to what it indexes.
    LBracket,
    RBracket,
    /// `{` and `}`: the `extends {A, B}` list, and the set and map literals
    /// that are not in the subset yet but do have to lex.
    LBrace,
    RBrace,
    /// `[\` and `\]`, the static argument brackets of `Array[\ZZ64\]`.
    LGeneric,
    RGeneric,
    Comma,
    Semi,
    Colon,
    Dot,

    /// Serves both the definition `=` and the equality operator; the parser
    /// disambiguates by position.
    Eq,
    ColonEq,
    Plus,
    Minus,
    Star,
    Slash,
    Lt,
    Gt,
    Le,
    Ge,
    /// `===`. There is no `==` in Fortress.
    EqEqEq,
    /// `=/=`.
    NotEq,
    /// `//` is an operator, never a comment opener.
    SlashSlash,
    /// `///`.
    SlashSlashSlash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: Kind<'a>,
    pub span: Span,
}

impl<'a> Token<'a> {
    #[must_use]
    pub const fn new(kind: Kind<'a>, span: Span) -> Self {
        Self { kind, span }
    }
}

/// The 79 reserved words outside the implemented subset, sorted for binary
/// search.
pub(crate) const RESERVED: [&str; 70] = [
    "BIG",
    "FORALL",
    "SI_unit",
    "Self",
    "Zilch",
    "absorbs",
    "abstract",
    "also",
    "asif",
    "at",
    "atomic",
    "bool",
    "case",
    "catch",
    "coerce",
    "coerces",
    "contravariant",
    "covariant",
    "default",
    "dim",
    "dominates",
    "ensures",
    "except",
    "exit",
    "finally",
    "fn",
    "for",
    "forbid",
    "getter",
    "goto",
    "grammar",
    "hidden",
    "idiom",
    "import",
    "int",
    "invariant",
    "io",
    "label",
    "most",
    "nat",
    "native",
    "of",
    "opr",
    "or",
    "override",
    "private",
    "property",
    "provided",
    "public",
    "pure",
    "reciprocal",
    "requires",
    "settable",
    "setter",
    "spawn",
    "static",
    "syntax",
    "test",
    "throw",
    "throws",
    "try",
    "tryatomic",
    "type",
    "typecase",
    "typed",
    "unit",
    "value",
    "widens",
    "with",
    "wrapped",
];

pub(crate) fn classify_word(word: &str) -> Kind<'_> {
    match word {
        "component" => Kind::KwComponent,
        "export" => Kind::KwExport,
        "end" => Kind::KwEnd,
        "do" => Kind::KwDo,
        "if" => Kind::KwIf,
        "then" => Kind::KwThen,
        "else" => Kind::KwElse,
        "elif" => Kind::KwElif,
        "while" => Kind::KwWhile,
        "api" => Kind::KwApi,
        "trait" => Kind::KwTrait,
        "object" => Kind::KwObject,
        "extends" => Kind::KwExtends,
        "comprises" => Kind::KwComprises,
        "excludes" => Kind::KwExcludes,
        "where" => Kind::KwWhere,
        "var" => Kind::KwVar,
        "self" => Kind::KwSelf,
        "true" => Kind::True,
        "false" => Kind::False,
        _ if RESERVED.binary_search(&word).is_ok() => Kind::Reserved(word),
        _ => Kind::Ident(word),
    }
}

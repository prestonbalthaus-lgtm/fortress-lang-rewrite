use fortress_ast::Span;

/// The 90 reserved words of `Keyword.rats:21-49`. Twenty are acted on by the
/// parser, `true`/`false` become literals, and the remaining 66 are reserved so
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
    /// M3d's lexer pass. Imports parse and are recorded; nothing reads them
    /// until separate compilation exists, which whole-program monomorphization
    /// deliberately does not have.
    KwImport,
    KwExcept,
    /// The receiver of a dotted method. Method bodies are parsed and not
    /// checked, so this never reaches the type checker.
    KwSelf,
    /// Member modifiers. A getter is read without an argument list and a setter
    /// is written to; both are declared like a method and only the invocation
    /// syntax differs, so the parser treats them as modifiers on one.
    KwGetter,
    KwSetter,

    /// One of the other 66 reserved words. The parser rejects these with
    /// "not in the M1 subset".
    Reserved(&'a str),
    Ident(&'a str),
    /// An OPERATOR WORD. `lexical-structure.tex:1167-1172`: a word that is not
    /// reserved, consists only of uppercase letters and underscores, does not
    /// begin or end with an underscore, and has at least two DIFFERENT letters.
    ///
    /// Those three clauses are each load bearing. No digits keeps `ZZ32` and
    /// `RR64` identifiers; two different letters keeps `ZZ`, `QQ` and `RR`; the
    /// underscore rule keeps `CT_`. `BIG` and `FORALL` are already reserved and
    /// the reserved test runs first, as the specification's "is not reserved"
    /// requires.
    ///
    /// M1's lexer plan deferred this deliberately. It stopped being deferrable
    /// when `a SUBSET b` was found to parse as a three-element juxtaposition
    /// and fold with multiplication: `SUBSET: ZZ64 = 2` then
    /// `println(3 SUBSET 4)` printed 24.
    OpWord(&'a str),

    True,
    False,
    /// `digits` has every group separator removed; `text` is the source slice.
    /// `'a'`, decoded. A Unicode scalar, so the lexer has already applied the
    /// escape and the parser never sees a backslash.
    CharLit(char),
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
    /// `=>`. A map entry, a case arm, and an import alias.
    FatArrow,
    /// `<|` and `|>`, the list-literal enclosers, and the bare `|` they are
    /// built from. Lexed, not parsed: enclosing operators need a precedence
    /// map, and that is not this milestone.
    LeftBar,
    RightBar,
    BarBar,
    Bar,
    /// `\`. Only ever part of an enclosing-operator name -- `[\` and `\]` are
    /// their own tokens and win the longest match.
    Backslash,
    /// `^`, exponentiation, and `#`, which the library uses as an operator.
    Caret,
    Hash,
    /// The six ordinary operator characters that had no token at all:
    /// `! ? ~ $ % @`. Tokenising them is not implementing them.
    Bang,
    Question,
    Tilde,
    Dollar,
    Percent,
    At,
    /// A contiguous run of THREE OR MORE vertical lines, which
    /// `lexical-structure.tex:1174-1177` makes one base operator. Two is
    /// `BarBar`; the slice is carried because the run has no fixed length.
    BarRun(&'a str),
    /// `<-` written as U+2190, and `->` as U+2192. Both ASCII spellings are two
    /// tokens joined by span adjacency, so the Unicode ones cannot reuse them.
    LeftArrow,
    RightArrow,
    /// An allowlisted Unicode operator character, carrying its own text. There
    /// is ONE token for all ten because 02-stack's decision 3 says mathematical
    /// symbols resolve through library aliasing rather than through a lexer
    /// token each: an `opr` declaration is what gives one meaning.
    UniOp(&'a str),
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

/// The reserved words outside the implemented subset, sorted for binary
/// search.
///
/// SEVEN OF THESE ARE THE UNIT OPERATORS -- `cubed`, `cubic`, `in`, `inverse`,
/// `per`, `square`, `squared` (`dimensions.tex:32`, `:49-54`). Reserving them
/// FIXED A LIVE WRONG ANSWER rather than merely reserving a name: with `in` an
/// ordinary identifier, `println(x in nm)` over three `RR64` bindings was a
/// three-way juxtaposition PRODUCT and printed `7.8`, at exit 0, with no
/// diagnostic anywhere. Retroactive invalidation was measured before the
/// reclassification, which is this project's own rule: ZERO of the 394 files
/// that compile use any of the seven as a name, with comments and strings
/// stripped.
pub(crate) const RESERVED: [&str; 73] = [
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
    "cubed",
    "cubic",
    "default",
    "dim",
    "dominates",
    "ensures",
    "exit",
    "finally",
    "fn",
    "for",
    "forbid",
    "goto",
    "grammar",
    "hidden",
    "idiom",
    "in",
    "int",
    "invariant",
    "inverse",
    "io",
    "label",
    "most",
    "nat",
    "native",
    "of",
    "opr",
    "or",
    "override",
    "per",
    "private",
    "property",
    "provided",
    "public",
    "pure",
    "reciprocal",
    "requires",
    "settable",
    "spawn",
    "square",
    "squared",
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
        "getter" => Kind::KwGetter,
        "setter" => Kind::KwSetter,
        "import" => Kind::KwImport,
        "except" => Kind::KwExcept,
        "true" => Kind::True,
        "false" => Kind::False,
        _ if RESERVED.binary_search(&word).is_ok() => Kind::Reserved(word),
        _ if is_operator_word(word) => Kind::OpWord(word),
        _ => Kind::Ident(word),
    }
}

/// `lexical-structure.tex:1167-1172`. The caller has already ruled out the
/// reserved words, which is the specification's own first clause.
fn is_operator_word(word: &str) -> bool {
    if !word.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
        return false;
    }
    if word.starts_with('_') || word.ends_with('_') {
        return false;
    }
    let mut letters = word.bytes().filter(|b| *b != b'_');
    let Some(first) = letters.next() else {
        return false;
    };
    letters.any(|b| b != first)
}

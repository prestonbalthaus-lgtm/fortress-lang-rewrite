//! The M1 AST. Types only: `parser` builds these and `codegen` consumes them,
//! so neither depends on the other.

use crate::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub exports: Vec<String>,
    /// Recorded and not read. Whole-program monomorphization has no separate
    /// compilation, so there is nothing for an import to resolve against yet.
    pub imports: Vec<ImportDecl>,
    pub decls: Vec<Decl>,
    /// Empty until monomorphization has run.
    pub bounds: Vec<BoundObligation>,
    /// `api Foo ... end` rather than `component Foo ... end`. Parsed so the
    /// corpus metric can move; an api has no bodies and is not executable, so
    /// the type checker refuses it rather than pretending to compile one.
    pub is_api: bool,
    pub span: Span,
}

/// `import Foo.Bar.{...}`. The name is kept; the brace group and any `except`
/// clause are consumed without being interpreted, because aliasing an operator
/// (`opr OPLUS => MYPLUS`) needs a precedence map that does not exist yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub api_name: String,
    /// `import api Foo` rather than `import Foo.{...}`.
    pub is_api: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decl {
    Function(FnDecl),
    Trait(TraitDecl),
    Object(ObjectDecl),
}
/// The declaration modifiers this parser ingests. They are RECORDED AND NOT
/// READ: M6's spike is the grammar, and the semantics of value types, native
/// bodies and access control are each a milestone of their own.
///
/// `abstract` in particular is stored and NOT consulted, on purpose. M3c
/// already decides abstractness from `body.is_none()` -- that is what keeps an
/// unimplemented abstract method out of the dispatch table -- so wiring the
/// flag in would give one fact two sources that could disagree.
///
/// `atomic` and `io` are deliberately NOT here. Both are real modifiers in 1.0
/// with real meaning, both are refused today with a diagnostic that names them,
/// and swallowing them here would turn a named deviation into a silent one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// `abstract opr <(self, other: T): Boolean`.
    pub abstract_: bool,
    /// `value object CaseInsensitiveString(s: String)`.
    pub value: bool,
    /// `native component File`, and a member whose body lives in C.
    pub native: bool,
    /// `private scale(x: ZZ32): ZZ32 = ...`.
    pub private: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {
    pub modifiers: Modifiers,
    pub name: String,
    pub static_params: Vec<StaticParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    /// `None` only inside an `api`, where a declaration is a signature.
    pub body: Option<Expr>,
    /// True when the source wrote a component-level value binding (`pi: RR64 =
    /// 3.14`) rather than a function. Both parse into this node because there
    /// is no value declaration node yet, and the checker refuses the binding: a
    /// value's initializer runs at component initialization, while a nullary
    /// function's body runs when it is called -- which, for a name nothing can
    /// reference, is never.
    pub value_binding: bool,
    pub span: Span,
}

/// `trait T extends {A, B} comprises {...} excludes {...} ... end`.
/// Only `extends` is checked; the rest is recorded and ignored, because
/// exclusion is decided extensionally from the concrete types in the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitDecl {
    pub modifiers: Modifiers,
    pub name: String,
    pub static_params: Vec<StaticParam>,
    pub extends: Vec<TypeRef>,
    pub comprises: Vec<TypeRef>,
    pub excludes: Vec<TypeRef>,
    pub members: Vec<Member>,
    pub span: Span,
}

/// `object O(x: T) extends {A} ... end`, or without the parentheses, a
/// singleton: one instance, built once before `run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectDecl {
    pub modifiers: Modifiers,
    pub name: String,
    pub static_params: Vec<StaticParam>,
    /// `None` is a singleton. `Some(vec![])` is `object O() ... end`.
    pub params: Option<Vec<Param>>,
    pub extends: Vec<TypeRef>,
    /// An object cannot `comprises` in 1.0, but it may `excludes`, and both
    /// clauses are read by one loop shared with `trait` -- so `comprises` is
    /// carried rather than special-cased away at the parser.
    pub comprises: Vec<TypeRef>,
    pub excludes: Vec<TypeRef>,
    pub members: Vec<Member>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Member {
    Field(FieldDecl),
    /// A method. The specification gives dotted and functional methods
    /// separate namespaces -- `x.f(y)` is not `f(x, y)` -- so which one this
    /// is comes from whether it declares a `self` parameter, and it is never
    /// desugared into the other.
    Method(MethodDecl),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: TypeRef,
    pub init: Option<Expr>,
    pub mutable: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodDecl {
    pub modifiers: Modifiers,
    pub name: String,
    pub static_params: Vec<StaticParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Option<Expr>,
    /// Declared `getter` or `setter`. An accessor is reached by `o.size`, not
    /// by `o.size()`, so it is not a dotted method call and is left out of the
    /// dispatch sets -- keeping M3h's position that it parses and is not read.
    pub accessor: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

/// Types are bare names (`ZZ32`), a name applied to static arguments
/// (`Map[\ZZ64, List[\String\]\]`), the unit type `()`, a tuple of two or more,
/// or an arrow. Resolution happens in the types crate; the parser only records
/// what was written. After monomorphization no `TypeRef` in a component has
/// static arguments -- expansion rewrites every one to a ground name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRef {
    Named {
        name: String,
        args: Vec<TypeRef>,
        span: Span,
    },
    /// `()`. The specification's special type, pronounced void; not a tuple.
    Unit { span: Span },
    /// Two or more, by construction. A one-element parenthesised list is
    /// unwrapped by the parser and can never arrive here.
    Tuple { elems: Vec<TypeRef>, span: Span },
    /// `A -> B`, right associative. Parsed, never resolved: this subset has no
    /// function values, so an arrow type is uninhabited.
    Arrow {
        from: Box<TypeRef>,
        to: Box<TypeRef>,
        span: Span,
    },
}

impl TypeRef {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Named { span, .. }
            | Self::Unit { span }
            | Self::Tuple { span, .. }
            | Self::Arrow { span, .. } => *span,
        }
    }

    /// The type as the user wrote it, for diagnostics.
    #[must_use]
    pub fn written(&self) -> String {
        match self {
            Self::Named { name, args, .. } if args.is_empty() => name.clone(),
            Self::Named { name, args, .. } => {
                let inner: Vec<String> = args.iter().map(Self::written).collect();
                format!("{name}[\\{}\\]", inner.join(", "))
            }
            Self::Unit { .. } => "()".to_owned(),
            Self::Tuple { elems, .. } => {
                let inner: Vec<String> = elems.iter().map(Self::written).collect();
                format!("({})", inner.join(", "))
            }
            Self::Arrow { from, to, .. } => format!("{} -> {}", from.written(), to.written()),
        }
    }
}

/// `[\T extends {Foo, Bar}\]`. Type parameters only: `nat`, `int`, `bool`,
/// `opr`, `unit` and `dim` are refused by the parser, because mixing static
/// integers with type parameters is a dependent type system and this is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticParam {
    pub name: String,
    pub bounds: Vec<TypeRef>,
    pub span: Span,
}

/// Recorded by monomorphization, discharged by the type checker. A bound cannot
/// be checked while it is being substituted -- subtyping needs the registry, and
/// the registry is built from the ground component expansion produces -- so the
/// obligation is carried across the phase boundary instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundObligation {
    pub subject: TypeRef,
    pub bound: TypeRef,
    /// The static parameter this came from, for the diagnostic.
    pub parameter: String,
    /// `(owner type, mangled method name)` when the obligation came from an
    /// over-approximated method stamp. A generic dotted method is stamped into
    /// every type declaring one of that name, because expansion cannot see the
    /// receiver's type; a bound failing on such a stamp means the guess was
    /// wrong, not that the program is. The checker prunes that stamp instead of
    /// refusing the component.
    pub speculative: Option<(String, String)>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    /// `^`. 1.0 puts it above every other operator, including tight
    /// juxtaposition, and makes it LEFT associative -- `2^3^4` is `(2^3)^4`.
    Pow,
    /// `AND` and `OR`, the short-circuit boolean operators. They are infix
    /// nodes for one reason -- one expression walk rather than two -- and they
    /// are the only `BinOp`s whose right operand may not be evaluated. The
    /// checker turns each into an `if`, which is where the branch comes from.
    And,
    Or,
}

/// Decided from byte-span adjacency, not from a token. Tight juxtaposition
/// binds tighter than tight `/`, which binds tighter than loose juxtaposition,
/// which binds tighter than any loose infix operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixity {
    /// No whitespace on either side: `x-1`.
    Tight,
    /// Whitespace on both sides: `x - 1`.
    Loose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
    /// `NOT`. A prefix operator, which 1.0 places above every infix operator,
    /// so `NOT a AND b` is `(NOT a) AND b`.
    Not,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// `()`. The one value of the unit type.
    Unit {
        span: Span,
    },
    /// `(a, b)`. Two or more, by construction.
    Tuple {
        items: Vec<Expr>,
        span: Span,
    },
    /// Digits with group separators already removed. Arbitrary precision at
    /// this stage: the types crate decides `ZZ32` versus `ZZ64`.
    IntLit {
        digits: String,
        span: Span,
    },
    FloatLit {
        int_digits: String,
        frac_digits: String,
        span: Span,
    },
    StrLit {
        value: String,
        span: Span,
    },
    BoolLit {
        value: bool,
        span: Span,
    },
    Var {
        name: String,
        span: Span,
    },

    /// An unresolved run of juxtaposed operands. Whether this is multiplication
    /// or string concatenation depends on operand types, so it stays flat until
    /// the types crate folds it. The reference implementation does the same.
    Juxt {
        items: Vec<Expr>,
        span: Span,
    },

    Infix {
        op: BinOp,
        fixity: Fixity,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Prefix {
        op: UnOp,
        operand: Box<Expr>,
        span: Span,
    },
    /// A tight application: the `(` is glued to the callee.
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
        span: Span,
    },
    Block {
        items: Vec<BlockItem>,
        span: Span,
    },
    /// `[1, 2, 3]`. Homogeneous and one dimensional; the element type comes
    /// from the elements, or from context when the literal is empty.
    ArrayLit {
        items: Vec<Expr>,
        span: Span,
    },
    /// `a[i]`, a tight subscript. Spaced, `a [i]` is a juxtaposition, exactly
    /// as `f (x)` is.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    While {
        cond: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    /// `x.f`. A field read, or -- under a glued `(` -- the receiver of a
    /// dotted method call, which the checker refuses.
    Field {
        base: Box<Expr>,
        name: String,
        span: Span,
    },
    /// `for i <- lo#count do body end`. 1.0 makes iterations parallel unless
    /// every generator is `seq(...)`, so `sequential` records which was
    /// written rather than leaving it to a later guess.
    For {
        binder: String,
        /// The lower bound, and the count or upper bound, already separated by
        /// the parser: `a:b` is inclusive and `a#n` is a count.
        lo: Box<Expr>,
        hi: Box<Expr>,
        /// True for `a:b`, false for `a#n`. The checker turns both into a
        /// half-open range so codegen only ever sees one shape.
        inclusive: bool,
        sequential: bool,
        body: Box<Expr>,
        span: Span,
    },
    /// `f[\ZZ64\]` in expression position. Monomorphization rewrites this to a
    /// plain `Var` naming the instantiation, so nothing downstream sees it.
    Instantiate {
        callee: Box<Expr>,
        args: Vec<TypeRef>,
        span: Span,
    },
    /// `atomic do ... end`, and `atomic <statement>`, which the parser wraps
    /// into the same one-item block so there is one node here rather than two.
    Atomic {
        body: Box<Expr>,
        span: Span,
    },
    /// `case subject of guard => e ... else => e end`. The subject is evaluated
    /// ONCE -- the checker binds it before the chain -- because a guard chain
    /// that re-evaluates it runs its side effects once per arm.
    ///
    /// The extremum form (`case most > of`) and the operator form
    /// (`case z IN of`) are refused by name in the parser: both need an
    /// operator table that does not exist.
    Case {
        subject: Box<Expr>,
        arms: Vec<CaseArm>,
        /// `else => e`. Required when the value is used, because there is no
        /// value to produce when nothing matches; 1.0 throws `MatchFailure`
        /// there and this subset has no exceptions.
        else_arm: Option<Box<Expr>>,
        span: Span,
    },
    /// `typecase subject of T => e ... else => e end`, and `x: T => e` to bind
    /// the subject at the narrowed type.
    TypeCase {
        subject: Box<Expr>,
        arms: Vec<TypeCaseArm>,
        /// REQUIRED. A trait's concrete types are a compile-time fact, but
        /// `comprises` is not enforced anywhere in this compiler, so an
        /// exhaustiveness proof drawn from it would rest on an unchecked
        /// clause.
        else_arm: Box<Expr>,
        span: Span,
    },
    /// `label L ... end L`. A forward jump within one function, so it needs no
    /// unwinding: the exits are the incoming edges of one phi.
    Label {
        name: String,
        body: Box<Expr>,
        span: Span,
    },
    /// `do A also do B also do C end`. `also.tex:17-21` makes each block an
    /// implicit thread of one group, and the group completes when all of them
    /// do.
    ///
    /// EVERY BLOCK MUST HAVE TYPE `()` and so does the group -- `also.tex:24-27`
    /// -- which is what makes serialising it legal rather than merely
    /// convenient: `parallelism.tex:88-90` permits an implementation to
    /// serialise any group of implicit threads, and with no value to combine
    /// there is nothing else the group could have meant.
    AlsoDo {
        blocks: Vec<Expr>,
        span: Span,
    },
    /// `SUM[i <- lo:hi] e`, and the same for `PROD`. A BIG reduction over a
    /// RANGE, which `reductions.tex:60-77` desugars to
    /// `do var r = identity; for i <- lo:hi do r OP= e end; r end` -- a shape
    /// M5's reduction pipeline already implements, so this needs no `Reduction`
    /// trait, no generator protocol and no closure.
    ///
    /// It is a NODE rather than a parser desugaring for one reason: the
    /// accumulator's type is the BODY's type, and the parser does not know it.
    /// `SUM[i <- 1:10] i` accumulates ZZ64, because a loop binder is ZZ64.
    BigReduction {
        /// `Add` for SUM, `Mul` for PROD. The identity and the merge both come
        /// from it.
        op: BinOp,
        binder: String,
        lo: Box<Expr>,
        hi: Box<Expr>,
        inclusive: bool,
        sequential: bool,
        body: Box<Expr>,
        span: Span,
    },
    /// `fn (x: T): R => e`. An anonymous function, and the FIRST expression in
    /// this language whose value is a function.
    ///
    /// It is lowered, not represented: closure lowering mints a generated
    /// object whose `apply` is this body and whose CONSTRUCTOR PARAMETERS are
    /// the names the body captures -- so a captured name is read inside `apply`
    /// exactly as a field is, by its own spelling, with no environment struct
    /// and no fat pointer.
    Lambda {
        params: Vec<Param>,
        /// `None` when the source wrote none; the arrow the lambda lands in
        /// supplies it, and there is no other inference.
        return_type: Option<TypeRef>,
        body: Box<Expr>,
        span: Span,
    },
    /// `exit L with e`, `exit L`, and `exit`, which names the innermost label.
    Exit {
        /// `None` is the innermost enclosing label.
        name: Option<String>,
        value: Option<Box<Expr>>,
        span: Span,
    },
}

/// One arm of a `case`. The guard is compared with the subject using `=`; the
/// operator form that would let the comparison be anything else is out of the
/// subset until there is an operator table to look it up in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseArm {
    pub guard: Expr,
    pub body: Expr,
    pub span: Span,
}

/// One arm of a `typecase`. `binder` is the `x` of `x: T => e`, bound to the
/// subject at type `T` for the body only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeCaseArm {
    pub binder: Option<String>,
    pub ty: TypeRef,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockItem {
    Binding(Binding),
    Assign(Assign),
    Expr(Expr),
}

/// `x := e` or `a[i] := e`. The target is checked in the types crate, which is
/// where a good diagnostic can be written for `f(x) := 1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assign {
    pub target: Expr,
    /// `None` for `:=`; `Some(op)` for `x op= e`, and it stays folded that way
    /// through the whole checker. The moment `l += e` becomes `l := l + e` the
    /// target counts as READ, which is reduction.tex:35's third condition, and
    /// every reduction in the program disqualifies itself. Only codegen
    /// splits it. This is exactly where the reference implementation lost the
    /// feature: Operators.scala:521-529 desugars in the typechecker and
    /// CodeGen.java:1682-1688 throws on whatever compound form survives.
    pub op: Option<BinOp>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    pub ty: Option<TypeRef>,
    pub value: Expr,
    /// `:=` rather than `=`. A mutable binding is the only thing assignment can
    /// target, and the only thing codegen puts in an `alloca`.
    pub mutable: bool,
    pub span: Span,
}

impl Expr {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Unit { span }
            | Self::Tuple { span, .. }
            | Self::IntLit { span, .. }
            | Self::FloatLit { span, .. }
            | Self::StrLit { span, .. }
            | Self::BoolLit { span, .. }
            | Self::Var { span, .. }
            | Self::Juxt { span, .. }
            | Self::Infix { span, .. }
            | Self::Prefix { span, .. }
            | Self::Call { span, .. }
            | Self::If { span, .. }
            | Self::Block { span, .. }
            | Self::ArrayLit { span, .. }
            | Self::Index { span, .. }
            | Self::While { span, .. }
            | Self::Field { span, .. }
            | Self::For { span, .. }
            | Self::Atomic { span, .. }
            | Self::Case { span, .. }
            | Self::TypeCase { span, .. }
            | Self::Label { span, .. }
            | Self::Lambda { span, .. }
            | Self::BigReduction { span, .. }
            | Self::AlsoDo { span, .. }
            | Self::Exit { span, .. }
            | Self::Instantiate { span, .. } => *span,
        }
    }
}

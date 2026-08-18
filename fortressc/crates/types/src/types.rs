//! The typed AST. Every operator and call in here names one concrete target,
//! so codegen never asks a type question.

use fortress_ast::Span;

/// What an array holds. A separate enum from [`Type`] so that [`Type`] stays
/// `Copy` without boxing, and so that "array of array" is unrepresentable
/// rather than merely rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elem {
    ZZ32,
    ZZ64,
    RR64,
    Boolean,
    String,
}

impl Elem {
    #[must_use]
    pub const fn as_type(self) -> Type {
        match self {
            Self::ZZ32 => Type::ZZ32,
            Self::ZZ64 => Type::ZZ64,
            Self::RR64 => Type::RR64,
            Self::Boolean => Type::Boolean,
            Self::String => Type::String,
        }
    }

    #[must_use]
    pub const fn of(ty: Type) -> Option<Self> {
        match ty {
            Type::ZZ32 => Some(Self::ZZ32),
            Type::ZZ64 => Some(Self::ZZ64),
            Type::RR64 => Some(Self::RR64),
            Type::Boolean => Some(Self::Boolean),
            Type::String => Some(Self::String),
            Type::Void | Type::Array(_) => None,
        }
    }

    /// Storage width in the array's data block. `Boolean` is stored as a byte;
    /// everything else is its natural machine width.
    #[must_use]
    pub const fn bytes(self) -> u64 {
        match self {
            Self::ZZ32 => 4,
            Self::Boolean => 1,
            Self::ZZ64 | Self::RR64 | Self::String => 8,
        }
    }

    /// Whether a slot holds a pointer. The runtime fills those with the empty
    /// string rather than a null, so an unwritten element is still a String.
    #[must_use]
    pub const fn is_pointer(self) -> bool {
        matches!(self, Self::String)
    }

    #[must_use]
    pub const fn symbol(self) -> &'static str {
        self.as_type().symbol()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    ZZ32,
    ZZ64,
    RR64,
    Boolean,
    String,
    Void,
    /// One dimensional and homogeneous. Nesting arrives with generics.
    Array(Elem),
}

impl Type {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ZZ32 => "ZZ32",
            Self::ZZ64 => "ZZ64",
            Self::RR64 => "RR64",
            Self::Boolean => "Boolean",
            Self::String => "String",
            Self::Void => "()",
            Self::Array(Elem::ZZ32) => "Array[\\ZZ32\\]",
            Self::Array(Elem::ZZ64) => "Array[\\ZZ64\\]",
            Self::Array(Elem::RR64) => "Array[\\RR64\\]",
            Self::Array(Elem::Boolean) => "Array[\\Boolean\\]",
            Self::Array(Elem::String) => "Array[\\String\\]",
        }
    }

    /// Lowercase form used to build target symbols like `add_zz64_zz64`.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::ZZ32 => "zz32",
            Self::ZZ64 => "zz64",
            Self::RR64 => "rr64",
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Void => "void",
            Self::Array(Elem::ZZ32) => "array_zz32",
            Self::Array(Elem::ZZ64) => "array_zz64",
            Self::Array(Elem::RR64) => "array_rr64",
            Self::Array(Elem::Boolean) => "array_boolean",
            Self::Array(Elem::String) => "array_string",
        }
    }

    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(self, Self::ZZ32 | Self::ZZ64)
    }

    #[must_use]
    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::ZZ32 | Self::ZZ64 | Self::RR64)
    }

    /// Whether `self` could be reached from `from` by a widening conversion.
    /// Used only to tell the user that `widen` exists; it is never applied.
    #[must_use]
    pub const fn is_widening_of(self, from: Self) -> bool {
        matches!(
            (from, self),
            (Self::ZZ32, Self::ZZ64) | (Self::ZZ32 | Self::ZZ64, Self::RR64)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

impl ArithOp {
    const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::Div => "div",
        }
    }
}

impl CompareOp {
    const fn symbol(self) -> &'static str {
        match self {
            Self::Lt => "lt",
            Self::Gt => "gt",
            Self::Le => "le",
            Self::Ge => "ge",
            Self::Eq => "eq",
            Self::Ne => "ne",
        }
    }
}

/// The MPI surface. Four calls, no arguments, no communicator: `MPI_COMM_WORLD`
/// is a macro whose expansion differs between OpenMPI and MPICH, so it is never
/// named in generated code. It lives in `runtime/mpi_shims.c` and nowhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpiOp {
    Init,
    CommRank,
    CommSize,
    Finalize,
}

impl MpiOp {
    /// The Fortress spelling, used for diagnostics.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Init => "mpiInit",
            Self::CommRank => "mpiCommRank",
            Self::CommSize => "mpiCommSize",
            Self::Finalize => "mpiFinalize",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "mpiInit" => Some(Self::Init),
            "mpiCommRank" => Some(Self::CommRank),
            "mpiCommSize" => Some(Self::CommSize),
            "mpiFinalize" => Some(Self::Finalize),
            _ => None,
        }
    }

    #[must_use]
    pub const fn returns(self) -> Type {
        match self {
            Self::Init | Self::Finalize => Type::Void,
            Self::CommRank | Self::CommSize => Type::ZZ32,
        }
    }

    /// The C symbol. The `fortress_mpi_` prefix keeps these clear of `libmpi`'s
    /// own `MPI_*` symbols and of any Fortran-linkage name a user picks.
    const fn symbol(self) -> &'static str {
        match self {
            Self::Init => "fortress_mpi_init",
            Self::CommRank => "fortress_mpi_comm_rank",
            Self::CommSize => "fortress_mpi_comm_size",
            Self::Finalize => "fortress_mpi_finalize",
        }
    }
}

/// The array runtime. Three entry points: one allocates, one reports the
/// length, and one turns an index into the address of a slot after checking it.
/// Everything else about an array is a typed load or store in generated code.
pub const ARRAY_ALLOC: &str = "fortress_array_alloc";
pub const ARRAY_LENGTH: &str = "fortress_array_length";
pub const ARRAY_SLOT: &str = "fortress_array_slot";

/// A statically chosen implementation. `symbol()` is the name codegen emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Arith {
        op: ArithOp,
        ty: Type,
    },
    Compare {
        op: CompareOp,
        ty: Type,
    },
    Negate {
        ty: Type,
    },
    /// `widen`, the only numeric conversion, and it is never inserted implicitly.
    Widen {
        from: Type,
        to: Type,
    },
    /// String conversion inserted by string juxtaposition. Not a widening: it
    /// is what the concatenation operator is defined to do.
    ToString {
        from: Type,
    },
    Concat,
    Println {
        ty: Type,
    },
    /// A function declared in this component.
    UserFn {
        name: String,
    },
    Mpi(MpiOp),
    /// `array(n)`. The element type is not in the symbol: the runtime is told
    /// the element width and whether it holds pointers, and that is all it
    /// needs to know.
    ArrayNew {
        elem: Elem,
    },
    ArrayLength,
}

impl Target {
    #[must_use]
    pub fn symbol(&self) -> String {
        match self {
            Self::Arith { op, ty } => format!("{}_{}_{}", op.symbol(), ty.symbol(), ty.symbol()),
            Self::Compare { op, ty } => format!("{}_{}_{}", op.symbol(), ty.symbol(), ty.symbol()),
            Self::Negate { ty } => format!("neg_{}", ty.symbol()),
            Self::Widen { from, to } => format!("widen_{}_{}", from.symbol(), to.symbol()),
            Self::ToString { from } => format!("to_string_{}", from.symbol()),
            Self::Concat => "concat_string_string".to_owned(),
            Self::Println { ty } => format!("println_{}", ty.symbol()),
            Self::UserFn { name } => name.clone(),
            Self::Mpi(op) => op.symbol().to_owned(),
            Self::ArrayNew { .. } => ARRAY_ALLOC.to_owned(),
            Self::ArrayLength => ARRAY_LENGTH.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedComponent {
    pub name: String,
    pub exports: Vec<String>,
    pub functions: Vec<TypedFn>,
    /// Set when any function resolved an MPI builtin. The driver reads it to
    /// decide whether the MPI shim goes into the link.
    pub uses_mpi: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedFn {
    pub name: String,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: TypedExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedParam {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypedExprKind {
    /// Already pinned to a concrete integer type by its context.
    IntConst(i128),
    FloatConst(f64),
    StrConst(String),
    BoolConst(bool),
    Var(String),
    /// Every operator and call becomes this. The target is concrete.
    Apply {
        target: Target,
        args: Vec<TypedExpr>,
    },
    If {
        cond: Box<TypedExpr>,
        then_branch: Box<TypedExpr>,
        else_branch: Option<Box<TypedExpr>>,
    },
    Block {
        items: Vec<TypedBlockItem>,
        tail: Option<Box<TypedExpr>>,
    },
    ArrayLit {
        elem: Elem,
        items: Vec<TypedExpr>,
    },
    Index {
        base: Box<TypedExpr>,
        index: Box<TypedExpr>,
        elem: Elem,
    },
    While {
        cond: Box<TypedExpr>,
        body: Box<TypedExpr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypedBlockItem {
    Binding {
        name: String,
        ty: Type,
        value: TypedExpr,
        /// Only a mutable binding gets an `alloca`; everything else stays an
        /// SSA value, which is what keeps M1's generated code unchanged.
        mutable: bool,
        span: Span,
    },
    Assign {
        target: AssignTarget,
        value: TypedExpr,
        span: Span,
    },
    Expr(TypedExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignTarget {
    Var {
        name: String,
        ty: Type,
    },
    Element {
        base: TypedExpr,
        index: TypedExpr,
        elem: Elem,
    },
}

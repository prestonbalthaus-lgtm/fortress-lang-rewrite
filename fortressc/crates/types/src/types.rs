//! The typed AST. Every operator and call in here names one concrete target,
//! so codegen never asks a type question.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex, PoisonError};

use fortress_ast::Span;

/// User declared type names, promoted to `'static` so that [`Type`] stays
/// `Copy` and comparable by value. The compiler handles one component per
/// process, so this table is that component's own vocabulary; interning is what
/// keeps it from growing by a copy per call in a test binary.
#[must_use]
pub fn intern(name: &str) -> &'static str {
    static TABLE: LazyLock<Mutex<HashSet<&'static str>>> =
        LazyLock::new(|| Mutex::new(HashSet::new()));
    let mut table = TABLE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = table.get(name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    table.insert(leaked);
    leaked
}

/// A tuple's element types, promoted to `'static` for the same reason names
/// are: a shared reference is `Copy` whatever it points at, which is the whole
/// trick that lets [`Type`] carry a composite and stay `Copy`.
///
/// SPIKE-COMPOSITE-TYPE measured the alternative -- making `Type` non-`Copy`
/// produced 162 located errors across the workspace. Interning produces four.
#[must_use]
pub fn intern_types(types: &[Type]) -> &'static [Type] {
    static TABLE: LazyLock<Mutex<HashSet<&'static [Type]>>> =
        LazyLock::new(|| Mutex::new(HashSet::new()));
    let mut table = TABLE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = table.get(types) {
        return found;
    }
    let leaked: &'static [Type] = Box::leak(types.to_vec().into_boxed_slice());
    table.insert(leaked);
    leaked
}

/// One [`Type`], promoted to `'static` so that `Thread[\T\]` can carry its
/// result type and [`Type`] can stay `Copy` -- the same trick `intern_types`
/// plays for a tuple, for the same reason.
#[must_use]
pub fn intern_type(ty: Type) -> &'static Type {
    static TABLE: LazyLock<Mutex<HashSet<&'static Type>>> =
        LazyLock::new(|| Mutex::new(HashSet::new()));
    let mut table = TABLE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = table.get(&ty) {
        return found;
    }
    let leaked: &'static Type = Box::leak(Box::new(ty));
    table.insert(leaked);
    leaked
}

/// What an array holds. A separate enum from [`Type`] so that [`Type`] stays
/// `Copy` without boxing, and so that "array of array" is unrepresentable
/// rather than merely rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
            // `Char` IS NOT AN ARRAY ELEMENT in this subset, and the refusal
            // is here rather than at the array constructor: nothing in the
            // corpus writes `Array[\Char\]`, and `String` indexing -- which
            // is where one would come from -- is out of the subset too.
            Type::Void
            | Type::Char
            | Type::Array(..)
            | Type::Object(_)
            | Type::Trait(_)
            | Type::Thread(_)
            | Type::Tuple(_) => None,
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
    pub fn symbol(self) -> &'static str {
        self.as_type().symbol()
    }
}

/// The type names this compiler knows WITHOUT a declaration, and the one list
/// that says so.
///
/// THERE WERE FOUR OF THESE AND THEY DISAGREED. `mono.rs` listed all eight,
/// `closure.rs` listed the first six, `Registry::resolve` special-cases
/// `Array` and reads the trait table for the two roots, and each was written
/// for its own pass. The disagreement was invisible until `Object` and `Any`
/// became real types: `x: Object` resolved, and `f: ZZ32 -> Object` did not.
///
/// `Array` is here because `Array[\T\]` is written like a declared generic
/// and resolved like a builtin. `Any` and `Object` are here because
/// `Checker::new` seeds them as root traits; they come out together on the day
/// import resolution can supply them from `LibraryBuiltin/AnyType.fss`.
/// The builtin type names that take STATIC ARGUMENTS, and the ONE list that
/// says so. `Array[\T\]` and `Thread[\T\]` are written like a declared
/// generic and resolved like a builtin, so two passes have to agree about them:
/// `mono`'s expansion, which must let one through instead of demanding a
/// template, and `Registry::resolve`, which must build the type.
///
/// IT IS ONE LIST BECAUSE THE LAST TIME THERE WERE FOUR THEY DISAGREED -- see
/// `BUILTIN_TYPE_NAMES` below. `mono` carried its own `["Array"]`, and adding
/// `Thread` to the registry alone made `Thread[\Any\]` resolve nowhere: it
/// died in expansion as `unknown type Thread`, before the registry ran.
pub const BUILTIN_TYPE_CONSTRUCTORS: [&str; 2] = ["Array", "Thread"];

pub const BUILTIN_TYPE_NAMES: [&str; 9] = [
    "ZZ32", "ZZ64", "RR64", "Boolean", "String", "Array", "Any", "Object", "Char",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    ZZ32,
    ZZ64,
    RR64,
    Boolean,
    String,
    /// One Unicode scalar. ORDERED but not NUMERIC: `Char.fss` compares with
    /// all six operators and the legacy records `run_out_equals=a`, so a
    /// character prints as itself. Arithmetic on one is refused by name --
    /// 1.0 spells the two conversions `char` and `codePoint`, and neither is
    /// in this subset.
    Char,
    Void,
    /// Homogeneous, and RANK IS PART OF THE TYPE as of the multi-dimensional
    /// milestone: `ZZ32[5]` is `Array(ZZ32, 1)` and `ZZ32[2,3]` is
    /// `Array(ZZ32, 2)`. Nesting still arrives with generics -- an element is
    /// an [`Elem`] and never another array.
    ///
    /// RANK IS IN THE TYPE AND THE EXTENT IS NOT, and the two decisions point
    /// opposite ways on purpose. `Type` is `Copy` and compared with `==` at
    /// `is_subtype`, at overload duplicate detection and across M3c's whole
    /// dispatch domain, so anything carried here re-decides what type equality
    /// MEANS. Rank survives that test and an extent does not:
    ///   * Rank is STATIC. `array(2,3)` and `array(m,n)` are both rank two
    ///     whatever their arguments are, so every construction site can supply
    ///     it. An extent cannot: `array(n)` takes a RUN-TIME count and has no
    ///     extent to give.
    ///   * Rank DISTINGUISHES. A rank-1 and a rank-2 array have different
    ///     subscript arities, different storage and different shims, so
    ///     calling them equal would be a wrong answer rather than a coarse
    ///     one. Two `ZZ32[5]` of different sizes ARE the same type, and 1.0
    ///     says so.
    ///
    /// The extent is still validated at a declaration and dropped there, and
    /// that NAMED DEVIATION with its two stated holes is unchanged.
    Array(Elem, u8),
    /// A concrete object type: a heap block whose first four bytes are its tag.
    Object(&'static str),
    /// A trait. No run-time representation of its own -- a trait typed value is
    /// a pointer to some concrete object, and its membership is a compile time
    /// fact about that object's tag.
    Trait(&'static str),
    /// `spawn e`'s handle, carrying the RESULT type of the body. A pointer at
    /// run time -- the `FortressThread` block `fortress_spawn` returns -- so it
    /// needs no more representation than an object does.
    ///
    /// THE RESULT TYPE IS IN THE TYPE because `val()` has to return something,
    /// and the corpus writes `Thread[\Any\]` while the body's real type is
    /// what `val()` must give back: `Spawn5.fss:22-23` declares
    /// `ft: Thread[\Any\]` and then asserts `ft.val()` equals `10`.
    Thread(&'static Type),
    /// Two or more element types, interned. SPIKE-COMPOSITE-TYPE's landing.
    ///
    /// UNCONSTRUCTABLE ON PURPOSE, AND THAT IS THE WHOLE POINT OF LANDING IT
    /// THIS WAY. Nothing builds one: `registry.rs`'s `resolve` still refuses
    /// `TypeRef::Tuple` by name, and that ONE site is the single gate. What
    /// this variant buys is that the compiler can now be ASKED where a tuple
    /// would go -- the four exhaustive matches below were found by adding it
    /// and harvesting E0004, which ripgrep cannot do -- and that the twenty
    /// non-exhaustive sites that would SWALLOW one have been read and answered.
    ///
    /// A tuple VALUE is a separate milestone and is not this: there is no
    /// boxing in this backend, so it needs a representation, and
    /// `overloading.tex:124-126` makes `f(x: (A,B))` and `f(a:A,b:B)` the same
    /// declaration, which means M3c's dispatch has to arity-flatten first.
    Tuple(&'static [Type]),
}

/// `Type` IS `Copy`, asserted by the compiler rather than claimed. A shared
/// reference is `Copy` whatever it points at; if a later composite is ever
/// added by value this line is what stops it silently.
const fn assert_copy<T: Copy>() {}
const _: () = assert_copy::<Type>();

impl Type {
    /// Whether a value of this type can be STORED -- in a field, a global or an
    /// `alloca`. `()` has no value at all and a tuple has no representation in
    /// this backend, and codegen's `basic_type` returns `None` for exactly
    /// these two. Kept here so a decision about storage can be made in the
    /// CHECKER, where it can still become a diagnostic.
    #[must_use]
    pub const fn has_storage(self) -> bool {
        !matches!(self, Self::Void | Self::Tuple(_))
    }

    /// NOT `const` any more, and that is the tuple variant's one real cost. A
    /// placeholder here -- one string for every tuple -- would give two
    /// different tuples the same SYMBOL below, which is a silent collision in
    /// the emitted object rather than a diagnostic. Interning the built name
    /// keeps the `'static` lifetime and keeps distinct tuples distinct.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ZZ32 => "ZZ32",
            Self::ZZ64 => "ZZ64",
            Self::RR64 => "RR64",
            Self::Boolean => "Boolean",
            Self::String => "String",
            Self::Char => "Char",
            Self::Void => "()",
            // RANK 1 KEEPS ITS EXACT SPELLING, which is not cosmetic: this name
            // reaches diagnostics and `symbol` below reaches the emitted object,
            // and every module that compiled before this milestone has to lower
            // byte for byte unchanged. 1.0's own names for the higher ranks are
            // `Array2` and `Array3`, `arrays.tex`.
            Self::Array(elem, 1) => match elem {
                Elem::ZZ32 => "Array[\\ZZ32\\]",
                Elem::ZZ64 => "Array[\\ZZ64\\]",
                Elem::RR64 => "Array[\\RR64\\]",
                Elem::Boolean => "Array[\\Boolean\\]",
                Elem::String => "Array[\\String\\]",
            },
            Self::Array(elem, rank) => {
                intern(&format!("Array{rank}[\\{}\\]", elem.as_type().name()))
            }
            Self::Object(name) | Self::Trait(name) => name,
            Self::Thread(result) => intern(&format!("Thread[\\{}\\]", result.name())),
            Self::Tuple(elems) => {
                let inner: Vec<&str> = elems.iter().map(|t| t.name()).collect();
                intern(&format!("({})", inner.join(", ")))
            }
        }
    }

    /// Lowercase form used to build target symbols like `add_zz64_zz64`.
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::ZZ32 => "zz32",
            Self::ZZ64 => "zz64",
            Self::RR64 => "rr64",
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Char => "char",
            Self::Void => "void",
            Self::Array(elem, 1) => match elem {
                Elem::ZZ32 => "array_zz32",
                Elem::ZZ64 => "array_zz64",
                Elem::RR64 => "array_rr64",
                Elem::Boolean => "array_boolean",
                Elem::String => "array_string",
            },
            // RANK IS IN THE MANGLE. Without it `ZZ32[5]` and `ZZ32[2,3]` build
            // the same symbol as static arguments to one generic, which is a
            // silent collision in the emitted object rather than a diagnostic --
            // the same argument the tuple variant's `name` makes.
            Self::Array(elem, rank) => intern(&format!("array{rank}_{}", elem.symbol())),
            Self::Object(name) | Self::Trait(name) => name,
            Self::Thread(result) => intern(&format!("thread_{}", result.symbol())),
            Self::Tuple(elems) => {
                let inner: Vec<&str> = elems.iter().map(|t| t.symbol()).collect();
                intern(&format!("tuple_{}", inner.join("_")))
            }
        }
    }

    /// How many subscripts one of these takes, and `None` for anything that is
    /// not an array. Asked at every site that used to write
    /// `matches!(ty, Type::Array(_))` and then assume one.
    #[must_use]
    pub const fn array_rank(self) -> Option<u8> {
        match self {
            Self::Array(_, rank) => Some(rank),
            _ => None,
        }
    }

    #[must_use]
    /// A THREAD HANDLE IS A REFERENCE. It is the pointer `fortress_spawn`
    /// returned, so everything that asks this question about storage and
    /// passing gets the same answer it gives an object.
    pub const fn is_reference(self) -> bool {
        matches!(self, Self::Object(_) | Self::Trait(_) | Self::Thread(_))
    }

    /// Whether the runtime has a per-scalar shim for this type: `println_x`,
    /// `print_x` and `to_string_x`.
    ///
    /// THIS USED TO BE `Elem::of(ty).is_some()` AND THE TWO QUESTIONS CAME
    /// APART WHEN `Char` ARRIVED. `Elem` is what an ARRAY can store, and a
    /// `Char` is deliberately not one of those -- nothing writes
    /// `Array[\Char\]` and `String` indexing is out of the subset. It is
    /// printable all the same, and `ProjectFortress/other_compiler_tests/
    /// Char.test` records `run_out_equals=a`, so the character prints as
    /// itself. Saying so here is a diagnostic; leaving it to codegen is
    /// `no runtime symbol to_string_Char` at exit 70.
    #[must_use]
    pub const fn has_scalar_shim(self) -> bool {
        matches!(
            self,
            Self::ZZ32 | Self::ZZ64 | Self::RR64 | Self::Boolean | Self::String | Self::Char
        )
    }

    /// Ordered without being numeric. `is_numeric` is what the arithmetic path
    /// asks and `Char` must stay out of it; `is_ordered` is what a comparison
    /// asks, and the two stopped being the same question when `Char` arrived.
    #[must_use]
    pub const fn is_ordered(self) -> bool {
        self.is_numeric() || matches!(self, Self::Char)
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
    /// Reachable only as a REDUCTION's fold operator: no source syntax spells
    /// `MAX=`, and codegen lowers these two to a compare and a select rather
    /// than to a call, so no `max_zz64_zz64` shim exists or is wanted.
    Max,
    Min,
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
            Self::Max => "max",
            Self::Min => "min",
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
/// The halt a failed `assert` calls. Named here so codegen and the checker
/// cannot disagree about it.
pub const ASSERT_FAILED: &str = "fortress_assert_failed";

/// The halt a `case` with no matching arm and no `else` reaches. 1.0 throws
/// MatchFailure there; this subset has no exceptions, and falling through
/// silently is the one option that is worse than either.
pub const CASE_FAILED: &str = "fortress_case_failed";

/// The parallel loop entry point and the environment allocator. The
/// environment is SCANNED: a capture may be a String or an Array, and the
/// collector has to see through the environment to it while a worker holds it.
pub const PARALLEL_FOR: &str = "fortress_parallel_for";
pub const SPAWN: &str = "fortress_spawn";
pub const THREAD_VAL: &str = "fortress_thread_val";
pub const THREAD_WAIT: &str = "fortress_thread_wait";
pub const THREAD_READY: &str = "fortress_thread_ready";
pub const THREAD_STOP: &str = "fortress_thread_stop";
pub const ENV_ALLOC: &str = "fortress_env_alloc";

/// `atomic`. One process-wide recursive mutex, and the pair also hands the
/// runtime's `fortress_in_parallel` flag over so that a loop reached from
/// inside an atomic region runs inline instead of deadlocking against it.
pub const ATOMIC_ENTER: &str = "fortress_atomic_enter";
pub const ATOMIC_LEAVE: &str = "fortress_atomic_leave";

/// The per-worker reduction accumulators. The runtime owns the row count
/// because a worker writes row `chunk`; codegen owns the stride and hands it
/// over, so the two sides cannot disagree about the padding.
pub const REDUCTION_ALLOC: &str = "fortress_reduction_alloc";
pub const REDUCTION_WORKERS: &str = "fortress_reduction_workers";

/// `throw e` with nothing to catch it, which in this subset is every throw.
/// The argument is the exception's STATIC type name.
pub const THROW: &str = "fortress_throw";
pub const ARRAY_ALLOC: &str = "fortress_array_alloc";
pub const ARRAY_LENGTH: &str = "fortress_array_length";
pub const ARRAY_SLOT: &str = "fortress_array_slot";
/// Rank two and above. A SEPARATE PAIR on purpose: rank one keeps the shims
/// above unchanged so that every module that compiled before the
/// multi-dimensional milestone lowers to the IR it lowered to then.
pub const ARRAY_ALLOC_N: &str = "fortress_array_alloc_n";
pub const ARRAY_SLOT_N: &str = "fortress_array_slot_n";

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
    /// `NOT`. One `xor` on an `i1`, and no branch: negation does not short
    /// circuit, so it needs none.
    Not,
    /// `^`, and the one operator whose operands may differ in type: 1.0
    /// declares it on every base-exponent pair, and
    /// `ProjectFortress/tests/expTest.fss` exercises all four of them. It is a
    /// C shim for every pair -- integers have no power instruction, and a loop
    /// emitted inline would be a second place the negative-exponent rule
    /// could be got wrong.
    Pow {
        base: Type,
        exponent: Type,
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
    /// `print`: `println` without the newline.
    Print {
        ty: Type,
    },
    /// The halt a failed `assert` reaches. It takes the message and does not
    /// return, so nothing downstream needs a value from it.
    AssertFailed,
    /// The halt a `case` falls out of the bottom into. No arguments: the
    /// diagnostic is the same wherever it is reached from, and a span would
    /// have to be lowered as a string constant per site.
    CaseFailed,
    /// A function declared in this component. `name` is already the mangled
    /// symbol: an overload set of one keeps its bare name, so nothing about
    /// pre-M3c generated code changes.
    UserFn {
        name: String,
    },
    /// A compiler generated decision tree over the concrete type tags of the
    /// arguments. Reached only when the table for this call site does not
    /// collapse to a single winner.
    Dispatch {
        symbol: String,
    },
    /// `O(...)`: the generated constructor for an object.
    ObjectNew {
        symbol: String,
    },
    Mpi(MpiOp),
    /// `array(n)`, and `array(m, n)` above rank one. The element type is not
    /// in the symbol: the runtime is told the element width and whether it
    /// holds pointers, and that is all it needs to know. The RANK is, because
    /// it picks which of the two allocators is called.
    ArrayNew {
        elem: Elem,
        rank: u8,
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
            Self::Not => "not_boolean".to_owned(),
            Self::Pow { base, exponent } => {
                format!("pow_{}_{}", base.symbol(), exponent.symbol())
            }
            Self::Widen { from, to } => format!("widen_{}_{}", from.symbol(), to.symbol()),
            Self::ToString { from } => format!("to_string_{}", from.symbol()),
            Self::Concat => "concat_string_string".to_owned(),
            Self::Println { ty } => format!("println_{}", ty.symbol()),
            Self::Print { ty } => format!("print_{}", ty.symbol()),
            Self::AssertFailed => ASSERT_FAILED.to_owned(),
            Self::CaseFailed => CASE_FAILED.to_owned(),
            Self::UserFn { name } => name.clone(),
            Self::Dispatch { symbol } | Self::ObjectNew { symbol } => symbol.clone(),
            Self::Mpi(op) => op.symbol().to_owned(),
            Self::ArrayNew { rank: 1, .. } => ARRAY_ALLOC.to_owned(),
            Self::ArrayNew { .. } => ARRAY_ALLOC_N.to_owned(),
            Self::ArrayLength => ARRAY_LENGTH.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedComponent {
    pub name: String,
    pub exports: Vec<String>,
    /// Declaration order, which is also singleton construction order.
    pub objects: Vec<TypedObject>,
    pub functions: Vec<TypedFn>,
    /// One per distinct (overload set, static argument tuple) that needed a
    /// run-time decision. Sorted by symbol, so the module is deterministic.
    pub dispatches: Vec<DispatchFn>,
    /// Set when any function resolved an MPI builtin. The driver reads it to
    /// decide whether the MPI shim goes into the link.
    pub uses_mpi: bool,
    /// The source was an `api`. It was CHECKED -- headers resolved, bounds
    /// discharged -- and there is nothing to emit, because an api declares
    /// signatures and signatures have no code. The driver stops before codegen
    /// on this flag rather than on the file extension.
    pub is_api: bool,
    /// Top-level values, ALREADY IN INITIALIZATION ORDER. Codegen emits one
    /// global each and runs the initializers in this order inside `main`,
    /// after `fortress_runtime_init` and before `run` -- exactly where
    /// singletons already go.
    pub values: Vec<TypedValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedFn {
    pub name: String,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: TypedExpr,
    pub span: Span,
}

/// The tag of the first object declared. Zero is never a valid tag, so a block
/// that was never given one cannot be mistaken for an instance of anything.
pub const FIRST_TAG: u32 = 1;

/// The allocator entry point for objects. It takes the tag as well as the size
/// so that writing the tag lives in exactly one place, the same way the bounds
/// check lives in exactly one place.
pub const OBJECT_ALLOC: &str = "fortress_object_alloc";

/// Called from a switch arm that no concrete tag can reach. Statically dead;
/// it exists so "unreachable" means a clean halt with a diagnostic.
pub const DISPATCH_FAILED: &str = "fortress_dispatch_failed";

#[derive(Debug, Clone, PartialEq)]
pub struct TypedObject {
    pub name: &'static str,
    pub tag: u32,
    /// The generated constructor, `Name$new`.
    pub symbol: String,
    /// Fields in layout order. The first `param_count` of them are the
    /// constructor's value parameters; the rest are computed by `initializers`,
    /// in the same order.
    pub fields: Vec<TypedField>,
    pub param_count: usize,
    pub initializers: Vec<TypedExpr>,
    /// One instance, built between `fortress_runtime_init` and `run`.
    pub singleton: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedField {
    pub name: String,
    pub ty: Type,
    /// Declared `var`. Only a mutable field is an assignment target, and it is
    /// the only storage in the program the collector can see a write to after
    /// construction -- Boehm needs no write barrier, so the store is a store.
    pub mutable: bool,
}

/// A generated dispatch function. Its signature is the call site's static
/// argument types, so every call that shares those types shares the function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchFn {
    pub symbol: String,
    /// The overload set's Fortress name, passed to the failure shim.
    pub set_name: String,
    pub params: Vec<Type>,
    pub returns: Type,
    pub tree: DispatchNode,
}

/// The table, already flattened. A row whose cells all name the same winner is
/// a [`DispatchNode::Call`], so the tree is usually shallower than the arity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchNode {
    /// Forward every parameter to this symbol and return the result.
    Call { symbol: String },
    Switch {
        /// Which parameter's tag to read.
        position: usize,
        arms: Vec<(u32, DispatchNode)>,
    },
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
    /// `()`. Typed as `Void`, and lowers to no value at all.
    Unit,
    /// Already pinned to a concrete integer type by its context.
    IntConst(i128),
    FloatConst(f64),
    StrConst(String),
    /// A Unicode scalar, and it lowers to an `i32` -- the same width `Boolean`
    /// crosses the C boundary as, and wide enough for every code point.
    CharConst(char),
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
    /// `throw e`. The value is evaluated for its effects and then the
    /// program halts naming `exception`, the value's STATIC type.
    ///
    /// IT HAS THE TYPE ITS CONTEXT WANTS, which is this compiler's stand-in
    /// for a bottom type: a throw never returns, so it may stand where any
    /// type is required and codegen emits a poison value of that type after
    /// the call. Nothing reads it -- the call does not return.
    Throw {
        exception: String,
        value: Box<TypedExpr>,
    },
    /// `(e1, e2)` where a TUPLE is what the context wants -- which is exactly
    /// one place: the result of a function whose declared result is a tuple.
    ///
    /// IT LOWERS TO AN LLVM AGGREGATE and it is STILL NON-MATERIALISING: the
    /// struct lives in SSA registers, built with `insertvalue`. No
    /// `fortress_alloc`, no GC block, no type tag, no `alloca`. LLVM's own ABI
    /// lowering decides whether the pair travels in registers or through a
    /// hidden pointer, and that decision belongs to the target.
    TupleValue {
        parts: Vec<TypedExpr>,
    },
    ArrayLit {
        elem: Elem,
        /// ROW MAJOR, whatever order the source wrote them in.
        items: Vec<TypedExpr>,
        /// One per dimension, outermost first; `[items.len()]` at rank one.
        extents: Vec<usize>,
    },
    Index {
        base: Box<TypedExpr>,
        /// As many as the array's rank, checked before this was built.
        indices: Vec<TypedExpr>,
        elem: Elem,
    },
    While {
        cond: Box<TypedExpr>,
        body: Box<TypedExpr>,
    },
    /// A field read. The index is into [`TypedObject::fields`]; the offset it
    /// becomes is codegen's business.
    Field {
        base: Box<TypedExpr>,
        index: u32,
    },
    /// A `for` loop over a half-open integer range. The body is OUTLINED into
    /// a real function -- `symbol` -- so a worker thread can call it; the
    /// values it reads from the enclosing scope are copied into an environment
    /// struct allocated ONCE, before the loop, never per iteration.
    ///
    /// `sequential` loops keep the same shape and run inline, so there is one
    /// lowering rather than two.
    ParallelFor {
        binder: String,
        lo: Box<TypedExpr>,
        hi: Box<TypedExpr>,
        body: Box<TypedExpr>,
        captures: Vec<TypedCapture>,
        /// The names the body reduces into. NOT captures: each worker adds
        /// into a private accumulator and the caller folds them afterwards, so
        /// there is nothing shared for the iterations to race on.
        reductions: Vec<TypedReduction>,
        symbol: String,
        sequential: bool,
    },
    /// `atomic e`. Serialised against every other atomic region in the
    /// process; the mechanism is the runtime's choice -- atomic.tex:89-90.
    Atomic {
        body: Box<TypedExpr>,
    },
    /// `spawn e`. The body is OUTLINED, exactly as a parallel loop body is, and
    /// `captures` is the environment it travels with. Evaluates to the handle
    /// `fortress_spawn` returns.
    Spawn {
        body: Box<TypedExpr>,
        captures: Vec<TypedCapture>,
        symbol: String,
    },
    /// One of the four methods a `Thread[\T\]` handle answers.
    ThreadOp {
        op: ThreadOp,
        handle: Box<TypedExpr>,
    },
    /// The one instance of a singleton object.
    Singleton {
        name: &'static str,
    },
    /// `typecase`. The subject is evaluated once and switched on its TAG, which
    /// is the same 32-bit load at offset 0 that M3c's dispatch tree does -- a
    /// trait has no run-time representation, so an arm naming one is the set of
    /// concrete tags below it, computed here and not at run time.
    ///
    /// Arms are matched FIRST ONE WINS: a tag claimed by an earlier arm is not
    /// offered to a later one, so the switch has one entry per tag and needs no
    /// ordering at run time.
    TypeCase {
        subject: Box<TypedExpr>,
        arms: Vec<TypedTypeCaseArm>,
        /// Always present. `comprises` is not enforced anywhere in this
        /// compiler, so exhaustiveness cannot be proved from it.
        else_branch: Box<TypedExpr>,
    },
    /// `label L ... end L`. One block, one phi over the exits and the
    /// fallthrough. No unwinding: every edge is a forward jump inside one
    /// function, which is why `exit` out of an outlined loop body is refused by
    /// the checker instead.
    Label {
        name: String,
        body: Box<TypedExpr>,
    },
    /// `exit L with e`. A branch to the label's merge block. It produces no
    /// value of its own -- control does not come back -- so codegen hands the
    /// enclosing expression a zero of the right type from a block nothing can
    /// reach.
    Exit {
        name: String,
        value: Option<Box<TypedExpr>>,
    },
}

/// One arm of a [`TypedExprKind::TypeCase`].
#[derive(Debug, Clone, PartialEq)]
pub struct TypedTypeCaseArm {
    /// Every concrete tag this arm claims, in declaration order.
    pub tags: Vec<u32>,
    /// `x` of `x: T => e`, bound to the subject at the narrowed type.
    pub binder: Option<String>,
    pub ty: Type,
    pub body: TypedExpr,
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
        /// `Some` for `x op= e`. Kept folded this far on purpose: splitting it
        /// into `x := x op e` makes the target a READ, and reduction.tex:35's
        /// third condition is that a reduction variable is not otherwise read.
        /// Codegen is what splits it.
        op: Option<ArithOp>,
        value: TypedExpr,
        span: Span,
    },
    Expr(TypedExpr),
    /// `(a, b) = e`, ALREADY SPLIT. The checker emits one entry per name with
    /// the element that fills it, so codegen sees ordinary bindings and NOTHING
    /// MATERIALISES -- no tuple value is ever built, stored or passed.
    ///
    /// That is the non-materialising convention the round-2 deferred doc calls
    /// option (2), and it is what keeps this milestone free of boxing: a tuple
    /// has no run-time representation here because no run-time value of one is
    /// ever created.
    TupleBinding {
        parts: Vec<TypedBinding>,
        span: Span,
    },
    /// `(a, b) = f(...)` where `f`'s declared result is a tuple. The value is
    /// ONE expression of tuple type and each name takes one FIELD of it.
    ///
    /// A SEPARATE VARIANT FROM `TupleBinding` BECAUSE THE CALL HAPPENS ONCE.
    /// `TupleBinding` above carries one expression per name, which is right
    /// when the source WROTE a tuple -- there is nothing whole to evaluate.
    /// Here there is: splitting it into one call per name would call `f` twice.
    TupleDestructure {
        /// Type is `Type::Tuple`, with one element per part.
        value: TypedExpr,
        /// Name and element type, in field order. The `value` field of each is
        /// unused -- the field is extracted from `value` above.
        parts: Vec<(String, Type)>,
        span: Span,
    },
}

/// A top-level value, and the initializer that fills it.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedValue {
    pub name: String,
    pub ty: Type,
    /// `None` inside an `api`, where a value declaration is a signature.
    pub init: Option<TypedExpr>,
    pub mutable: bool,
    pub span: Span,
}

/// One name of a destructured tuple, and the element that fills it.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedBinding {
    pub name: String,
    pub ty: Type,
    pub value: TypedExpr,
}

/// What a `Thread[\T\]` handle can be asked. Four, and no more: 1.0's
/// `Thread` trait is larger, and every other member is refused by name rather
/// than approximated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadOp {
    /// Join, discarding the value.
    Wait,
    /// Join and return the body's value.
    Val,
    /// Has it finished? Never blocks.
    Ready,
    /// ABANDON. A named deviation from 1.0, which is closer to cancellation --
    /// real cancellation needs a safe point in generated code, and a
    /// `pthread_cancel` at an arbitrary instruction can end a thread holding
    /// the process-wide atomic mutex.
    Stop,
}

/// A value an outlined loop or spawned body reads or writes from the
/// enclosing scope.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedCapture {
    pub name: String,
    pub ty: Type,
    /// The environment carries the ADDRESS of the caller's storage rather than
    /// a copy of its value. Anything the body assigns to needs it: a copy has
    /// no store target at all, which is the exit-70 internal error, and under
    /// the atomic lock it would be a lock around a private copy -- every
    /// worker incrementing its own loop-entry value, update lost with the lock
    /// held.
    ///
    /// The lifetime is safe by construction: `fortress_parallel_for` blocks on
    /// its done-wait before it returns, so the caller's stack slot outlives
    /// every worker's use of it. No heap box and no escape analysis.
    pub by_ref: bool,
}

/// A recognised reduction variable -- reduction.tex:28-39. It needs no syntax:
/// the shape is what is recognised, and `atomic` is neither required nor an
/// obstacle.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedReduction {
    pub name: String,
    pub ty: Type,
    /// What the partials are folded with, and what each slot is initialised to.
    /// `+` and `-` share `Add` -- `-=` accumulates `Identity - e`, so the group
    /// inverse is already inside the partial -- and `*` is its own, because 0
    /// is not the identity for it and adding the partials of a product is not
    /// a product.
    pub op: ArithOp,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignTarget {
    Var {
        name: String,
        ty: Type,
    },
    /// Boxed: an assignment target holding two whole expressions inline made
    /// `TypedBlockItem` several hundred bytes wide once `Type` grew a name.
    Element {
        base: Box<TypedExpr>,
        /// As many as the array's rank, checked before this was built.
        indices: Vec<TypedExpr>,
        elem: Elem,
    },
    /// `o.f := e`, and a bare `f := e` inside a method, which resolves to the
    /// same thing with `self` as the base.
    ///
    /// A DIRECT STORE, and that is a named deviation:
    /// `Specification/basic/expressions/bindings.tex:60-61` says assigning a
    /// field calls the corresponding setter. There is no setter to call --
    /// accessors are excluded from every member walk in this compiler -- so
    /// calling one would mean inventing it. The store is what the layout
    /// already supports; when setters land, this becomes the path a field with
    /// no declared setter takes.
    Field {
        base: Box<TypedExpr>,
        /// Into [`TypedObject::fields`], as [`TypedExprKind::Field`] is.
        index: u32,
        ty: Type,
    },
}

//! Typed AST to LLVM IR.
//!
//! Every operator and call arrives already resolved to one concrete
//! [`Target`], so nothing here dispatches: lowering is a translation, not a
//! decision. Failures are compiler bugs, not user errors.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use fortress_types::{
    ArithOp, AssignTarget, CompareOp, DispatchFn, DispatchNode, Elem, Target, Type, TypedBlockItem,
    TypedCapture, TypedComponent, TypedExpr, TypedExprKind, TypedFn, TypedObject, TypedReduction,
    TypedTypeCaseArm, ARRAY_ALLOC, ARRAY_ALLOC_N, ARRAY_LENGTH, ARRAY_SLOT, ARRAY_SLOT_N,
    ASSERT_FAILED, ATOMIC_ENTER, ATOMIC_LEAVE, CASE_FAILED, DISPATCH_FAILED, ENV_ALLOC,
    OBJECT_ALLOC, PARALLEL_FOR, REDUCTION_ALLOC, REDUCTION_WORKERS,
};
use inkwell::attributes::AttributeLoc;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target as LlvmTarget, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, PointerType, StructType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, GlobalValue, IntValue, PointerValue,
};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};

mod error;
pub use error::CodegenError;

/// The Fortress entry point. `main` calls it and returns 0.
const ENTRY: &str = "run";

/// Starts the collector. Emitted as the first instruction of `main`.
const RUNTIME_INIT: &str = "fortress_runtime_init";

/// Bytes between one worker's reduction slot and the next. A cacheline, and it
/// is measured rather than hygiene: 20M updates, padded against a plain
/// `int64_t[16]`, best of 3 -- 0.0055 against 0.0078 at 8 workers and 0.0036
/// against 0.0093 at 14, and the unpadded version gets WORSE from 8 to 14.
///
/// Handed to the allocator rather than duplicated in C, so the two sides
/// cannot disagree about how big the block is.
const REDUCTION_STRIDE: u64 = 64;

/// The processor an object is built for. This is chosen, not detected: the
/// machine that runs the compiler is a login node or a laptop, and the machine
/// that runs the binary is a compute node. `x86-64-v3` is AVX2 era and every
/// x86 part worth scheduling on has it; `skylake-avx512` is the Platinum 8160.
pub const DEFAULT_CPU: &str = "x86-64-v3";

/// LLVM answers an unrecognised processor with a warning and then builds for
/// the baseline anyway, which is a wrong binary rather than a failed build. The
/// name is checked against this list before it reaches LLVM.
pub const SUPPORTED_CPUS: [&str; 6] = [
    "x86-64",
    "x86-64-v2",
    "x86-64-v3",
    "x86-64-v4",
    "skylake-avx512",
    "native",
];

pub fn emit_object(
    component: &TypedComponent,
    object_path: &Path,
    cpu: &str,
) -> Result<(), CodegenError> {
    let machine = target_machine(cpu)?;
    let context = Context::create();
    let module = build_module(&context, component, &machine)?;
    machine
        .write_to_file(&module, FileType::Object, object_path)
        .map_err(|e| CodegenError::ObjectWriteFailed {
            detail: e.to_string(),
        })
}

pub fn emit_ir(component: &TypedComponent, cpu: &str) -> Result<String, CodegenError> {
    let machine = target_machine(cpu)?;
    let context = Context::create();
    let module = build_module(&context, component, &machine)?;
    Ok(module.print_to_string().to_string())
}

fn build_module<'ctx>(
    context: &'ctx Context,
    component: &TypedComponent,
    machine: &TargetMachine,
) -> Result<Module<'ctx>, CodegenError> {
    let module = context.create_module(&component.name);
    module.set_triple(&machine.get_triple());
    module.set_data_layout(&machine.get_target_data().get_data_layout());
    let builder = context.create_builder();
    let mut lowering = Lowering {
        context,
        module,
        builder,
        cpu: machine.get_cpu().to_string_lossy().into_owned(),
        functions: HashMap::new(),
        objects: HashMap::new(),
        singletons: HashMap::new(),
        scopes: Vec::new(),
        reductions: HashSet::new(),
        labels: Vec::new(),
    };
    lowering.declare_runtime();
    if component.uses_mpi {
        lowering.declare_mpi();
    }
    lowering.declare_objects(component)?;
    lowering.declare_functions(component)?;
    lowering.declare_dispatches(component)?;
    for o in &component.objects {
        lowering.define_constructor(o)?;
    }
    for f in &component.functions {
        lowering.define_function(f)?;
    }
    for d in &component.dispatches {
        lowering.define_dispatch(d)?;
    }
    lowering.emit_main(component)?;

    let module = lowering.module;
    module
        .verify()
        .map_err(|e| CodegenError::ModuleVerificationFailed {
            detail: e.to_string(),
        })?;
    Ok(module)
}

struct Lowering<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    /// Stamped onto every function this module defines, so the object records
    /// what it was built for instead of leaving it to whoever reads it.
    cpu: String,
    functions: HashMap<String, FunctionValue<'ctx>>,
    /// `{ i32 tag, i32 pad, fields... }`. The pad is explicit so that fields
    /// start at +8 whatever they are, which is what the layout says and what
    /// keeps the tag load at offset 0 the only thing dispatch has to know.
    objects: HashMap<&'static str, StructType<'ctx>>,
    singletons: HashMap<&'static str, GlobalValue<'ctx>>,
    scopes: Vec<HashMap<String, Slot<'ctx>>>,
    /// The reduction variables of the loop body being emitted. Read only by
    /// the `atomic` lowering, which drops the lock around a block that does
    /// nothing but write them.
    reductions: HashSet<String>,
    /// The labels open around the lowering, innermost last. A loop body is
    /// outlined into its own function and the checker refuses an `exit` that
    /// would cross into one, so a frame here always belongs to the function
    /// being emitted.
    labels: Vec<LabelFrame<'ctx>>,
}

/// Everything the outliner needs about one `for`. A struct rather than nine
/// positional arguments, because `captures` and `reductions` are both slices
/// of near-identical shape and swapping them would compile.
struct ParallelLoop<'a> {
    binder: &'a str,
    lo: &'a TypedExpr,
    hi: &'a TypedExpr,
    body: &'a TypedExpr,
    captures: &'a [TypedCapture],
    reductions: &'a [TypedReduction],
    symbol: &'a str,
    sequential: bool,
}

/// An immutable binding is an SSA value and costs nothing. Only a binding that
/// can be assigned to gets storage, which keeps generated code for the whole
/// pre-M3b language byte for byte what it was.
#[derive(Debug, Clone, Copy)]
enum Slot<'ctx> {
    Value(BasicValueEnum<'ctx>),
    Cell {
        pointer: PointerValue<'ctx>,
        ty: BasicTypeEnum<'ctx>,
    },
}

/// One `label` open around the lowering. Its merge block is the target of
/// every `exit`, and `incoming` is the phi's edge list.
struct LabelFrame<'ctx> {
    name: String,
    end: BasicBlock<'ctx>,
    ty: Type,
    incoming: Vec<(BasicValueEnum<'ctx>, BasicBlock<'ctx>)>,
}

impl<'ctx> Lowering<'ctx> {
    fn ptr_type(&self) -> PointerType<'ctx> {
        self.context.ptr_type(AddressSpace::default())
    }

    fn ptr(&self) -> BasicTypeEnum<'ctx> {
        self.ptr_type().into()
    }

    fn basic_type(&self, ty: Type) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            Type::ZZ32 => Some(self.context.i32_type().into()),
            Type::ZZ64 => Some(self.context.i64_type().into()),
            Type::RR64 => Some(self.context.f64_type().into()),
            Type::Boolean => Some(self.context.bool_type().into()),
            Type::String | Type::Array(..) | Type::Object(_) | Type::Trait(_) => Some(self.ptr()),
            Type::Void => None,
            // A TUPLE HAS NO REPRESENTATION IN THIS BACKEND, and `None` here
            // means "no storage" -- which is what `Void` means and is NOT what
            // a tuple means. Nothing can reach this arm: `registry.rs`'s
            // `resolve` refuses `TypeRef::Tuple` by name and is the single
            // construction gate, so a `Type::Tuple` in the typed AST is a
            // compiler defect and not a program. Returning a pointer would be
            // the silent answer -- every tuple would lower to one word and the
            // first two-element tuple would corrupt a frame.
            Type::Tuple(_) => unreachable!(
                "a tuple type reached codegen; `Registry::resolve` is the only                  gate that can build one and it refuses"
            ),
        }
    }

    /// How an element sits in an array's data block. `Boolean` is a byte there
    /// and an `i1` in a register, so the two cross through a zext and a trunc.
    fn element_type(&self, elem: Elem) -> BasicTypeEnum<'ctx> {
        match elem {
            Elem::ZZ32 => self.context.i32_type().into(),
            Elem::ZZ64 => self.context.i64_type().into(),
            Elem::RR64 => self.context.f64_type().into(),
            Elem::Boolean => self.context.i8_type().into(),
            Elem::String => self.ptr(),
        }
    }

    /// An `alloca` belongs in the entry block, not where the declaration
    /// happens to appear. A mutable declared inside a loop body would otherwise
    /// allocate one stack slot per iteration and overflow the stack.
    fn entry_alloca(
        &self,
        name: &str,
        ty: BasicTypeEnum<'ctx>,
    ) -> Result<PointerValue<'ctx>, CodegenError> {
        let entry = self
            .builder
            .get_insert_block()
            .and_then(inkwell::basic_block::BasicBlock::get_parent)
            .and_then(FunctionValue::get_first_basic_block)
            .ok_or_else(|| CodegenError::internal("no entry block for an alloca".to_owned()))?;

        let builder = self.context.create_builder();
        match entry.get_first_instruction() {
            Some(first) => builder.position_before(&first),
            None => builder.position_at_end(entry),
        }
        builder
            .build_alloca(ty, name)
            .map_err(CodegenError::from_builder)
    }

    /// The C shims. `Boolean` crosses as `int`, so those take `i32`.
    fn declare_runtime(&mut self) {
        let i32t = self.context.i32_type();
        let i64t = self.context.i64_type();
        let f64t = self.context.f64_type();
        let ptr = self.ptr();
        let void = self.context.void_type();

        let printlns: [(&str, Option<BasicMetadataTypeEnum<'ctx>>); 12] = [
            ("println_string", Some(ptr.into())),
            ("println_zz32", Some(i32t.into())),
            ("println_zz64", Some(i64t.into())),
            ("println_rr64", Some(f64t.into())),
            ("println_boolean", Some(i32t.into())),
            ("println_void", None),
            ("print_string", Some(ptr.into())),
            ("print_zz32", Some(i32t.into())),
            ("print_zz64", Some(i64t.into())),
            ("print_rr64", Some(f64t.into())),
            ("print_boolean", Some(i32t.into())),
            ("print_void", None),
        ];
        for (name, arg) in printlns {
            let ty = match arg {
                Some(a) => void.fn_type(&[a], false),
                None => void.fn_type(&[], false),
            };
            self.module.add_function(name, ty, Some(Linkage::External));
        }

        let to_strings: [(&str, BasicMetadataTypeEnum<'ctx>); 4] = [
            ("to_string_zz32", i32t.into()),
            ("to_string_zz64", i64t.into()),
            ("to_string_rr64", f64t.into()),
            ("to_string_boolean", i32t.into()),
        ];
        for (name, arg) in to_strings {
            let ty = ptr.fn_type(&[arg], false);
            self.module.add_function(name, ty, Some(Linkage::External));
        }

        // `^`, one shim per base-exponent pair. All nine exist because 1.0
        // declares the operator on all of them, not because a rule was chosen
        // about which combinations are allowed.
        let powers: [(
            &str,
            BasicTypeEnum<'ctx>,
            BasicMetadataTypeEnum<'ctx>,
            BasicMetadataTypeEnum<'ctx>,
        ); 9] = [
            ("pow_zz32_zz32", i32t.into(), i32t.into(), i32t.into()),
            ("pow_zz32_zz64", i32t.into(), i32t.into(), i64t.into()),
            ("pow_zz32_rr64", f64t.into(), i32t.into(), f64t.into()),
            ("pow_zz64_zz32", i64t.into(), i64t.into(), i32t.into()),
            ("pow_zz64_zz64", i64t.into(), i64t.into(), i64t.into()),
            ("pow_zz64_rr64", f64t.into(), i64t.into(), f64t.into()),
            ("pow_rr64_zz32", f64t.into(), f64t.into(), i32t.into()),
            ("pow_rr64_zz64", f64t.into(), f64t.into(), i64t.into()),
            ("pow_rr64_rr64", f64t.into(), f64t.into(), f64t.into()),
        ];
        for (name, ret, base, exponent) in powers {
            self.module.add_function(
                name,
                ret.fn_type(&[base, exponent], false),
                Some(Linkage::External),
            );
        }

        // Integer division, which is a shim and not an `sdiv` because two of
        // its operand pairs fault rather than producing a value. RR64 division
        // stays an `fdiv`: dividing by zero there is `inf`, not a failure.
        let divisions: [(&str, BasicTypeEnum<'ctx>); 2] = [
            ("fortress_div_zz32", i32t.into()),
            ("fortress_div_zz64", i64t.into()),
        ];
        for (name, width) in divisions {
            self.module.add_function(
                name,
                width.fn_type(&[width.into(), width.into()], false),
                Some(Linkage::External),
            );
        }

        let concat = ptr.fn_type(&[ptr.into(), ptr.into()], false);
        self.module
            .add_function("concat_string_string", concat, Some(Linkage::External));

        // The array runtime. Bounds checking and the pointer arithmetic live in
        // C; what comes back is the address of a slot, and the load or store
        // through it stays typed in IR.
        self.module.add_function(
            ARRAY_ALLOC,
            ptr.fn_type(&[i64t.into(), i64t.into(), i32t.into()], false),
            Some(Linkage::External),
        );
        self.module.add_function(
            ARRAY_LENGTH,
            i64t.fn_type(&[ptr.into()], false),
            Some(Linkage::External),
        );
        self.module.add_function(
            ARRAY_SLOT,
            ptr.fn_type(&[ptr.into(), i64t.into()], false),
            Some(Linkage::External),
        );
        // Rank two and above. The extents and the indices cross as a POINTER to
        // a buffer the caller owns rather than as a variadic call: a shim whose
        // arity depends on a rank would need one declaration per rank, and the
        // rank is already an argument.
        self.module.add_function(
            ARRAY_ALLOC_N,
            ptr.fn_type(&[i64t.into(), ptr.into(), i64t.into(), i32t.into()], false),
            Some(Linkage::External),
        );
        self.module.add_function(
            ARRAY_SLOT_N,
            ptr.fn_type(&[ptr.into(), i64t.into(), ptr.into()], false),
            Some(Linkage::External),
        );

        // Objects. The tag goes in with the allocation so that writing it
        // lives in one place, the way the bounds check does.
        self.module.add_function(
            OBJECT_ALLOC,
            ptr.fn_type(&[i64t.into(), i32t.into()], false),
            Some(Linkage::External),
        );
        // Declared unconditionally: a switch arm no tag can reach still has to
        // halt cleanly rather than fall into undefined behaviour.
        self.module.add_function(
            DISPATCH_FAILED,
            void.fn_type(&[ptr.into(), i32t.into(), i32t.into()], false),
            Some(Linkage::External),
        );

        // Declared unconditionally, like the dispatch halt: a failed assert
        // is a clean exit with a diagnostic, never a silent continue.
        self.module.add_function(
            ASSERT_FAILED,
            void.fn_type(&[ptr.into()], false),
            Some(Linkage::External),
        );

        // The `case` fallthrough halt, declared on the same terms as the two
        // above: a program with no `case` never calls it.
        self.module.add_function(
            CASE_FAILED,
            void.fn_type(&[], false),
            Some(Linkage::External),
        );

        // The parallel loop entry point and its environment allocator. Declared
        // unconditionally, like the dispatch halt: a program with no parallel
        // loop simply never calls them.
        self.module.add_function(
            PARALLEL_FOR,
            void.fn_type(
                &[
                    i64t.into(),
                    i64t.into(),
                    ptr.into(),
                    ptr.into(),
                    i64t.into(),
                ],
                false,
            ),
            Some(Linkage::External),
        );
        self.module.add_function(
            ENV_ALLOC,
            ptr.fn_type(&[i64t.into()], false),
            Some(Linkage::External),
        );

        // `atomic`, and the reduction accumulators. Declared unconditionally
        // like the rest: a program with neither never calls them, and a
        // program with no `atomic` in it emits byte-identical IR to M4's.
        for name in [ATOMIC_ENTER, ATOMIC_LEAVE] {
            self.module
                .add_function(name, void.fn_type(&[], false), Some(Linkage::External));
        }
        self.module.add_function(
            REDUCTION_ALLOC,
            ptr.fn_type(&[i64t.into(), i64t.into()], false),
            Some(Linkage::External),
        );
        self.module.add_function(
            REDUCTION_WORKERS,
            i64t.fn_type(&[], false),
            Some(Linkage::External),
        );

        self.module.add_function(
            RUNTIME_INIT,
            void.fn_type(&[], false),
            Some(Linkage::External),
        );
    }

    /// Layouts, constructors and the one global per singleton. Declared before
    /// any body so that a constructor can be called from anywhere.
    fn declare_objects(&mut self, component: &TypedComponent) -> Result<(), CodegenError> {
        let i32t = self.context.i32_type();
        for o in &component.objects {
            let mut members: Vec<BasicTypeEnum<'ctx>> = vec![i32t.into(), i32t.into()];
            for field in &o.fields {
                members.push(self.basic_type(field.ty).ok_or_else(|| {
                    CodegenError::internal(format!("field `{}` has no storage type", field.name))
                })?);
            }
            let layout = self.context.opaque_struct_type(o.name);
            layout.set_body(&members, false);
            self.objects.insert(o.name, layout);

            let params: Vec<BasicMetadataTypeEnum<'ctx>> = o
                .fields
                .iter()
                .take(o.param_count)
                .filter_map(|f| self.basic_type(f.ty))
                .map(Into::into)
                .collect();
            let constructor =
                self.module
                    .add_function(&o.symbol, self.ptr().fn_type(&params, false), None);
            self.stamp_target(constructor);
            self.functions.insert(o.symbol.clone(), constructor);

            if o.singleton {
                let global =
                    self.module
                        .add_global(self.ptr_type(), None, &format!("{}$instance", o.name));
                global.set_linkage(Linkage::Internal);
                global.set_initializer(&self.ptr_type().const_null());
                self.singletons.insert(o.name, global);
            }
        }
        Ok(())
    }

    fn declare_dispatches(&mut self, component: &TypedComponent) -> Result<(), CodegenError> {
        for d in &component.dispatches {
            let params: Vec<BasicMetadataTypeEnum<'ctx>> = d
                .params
                .iter()
                .filter_map(|t| self.basic_type(*t))
                .map(Into::into)
                .collect();
            let fn_type = match self.basic_type(d.returns) {
                Some(ret) => ret.fn_type(&params, false),
                None => self.context.void_type().fn_type(&params, false),
            };
            let value = self.module.add_function(&d.symbol, fn_type, None);
            self.stamp_target(value);
            self.functions.insert(d.symbol.clone(), value);
        }
        Ok(())
    }

    fn stamp_target(&self, function: FunctionValue<'ctx>) {
        let attribute = self
            .context
            .create_string_attribute("target-cpu", &self.cpu);
        function.add_attribute(AttributeLoc::Function, attribute);
    }

    /// The MPI shims, declared only for a component that calls one. A program
    /// that never touches MPI must not name an MPI symbol at all, so that it
    /// keeps linking without `libmpi`.
    fn declare_mpi(&mut self) {
        let i32t = self.context.i32_type();
        let void = self.context.void_type();
        let shims = [
            ("fortress_mpi_init", false),
            ("fortress_mpi_comm_rank", true),
            ("fortress_mpi_comm_size", true),
            ("fortress_mpi_finalize", false),
        ];
        for (name, returns_rank) in shims {
            let ty = if returns_rank {
                i32t.fn_type(&[], false)
            } else {
                void.fn_type(&[], false)
            };
            self.module.add_function(name, ty, Some(Linkage::External));
        }
    }

    /// Declared before any body is defined, so recursion and forward calls
    /// resolve.
    fn declare_functions(&mut self, component: &TypedComponent) -> Result<(), CodegenError> {
        for f in &component.functions {
            let params: Vec<BasicMetadataTypeEnum<'ctx>> = f
                .params
                .iter()
                .filter_map(|p| self.basic_type(p.ty))
                .map(Into::into)
                .collect();
            let fn_type = match self.basic_type(f.return_type) {
                Some(ret) => ret.fn_type(&params, false),
                None => self.context.void_type().fn_type(&params, false),
            };
            let value = self.module.add_function(&f.name, fn_type, None);
            self.stamp_target(value);
            self.functions.insert(f.name.clone(), value);
        }
        Ok(())
    }

    fn define_function(&mut self, f: &TypedFn) -> Result<(), CodegenError> {
        let function = *self
            .functions
            .get(&f.name)
            .ok_or_else(|| CodegenError::internal(format!("undeclared function `{}`", f.name)))?;

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let mut scope = HashMap::new();
        for (index, param) in f.params.iter().enumerate() {
            let value = function.get_nth_param(index as u32).ok_or_else(|| {
                CodegenError::internal(format!("missing parameter {index} of `{}`", f.name))
            })?;
            value.set_name(&param.name);
            scope.insert(param.name.clone(), Slot::Value(value));
        }
        self.scopes.push(scope);

        let body = self.expr(&f.body);
        self.scopes.pop();
        let body = body?;

        match body {
            Some(value) => self.builder.build_return(Some(&value)),
            None => self.builder.build_return(None),
        }
        .map_err(CodegenError::from_builder)?;
        Ok(())
    }

    /// One allocation, the parameters stored into it, then the computed
    /// fields in declaration order. Every object goes through the scanned
    /// allocator with no exception for an all-scalar one: saving a word there
    /// re-arms the landmine `runtime/tests/array_trace.c` exists to catch.
    fn define_constructor(&mut self, o: &TypedObject) -> Result<(), CodegenError> {
        let function = *self
            .functions
            .get(&o.symbol)
            .ok_or_else(|| CodegenError::internal(format!("undeclared `{}`", o.symbol)))?;
        let layout = *self
            .objects
            .get(o.name)
            .ok_or_else(|| CodegenError::internal(format!("no layout for `{}`", o.name)))?;

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let size = layout
            .size_of()
            .ok_or_else(|| CodegenError::internal(format!("`{}` has no size", o.name)))?;
        let tag = self.context.i32_type().const_int(u64::from(o.tag), false);
        let object = self
            .call_runtime(OBJECT_ALLOC, &[size.into(), tag.into()], true)?
            .ok_or_else(|| CodegenError::internal("the allocator returned nothing".to_owned()))?
            .into_pointer_value();

        self.scopes.push(HashMap::new());
        let stored = self.store_fields(function, o, layout, object);
        self.scopes.pop();
        stored?;

        self.builder
            .build_return(Some(&object))
            .map_err(CodegenError::from_builder)?;
        Ok(())
    }

    fn store_fields(
        &mut self,
        function: FunctionValue<'ctx>,
        o: &TypedObject,
        layout: StructType<'ctx>,
        object: PointerValue<'ctx>,
    ) -> Result<(), CodegenError> {
        for (index, field) in o.fields.iter().enumerate().take(o.param_count) {
            let position = u32::try_from(index)
                .map_err(|_| CodegenError::internal("too many fields".to_owned()))?;
            let value = function.get_nth_param(position).ok_or_else(|| {
                CodegenError::internal(format!("missing parameter {index} of `{}`", o.symbol))
            })?;
            value.set_name(&field.name);
            self.bind(&field.name, value);
            self.store_field(layout, object, position, &field.name, value)?;
        }
        for (offset, (init, field)) in o
            .initializers
            .iter()
            .zip(o.fields.iter().skip(o.param_count))
            .enumerate()
        {
            let index = o.param_count.saturating_add(offset);
            let position = u32::try_from(index)
                .map_err(|_| CodegenError::internal("too many fields".to_owned()))?;
            let value = self.operand(init)?;
            value.set_name(&field.name);
            self.bind(&field.name, value);
            self.store_field(layout, object, position, &field.name, value)?;
        }
        Ok(())
    }

    fn bind(&mut self, name: &str, value: BasicValueEnum<'ctx>) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), Slot::Value(value));
        }
    }

    /// Field `index` sits at struct member `index + 2`: the tag and its pad.
    fn field_slot(
        &self,
        layout: StructType<'ctx>,
        object: PointerValue<'ctx>,
        index: u32,
        name: &str,
    ) -> Result<PointerValue<'ctx>, CodegenError> {
        self.builder
            .build_struct_gep(layout, object, index.saturating_add(2), name)
            .map_err(CodegenError::from_builder)
    }

    fn store_field(
        &self,
        layout: StructType<'ctx>,
        object: PointerValue<'ctx>,
        index: u32,
        name: &str,
        value: BasicValueEnum<'ctx>,
    ) -> Result<(), CodegenError> {
        let slot = self.field_slot(layout, object, index, name)?;
        self.builder
            .build_store(slot, value)
            .map_err(CodegenError::from_builder)?;
        Ok(())
    }

    /// The address of one field of one object, with the base evaluated exactly
    /// once. A compound assignment reads and writes through the same address,
    /// so evaluating the receiver twice would be a second side effect.
    fn field_pointer(
        &mut self,
        base: &TypedExpr,
        index: u32,
    ) -> Result<PointerValue<'ctx>, CodegenError> {
        let Type::Object(name) = base.ty else {
            return Err(CodegenError::internal(
                "a field reference on something that is not an object".to_owned(),
            ));
        };
        let layout = *self
            .objects
            .get(name)
            .ok_or_else(|| CodegenError::internal(format!("no layout for `{name}`")))?;
        let object = self.operand(base)?.into_pointer_value();
        self.field_slot(layout, object, index, "field")
    }

    fn load_field(
        &mut self,
        base: &TypedExpr,
        index: u32,
        ty: Type,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let slot = self.field_pointer(base, index)?;
        let loaded = self
            .basic_type(ty)
            .ok_or_else(|| CodegenError::internal("a field with no storage type".to_owned()))?;
        self.builder
            .build_load(loaded, slot, "field")
            .map_err(CodegenError::from_builder)
    }

    /// The table, already flattened by the type checker. Every leaf is a direct
    /// call, so the callees stay ordinary inlinable functions.
    fn define_dispatch(&mut self, d: &DispatchFn) -> Result<(), CodegenError> {
        let function = *self
            .functions
            .get(&d.symbol)
            .ok_or_else(|| CodegenError::internal(format!("undeclared `{}`", d.symbol)))?;
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let mut arguments = Vec::with_capacity(d.params.len());
        for index in 0..d.params.len() {
            let position = u32::try_from(index)
                .map_err(|_| CodegenError::internal("too many parameters".to_owned()))?;
            arguments.push(function.get_nth_param(position).ok_or_else(|| {
                CodegenError::internal(format!("missing parameter {index} of `{}`", d.symbol))
            })?);
        }
        let name = self
            .builder
            .build_global_string_ptr(&d.set_name, "dispatch.name")
            .map_err(CodegenError::from_builder)?
            .as_pointer_value();

        self.dispatch_node(function, &d.tree, &arguments, name, d.returns)
    }

    fn dispatch_node(
        &mut self,
        function: FunctionValue<'ctx>,
        node: &DispatchNode,
        arguments: &[BasicValueEnum<'ctx>],
        name: PointerValue<'ctx>,
        returns: Type,
    ) -> Result<(), CodegenError> {
        match node {
            DispatchNode::Call { symbol } => {
                let callee = *self.functions.get(symbol).ok_or_else(|| {
                    CodegenError::internal(format!("no dispatch target `{symbol}`"))
                })?;
                let lowered: Vec<BasicMetadataValueEnum<'ctx>> =
                    arguments.iter().copied().map(Into::into).collect();
                let call = self
                    .builder
                    .build_call(callee, &lowered, "leaf")
                    .map_err(CodegenError::from_builder)?;
                match call.try_as_basic_value().basic() {
                    Some(value) if returns != Type::Void => self.builder.build_return(Some(&value)),
                    _ => self.builder.build_return(None),
                }
                .map_err(CodegenError::from_builder)?;
                Ok(())
            }
            DispatchNode::Switch { position, arms } => {
                let i32t = self.context.i32_type();
                let base = arguments
                    .get(*position)
                    .ok_or_else(|| CodegenError::internal("no argument to dispatch on".to_owned()))?
                    .into_pointer_value();
                // The tag is at offset 0, so this is the load, not a GEP.
                let tag = self
                    .builder
                    .build_load(i32t, base, "tag")
                    .map_err(CodegenError::from_builder)?
                    .into_int_value();

                let fail = self.context.append_basic_block(function, "dispatch.fail");
                let mut cases = Vec::with_capacity(arms.len());
                let mut children = Vec::with_capacity(arms.len());
                for (value, child) in arms {
                    let arm = self.context.append_basic_block(function, "dispatch.arm");
                    cases.push((i32t.const_int(u64::from(*value), false), arm));
                    children.push((arm, child));
                }
                self.builder
                    .build_switch(tag, fail, &cases)
                    .map_err(CodegenError::from_builder)?;

                // One fail arm per switch, not one per function: a shared one
                // would use a tag that is not defined on every path into it,
                // and the module would not verify.
                self.builder.position_at_end(fail);
                let at = i32t.const_int(*position as u64, false);
                self.call_runtime(
                    DISPATCH_FAILED,
                    &[name.into(), at.into(), tag.into()],
                    false,
                )?;
                self.builder
                    .build_unreachable()
                    .map_err(CodegenError::from_builder)?;

                for (arm, child) in children {
                    self.builder.position_at_end(arm);
                    self.dispatch_node(function, child, arguments, name, returns)?;
                }
                Ok(())
            }
        }
    }

    /// `main` exists so the ELF has a C entry point. It starts the runtime,
    /// calls `run` and returns 0; a Fortress program's exit status is not its
    /// return value.
    fn emit_main(&mut self, component: &TypedComponent) -> Result<(), CodegenError> {
        let i32t = self.context.i32_type();
        let main = self
            .module
            .add_function("main", i32t.fn_type(&[], false), None);
        self.stamp_target(main);
        let entry = self.context.append_basic_block(main, "entry");
        self.builder.position_at_end(entry);

        // Before anything else, including the first allocation: the collector
        // has to be up before the program can hand it work.
        self.call_runtime(RUNTIME_INIT, &[], false)?;

        // Singletons, in declaration order, before `run` can name one. Their
        // initializers cannot reach another singleton, a constructor or a user
        // function, so declaration order is the only order there is.
        for o in component.objects.iter().filter(|o| o.singleton) {
            let (Some(constructor), Some(global)) = (
                self.functions.get(&o.symbol).copied(),
                self.singletons.get(o.name).copied(),
            ) else {
                return Err(CodegenError::internal(format!(
                    "singleton `{}` was never declared",
                    o.name
                )));
            };
            let instance = self
                .builder
                .build_call(constructor, &[], "singleton")
                .map_err(CodegenError::from_builder)?
                .try_as_basic_value()
                .basic()
                .ok_or_else(|| CodegenError::internal(format!("`{}` built nothing", o.symbol)))?;
            self.builder
                .build_store(global.as_pointer_value(), instance)
                .map_err(CodegenError::from_builder)?;
        }

        if let Some(run) = self.functions.get(ENTRY) {
            self.builder
                .build_call(*run, &[], "run")
                .map_err(CodegenError::from_builder)?;
        }
        self.builder
            .build_return(Some(&i32t.const_int(0, false)))
            .map_err(CodegenError::from_builder)?;
        Ok(())
    }

    // ------------------------------------------------------------- values

    fn lookup(&self, name: &str) -> Option<Slot<'ctx>> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn read(&self, name: &str) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        match self.lookup(name) {
            Some(Slot::Value(v)) => Ok(v),
            Some(Slot::Cell { pointer, ty }) => self
                .builder
                .build_load(ty, pointer, name)
                .map_err(CodegenError::from_builder),
            None => Err(CodegenError::internal(format!("unbound name `{name}`"))),
        }
    }

    fn expr(&mut self, e: &TypedExpr) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        match &e.kind {
            // Void has no representation, so there is nothing to produce. The
            // same `None` a call returning Void already yields.
            TypedExprKind::Unit => Ok(None),
            TypedExprKind::IntConst(value) => {
                let ty = match e.ty {
                    Type::ZZ32 => self.context.i32_type(),
                    _ => self.context.i64_type(),
                };
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                Ok(Some(ty.const_int(*value as u64, true).into()))
            }
            TypedExprKind::FloatConst(value) => {
                Ok(Some(self.context.f64_type().const_float(*value).into()))
            }
            TypedExprKind::BoolConst(value) => Ok(Some(
                self.context
                    .bool_type()
                    .const_int(u64::from(*value), false)
                    .into(),
            )),
            TypedExprKind::StrConst(value) => {
                let global = self
                    .builder
                    .build_global_string_ptr(value, "str")
                    .map_err(CodegenError::from_builder)?;
                Ok(Some(global.as_pointer_value().into()))
            }
            TypedExprKind::Var(name) => self.read(name).map(Some),
            TypedExprKind::Apply { target, args } => self.apply(target, args, e.ty),
            TypedExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.if_expr(cond, then_branch, else_branch.as_deref(), e.ty),
            TypedExprKind::Block { items, tail } => self.block(items, tail.as_deref()),
            TypedExprKind::ArrayLit { elem, items } => self.array_literal(*elem, items).map(Some),
            TypedExprKind::Index {
                base,
                indices,
                elem,
            } => {
                let slot = self.slot(base, indices)?;
                self.load_element(*elem, slot).map(Some)
            }
            TypedExprKind::While { cond, body } => self.while_loop(cond, body).map(|()| None),
            TypedExprKind::ParallelFor {
                binder,
                lo,
                hi,
                body,
                captures,
                reductions,
                symbol,
                sequential,
            } => self
                .parallel_for(ParallelLoop {
                    binder,
                    lo,
                    hi,
                    body,
                    captures,
                    reductions,
                    symbol,
                    sequential: *sequential,
                })
                .map(|()| None),
            TypedExprKind::Atomic { body } => self.atomic(body),
            TypedExprKind::TypeCase {
                subject,
                arms,
                else_branch,
            } => self.typecase(subject, arms, else_branch, e.ty),
            TypedExprKind::Label { name, body } => self.label(name, body, e.ty),
            TypedExprKind::Exit { name, value } => self.exit(name, value.as_deref(), e.ty),
            TypedExprKind::Field { base, index } => self.load_field(base, *index, e.ty).map(Some),
            TypedExprKind::Singleton { name } => {
                let global =
                    self.singletons.get(name).copied().ok_or_else(|| {
                        CodegenError::internal(format!("no instance of `{name}`"))
                    })?;
                self.builder
                    .build_load(self.ptr(), global.as_pointer_value(), name)
                    .map_err(CodegenError::from_builder)
                    .map(Some)
            }
        }
    }

    /// The outliner.
    ///
    /// The body becomes a real function `symbol(i64 index, ptr env, i64 chunk)`
    /// so a worker thread can call it, and the values it reads from the
    /// enclosing scope are copied into one environment struct. The struct is
    /// filled and allocated ONCE, here, before the loop starts -- allocation
    /// inside the parallel region is what makes an allocating loop collect N
    /// times as often and run slower than the serial one.
    ///
    /// `chunk` went LAST deliberately. Putting the worker index second would
    /// renumber `env` from `get_nth_param(1)` to `(2)` below, and
    /// `get_nth_param` returns an `Option` -- a wrong index is a run-time
    /// internal error rather than a compile error.
    ///
    /// A `seq(...)` loop takes the same path. The runtime runs a range below
    /// its threshold inline anyway, so one lowering serves both and there is no
    /// second code path to keep correct.
    fn parallel_for(&mut self, loop_: ParallelLoop<'_>) -> Result<(), CodegenError> {
        let lo_value = self.operand(loop_.lo)?;
        let hi_value = self.operand(loop_.hi)?;

        // The environment: one field per captured value, in the order the
        // checker recorded them, which is sorted and therefore deterministic.
        // A by-reference capture carries the ADDRESS of the caller's storage,
        // so its field is a pointer whatever the value's type is.
        let mut field_types: Vec<BasicTypeEnum<'ctx>> = loop_
            .captures
            .iter()
            .map(|c| {
                if c.by_ref {
                    return Ok(self.ptr());
                }
                self.basic_type(c.ty).ok_or_else(|| {
                    CodegenError::internal("a captured value has no type".to_owned())
                })
            })
            .collect::<Result<_, _>>()?;
        if !loop_.reductions.is_empty() {
            field_types.push(self.ptr());
        }
        let env_type = self.context.struct_type(&field_types, false);

        let partials = self.reduction_alloc(loop_.reductions)?;
        if let Some(block) = partials {
            self.reduction_init(block, loop_.reductions)?;
        }

        // Scanned, not atomic: a capture may be a String or an Array, and the
        // collector has to see through the environment to it while a worker is
        // still using it.
        let env = if field_types.is_empty() {
            self.ptr_type().const_null()
        } else {
            let size = env_type.size_of().ok_or_else(|| {
                CodegenError::internal("the loop environment has no size".to_owned())
            })?;
            let raw = self
                .call_runtime(ENV_ALLOC, &[size.into()], true)?
                .ok_or_else(|| CodegenError::internal("no environment returned".to_owned()))?;
            let pointer = raw.into_pointer_value();
            for (index, capture) in loop_.captures.iter().enumerate() {
                let value = if capture.by_ref {
                    self.address_of(&capture.name)?.into()
                } else {
                    self.load_name(&capture.name)?
                };
                let slot = self
                    .builder
                    .build_struct_gep(env_type, pointer, index as u32, "env.slot")
                    .map_err(CodegenError::from_builder)?;
                self.builder
                    .build_store(slot, value)
                    .map_err(CodegenError::from_builder)?;
            }
            if let Some(block) = partials {
                let slot = self
                    .builder
                    .build_struct_gep(
                        env_type,
                        pointer,
                        loop_.captures.len() as u32,
                        "env.partials",
                    )
                    .map_err(CodegenError::from_builder)?;
                self.builder
                    .build_store(slot, block)
                    .map_err(CodegenError::from_builder)?;
            }
            pointer
        };

        let outlined = self.declare_loop_body(loop_.symbol);
        self.define_loop_body(outlined, &loop_, env_type)?;

        let workers = if loop_.sequential {
            // One worker means the runtime runs the whole range on the calling
            // thread. `seq` is a promise about ORDER, so it cannot be handed to
            // a pool however small the range is.
            self.context.i64_type().const_int(1, false)
        } else {
            self.context.i64_type().const_zero()
        };
        self.call_runtime(
            PARALLEL_FOR,
            &[
                lo_value,
                hi_value,
                outlined.as_global_value().as_pointer_value().into(),
                env.into(),
                workers.into(),
            ],
            false,
        )?;

        // The merge, and it is emitted HERE rather than in the runtime for two
        // reasons: it is typed, and the shim has no type knowledge; and it is
        // after `fortress_parallel_for`, which blocks on its done-wait before
        // it returns, so "after the call" IS "after the join barrier".
        //
        // An empty range needs no special case: the runtime returns early on
        // `hi <= lo`, the slots still hold Identity, and the fold yields the
        // variable's entry value.
        if let Some(block) = partials {
            self.reduction_merge(block, loop_.reductions)?;
        }
        Ok(())
    }

    /// One scanned-free block of `workers x reductions` cacheline slots, all
    /// Identity. The row count belongs to the runtime because a worker writes
    /// row `chunk`; the stride is handed over so the two cannot disagree.
    fn reduction_alloc(
        &mut self,
        reductions: &[TypedReduction],
    ) -> Result<Option<PointerValue<'ctx>>, CodegenError> {
        if reductions.is_empty() {
            return Ok(None);
        }
        let i64t = self.context.i64_type();
        let raw = self
            .call_runtime(
                REDUCTION_ALLOC,
                &[
                    i64t.const_int(reductions.len() as u64, false).into(),
                    i64t.const_int(REDUCTION_STRIDE, false).into(),
                ],
                true,
            )?
            .ok_or_else(|| CodegenError::internal("no accumulators returned".to_owned()))?;
        Ok(Some(raw.into_pointer_value()))
    }

    /// The address of one worker's slot for one reduction:
    /// `block + (chunk * reductions + k) * stride`.
    fn reduction_slot(
        &self,
        block: PointerValue<'ctx>,
        chunk: IntValue<'ctx>,
        count: usize,
        k: usize,
    ) -> Result<PointerValue<'ctx>, CodegenError> {
        let i64t = self.context.i64_type();
        let row = self
            .builder
            .build_int_mul(chunk, i64t.const_int(count as u64, false), "reduce.row")
            .map_err(CodegenError::from_builder)?;
        let cell = self
            .builder
            .build_int_add(row, i64t.const_int(k as u64, false), "reduce.cell")
            .map_err(CodegenError::from_builder)?;
        let offset = self
            .builder
            .build_int_mul(
                cell,
                i64t.const_int(REDUCTION_STRIDE, false),
                "reduce.offset",
            )
            .map_err(CodegenError::from_builder)?;
        // Address arithmetic rather than a GEP, because inkwell's `build_gep`
        // is an `unsafe fn` and this crate denies unsafe code. The block is
        // kept alive by the environment struct, which is scanned and holds the
        // BASE pointer, so a derived interior pointer retains nothing on its
        // own and needs to retain nothing.
        let base = self
            .builder
            .build_ptr_to_int(block, i64t, "reduce.base")
            .map_err(CodegenError::from_builder)?;
        let address = self
            .builder
            .build_int_add(base, offset, "reduce.address")
            .map_err(CodegenError::from_builder)?;
        self.builder
            .build_int_to_ptr(address, self.ptr_type(), "reduce.slot")
            .map_err(CodegenError::from_builder)
    }

    /// Folds every worker's accumulator into the variable, on the calling
    /// thread, in worker order, starting from the value it held at loop entry
    /// -- `reduction.tex:70-73`. A fixed order is what makes the answer
    /// byte-identical run to run for a fixed worker count.
    ///
    /// RR64 is deterministic per worker count and NOT across worker counts.
    /// That is inherent to reassociation, `reduction.tex:43-46` permits it, and
    /// the gate pins FORTRESS_WORKERS rather than asserting an equality that is
    /// not true. ZZ32 and ZZ64 are unaffected: two's-complement addition is
    /// associative whatever the grouping, overflow included.
    /// Every slot to its reduction's identity, before the loop runs.
    ///
    /// The runtime memsets the block to zero, which is the identity for `+` and
    /// `-` on all three reducible types and is NOT the identity for `*`. Doing
    /// it here instead of there is what lets the identity be a fact about the
    /// operator rather than about the allocator -- and the allocator has no
    /// type knowledge to decide it with.
    fn reduction_init(
        &mut self,
        block: PointerValue<'ctx>,
        reductions: &[TypedReduction],
    ) -> Result<(), CodegenError> {
        if reductions.iter().all(|r| r.op == ArithOp::Add) {
            // The runtime's memset already wrote this one, and emitting a loop
            // to write zeroes over zeroes would change the IR of every program
            // that already had a reduction.
            return Ok(());
        }
        let i64t = self.context.i64_type();
        let workers = self
            .call_runtime(REDUCTION_WORKERS, &[], true)?
            .ok_or_else(|| CodegenError::internal("no worker count returned".to_owned()))?
            .into_int_value();
        let function = self.current_function()?;
        let cond_bb = self.context.append_basic_block(function, "init.cond");
        let body_bb = self.context.append_basic_block(function, "init.body");
        let end_bb = self.context.append_basic_block(function, "init.end");

        let counter = self.entry_alloca("init.w", i64t.into())?;
        self.builder
            .build_store(counter, i64t.const_zero())
            .map_err(CodegenError::from_builder)?;
        self.branch_to(cond_bb)?;

        self.builder.position_at_end(cond_bb);
        let w = self
            .builder
            .build_load(i64t, counter, "init.w")
            .map_err(CodegenError::from_builder)?
            .into_int_value();
        let more = self
            .builder
            .build_int_compare(inkwell::IntPredicate::SLT, w, workers, "init.more")
            .map_err(CodegenError::from_builder)?;
        self.builder
            .build_conditional_branch(more, body_bb, end_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(body_bb);
        for (k, reduction) in reductions.iter().enumerate() {
            let slot = self.reduction_slot(block, w, reductions.len(), k)?;
            let identity = self.identity_of(reduction)?;
            self.builder
                .build_store(slot, identity)
                .map_err(CodegenError::from_builder)?;
        }
        let next = self
            .builder
            .build_int_add(w, i64t.const_int(1, false), "init.next")
            .map_err(CodegenError::from_builder)?;
        self.builder
            .build_store(counter, next)
            .map_err(CodegenError::from_builder)?;
        self.branch_to(cond_bb)?;
        self.builder.position_at_end(end_bb);
        Ok(())
    }

    /// 0 for `+` and `-`, 1 for `*`, at the reduction's own type.
    fn identity_of(
        &self,
        reduction: &TypedReduction,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let one = reduction.op == ArithOp::Mul;
        // THE TYPE'S OWN EXTREMUM for `MAX` and `MIN`, which is the whole
        // reason the identity moved out of the runtime's memset: it is a fact
        // about the operator AND the type, and the allocator knows neither. A
        // MAX slot starting at zero reports 0 as the maximum of a set of
        // negative numbers, silently.
        match (reduction.op, reduction.ty) {
            (ArithOp::Max, Type::ZZ32) => {
                return Ok(self
                    .context
                    .i32_type()
                    .const_int(u64::from(i32::MIN.cast_unsigned()), false)
                    .into())
            }
            (ArithOp::Min, Type::ZZ32) => {
                return Ok(self
                    .context
                    .i32_type()
                    .const_int(u64::from(i32::MAX.cast_unsigned()), false)
                    .into())
            }
            (ArithOp::Max, Type::ZZ64) => {
                return Ok(self
                    .context
                    .i64_type()
                    .const_int(i64::MIN.cast_unsigned(), false)
                    .into())
            }
            (ArithOp::Min, Type::ZZ64) => {
                return Ok(self
                    .context
                    .i64_type()
                    .const_int(i64::MAX.cast_unsigned(), false)
                    .into())
            }
            (ArithOp::Max, Type::RR64) => {
                return Ok(self
                    .context
                    .f64_type()
                    .const_float(f64::NEG_INFINITY)
                    .into())
            }
            (ArithOp::Min, Type::RR64) => {
                return Ok(self.context.f64_type().const_float(f64::INFINITY).into())
            }
            _ => {}
        }
        Ok(match reduction.ty {
            Type::ZZ32 => self
                .context
                .i32_type()
                .const_int(u64::from(one), false)
                .into(),
            Type::ZZ64 => self
                .context
                .i64_type()
                .const_int(u64::from(one), false)
                .into(),
            Type::RR64 => self
                .context
                .f64_type()
                .const_float(if one { 1.0 } else { 0.0 })
                .into(),
            other => {
                return Err(CodegenError::internal(format!(
                    "`{}` is not a reducible type",
                    other.name()
                )))
            }
        })
    }

    fn reduction_merge(
        &mut self,
        block: PointerValue<'ctx>,
        reductions: &[TypedReduction],
    ) -> Result<(), CodegenError> {
        let i64t = self.context.i64_type();
        let workers = self
            .call_runtime(REDUCTION_WORKERS, &[], true)?
            .ok_or_else(|| CodegenError::internal("no worker count returned".to_owned()))?
            .into_int_value();

        let function = self
            .builder
            .get_insert_block()
            .and_then(inkwell::basic_block::BasicBlock::get_parent)
            .ok_or_else(|| CodegenError::internal("no enclosing function".to_owned()))?;
        let cond_bb = self.context.append_basic_block(function, "reduce.cond");
        let body_bb = self.context.append_basic_block(function, "reduce.body");
        let end_bb = self.context.append_basic_block(function, "reduce.end");

        let counter = self.entry_alloca("reduce.w", i64t.into())?;
        self.builder
            .build_store(counter, i64t.const_zero())
            .map_err(CodegenError::from_builder)?;
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(cond_bb);
        let w = self
            .builder
            .build_load(i64t, counter, "reduce.w")
            .map_err(CodegenError::from_builder)?
            .into_int_value();
        let more = self
            .builder
            .build_int_compare(inkwell::IntPredicate::SLT, w, workers, "reduce.more")
            .map_err(CodegenError::from_builder)?;
        self.builder
            .build_conditional_branch(more, body_bb, end_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(body_bb);
        for (k, reduction) in reductions.iter().enumerate() {
            let ty = self.basic_type(reduction.ty).ok_or_else(|| {
                CodegenError::internal("a reduction variable has no type".to_owned())
            })?;
            let slot = self.reduction_slot(block, w, reductions.len(), k)?;
            let partial = self
                .builder
                .build_load(ty, slot, "reduce.partial")
                .map_err(CodegenError::from_builder)?;
            let target = self.address_of(&reduction.name)?;
            let current = self
                .builder
                .build_load(ty, target, &reduction.name)
                .map_err(CodegenError::from_builder)?;
            // The reduction's OWN operator. `+=` and `-=` share `Add` -- `-=`
            // accumulated `Identity - e`, so the group inverse is already
            // inside the partial -- and `*=` folds with `Mul`, because adding
            // the partials of a product is not a product.
            let merged = self.arith(reduction.op, reduction.ty, current, partial)?;
            self.builder
                .build_store(target, merged)
                .map_err(CodegenError::from_builder)?;
        }
        let next = self
            .builder
            .build_int_add(w, i64t.const_int(1, false), "reduce.next")
            .map_err(CodegenError::from_builder)?;
        self.builder
            .build_store(counter, next)
            .map_err(CodegenError::from_builder)?;
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(end_bb);
        Ok(())
    }

    /// `atomic e`.
    ///
    /// A block that does nothing but write recognised reduction variables
    /// needs no lock: every worker is adding into storage only it can see, and
    /// `reduction.tex:40-42` gives up atomic's visibility guarantee for exactly
    /// that name. Without this, the one corpus file M5 unlocks that reaches the
    /// pool would take a process-wide mutex 30000 times -- measured at 13.7x
    /// SLOWER than the serial loop it replaced.
    fn atomic(&mut self, body: &TypedExpr) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        if self.is_pure_reduction(body) {
            return self.expr(body);
        }
        self.call_runtime(ATOMIC_ENTER, &[], false)?;
        let value = self.expr(body)?;
        self.call_runtime(ATOMIC_LEAVE, &[], false)?;
        Ok(value)
    }

    fn is_pure_reduction(&self, body: &TypedExpr) -> bool {
        if self.reductions.is_empty() {
            return false;
        }
        let TypedExprKind::Block { items, tail: None } = &body.kind else {
            return false;
        };
        !items.is_empty()
            && items.iter().all(|item| {
                matches!(
                    item,
                    TypedBlockItem::Assign {
                        target: AssignTarget::Var { name, .. },
                        op: Some(_),
                        ..
                    } if self.reductions.contains(name)
                )
            })
    }

    fn declare_loop_body(&mut self, symbol: &str) -> FunctionValue<'ctx> {
        if let Some(existing) = self.functions.get(symbol) {
            return *existing;
        }
        let i64t = self.context.i64_type();
        let ty = self
            .context
            .void_type()
            .fn_type(&[i64t.into(), self.ptr().into(), i64t.into()], false);
        let function = self.module.add_function(symbol, ty, None);
        self.functions.insert(symbol.to_owned(), function);
        function
    }

    /// Emits the outlined body. The builder is parked and restored, because the
    /// caller is in the middle of emitting the function this loop appears in.
    fn define_loop_body(
        &mut self,
        function: FunctionValue<'ctx>,
        loop_: &ParallelLoop<'_>,
        env_type: inkwell::types::StructType<'ctx>,
    ) -> Result<(), CodegenError> {
        let resume = self.builder.get_insert_block();
        let saved = std::mem::take(&mut self.scopes);
        let saved_reductions = std::mem::take(&mut self.reductions);

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let index = function
            .get_nth_param(0)
            .ok_or_else(|| CodegenError::internal("the loop body has no index".to_owned()))?;
        let env = function
            .get_nth_param(1)
            .ok_or_else(|| CodegenError::internal("the loop body has no environment".to_owned()))?
            .into_pointer_value();
        let chunk = function
            .get_nth_param(2)
            .ok_or_else(|| CodegenError::internal("the loop body has no worker index".to_owned()))?
            .into_int_value();

        let mut scope: HashMap<String, Slot<'ctx>> = HashMap::new();
        scope.insert(loop_.binder.to_owned(), Slot::Value(index));
        for (position, capture) in loop_.captures.iter().enumerate() {
            let ty = self
                .basic_type(capture.ty)
                .ok_or_else(|| CodegenError::internal("a captured value has no type".to_owned()))?;
            let field = self
                .builder
                .build_struct_gep(env_type, env, position as u32, "env.read")
                .map_err(CodegenError::from_builder)?;
            let slot = if capture.by_ref {
                // The field holds the caller's `alloca`, so the read and the
                // write inside the body both land on live storage. Its
                // lifetime is safe by construction: the runtime blocks on its
                // done-wait before `fortress_parallel_for` returns.
                let pointer = self
                    .builder
                    .build_load(self.ptr(), field, &capture.name)
                    .map_err(CodegenError::from_builder)?
                    .into_pointer_value();
                Slot::Cell { pointer, ty }
            } else {
                let value = self
                    .builder
                    .build_load(ty, field, &capture.name)
                    .map_err(CodegenError::from_builder)?;
                Slot::Value(value)
            };
            scope.insert(capture.name.clone(), slot);
        }

        // A reduction variable binds to this worker's own accumulator, which is
        // why nothing downstream needs to know it is one: `l += e` is the
        // ordinary compound assignment through an ordinary cell, and the cell
        // is private.
        if !loop_.reductions.is_empty() {
            let field = self
                .builder
                .build_struct_gep(
                    env_type,
                    env,
                    loop_.captures.len() as u32,
                    "env.partials.read",
                )
                .map_err(CodegenError::from_builder)?;
            let block = self
                .builder
                .build_load(self.ptr(), field, "partials")
                .map_err(CodegenError::from_builder)?
                .into_pointer_value();
            for (k, reduction) in loop_.reductions.iter().enumerate() {
                let ty = self.basic_type(reduction.ty).ok_or_else(|| {
                    CodegenError::internal("a reduction variable has no type".to_owned())
                })?;
                let pointer = self.reduction_slot(block, chunk, loop_.reductions.len(), k)?;
                scope.insert(reduction.name.clone(), Slot::Cell { pointer, ty });
                self.reductions.insert(reduction.name.clone());
            }
        }
        self.scopes.push(scope);

        let emitted = self.expr(loop_.body);
        self.scopes.pop();
        emitted?;

        self.builder
            .build_return(None)
            .map_err(CodegenError::from_builder)?;

        self.scopes = saved;
        self.reductions = saved_reductions;
        if let Some(block) = resume {
            self.builder.position_at_end(block);
        }
        Ok(())
    }

    /// Where a name's storage lives, for a capture that travels by reference
    /// and for the merge's target. Only a mutable binding has any, which is
    /// exactly the set that can be assigned to.
    fn address_of(&self, name: &str) -> Result<PointerValue<'ctx>, CodegenError> {
        match self.lookup(name) {
            Some(Slot::Cell { pointer, .. }) => Ok(pointer),
            _ => Err(CodegenError::internal(format!(
                "`{name}` needs storage to be captured by reference, and has none"
            ))),
        }
    }

    fn load_name(&mut self, name: &str) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let slot = self
            .scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name).copied())
            .ok_or_else(|| CodegenError::internal(format!("no binding `{name}` to capture")))?;
        match slot {
            Slot::Value(value) => Ok(value),
            Slot::Cell { pointer, ty } => self
                .builder
                .build_load(ty, pointer, name)
                .map_err(CodegenError::from_builder),
        }
    }

    fn array_alloc(
        &mut self,
        elem: Elem,
        count: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let bytes = self.context.i64_type().const_int(elem.bytes(), false);
        let holds_pointers = self
            .context
            .i32_type()
            .const_int(u64::from(elem.is_pointer()), false);
        self.call_runtime(
            ARRAY_ALLOC,
            &[count, bytes.into(), holds_pointers.into()],
            true,
        )?
        .ok_or_else(|| CodegenError::internal("the allocator returned nothing".to_owned()))
    }

    /// One `i64` buffer in the ENTRY BLOCK, filled here. A buffer allocated
    /// where the subscript appears would be one stack slot per iteration of any
    /// loop around it, which is the silent stack overflow `entry_alloca` exists
    /// to stop.
    ///
    /// A STRUCT OF `n` `i64`s AND NOT `[n x i64]`, and the reason is inkwell
    /// rather than LLVM: `build_gep` is an `unsafe fn` and this crate denies
    /// unsafe code, and `build_struct_gep` refuses an array pointee outright --
    /// "GEP pointee is not a struct". The two layouts are the same bytes, so
    /// the shim reads either one as `const long long *`.
    fn i64_buffer(
        &mut self,
        name: &str,
        values: &[BasicValueEnum<'ctx>],
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let i64t = self.context.i64_type();
        let fields: Vec<BasicTypeEnum<'ctx>> = vec![i64t.into(); values.len()];
        let buffer = self.context.struct_type(&fields, false);
        let slot = self.entry_alloca(name, buffer.into())?;
        for (at, value) in values.iter().enumerate() {
            let element = self
                .builder
                .build_struct_gep(
                    buffer,
                    slot,
                    u32::try_from(at).unwrap_or(u32::MAX),
                    &format!("{name}.{at}"),
                )
                .map_err(CodegenError::from_builder)?;
            self.builder
                .build_store(element, *value)
                .map_err(CodegenError::from_builder)?;
        }
        Ok(slot.into())
    }

    /// `array(m, n)` and above. RANK ONE DOES NOT COME THROUGH HERE -- it keeps
    /// `array_alloc` and the shim it always called, so every module that
    /// compiled before this milestone lowers unchanged.
    fn array_alloc_n(
        &mut self,
        elem: Elem,
        counts: &[TypedExpr],
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let mut extents = Vec::with_capacity(counts.len());
        for count in counts {
            extents.push(self.operand(count)?);
        }
        let buffer = self.i64_buffer("extents", &extents)?;
        let i64t = self.context.i64_type();
        let rank = i64t.const_int(counts.len() as u64, false);
        let bytes = i64t.const_int(elem.bytes(), false);
        let holds_pointers = self
            .context
            .i32_type()
            .const_int(u64::from(elem.is_pointer()), false);
        self.call_runtime(
            ARRAY_ALLOC_N,
            &[rank.into(), buffer, bytes.into(), holds_pointers.into()],
            true,
        )?
        .ok_or_else(|| CodegenError::internal("the allocator returned nothing".to_owned()))
    }

    fn array_literal(
        &mut self,
        elem: Elem,
        items: &[TypedExpr],
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let count = self
            .context
            .i64_type()
            .const_int(items.len() as u64, false)
            .into();
        let array = self.array_alloc(elem, count)?;
        for (index, item) in items.iter().enumerate() {
            let value = self.operand(item)?;
            let at = self
                .context
                .i64_type()
                .const_int(index as u64, false)
                .into();
            let slot = self.slot_of(array, at)?;
            self.store_element(elem, slot, value)?;
        }
        Ok(array)
    }

    /// The address of one element, bounds checked. Every read and every write
    /// goes through this, so there is one place the check can be in.
    fn slot(
        &mut self,
        base: &TypedExpr,
        indices: &[TypedExpr],
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let array = self.operand(base)?;
        // THE BASE IS EVALUATED ONCE AND SO IS EVERY INDEX, which is what makes
        // `a[i,j] += v` a single load and store through one address rather than
        // two walks of the same subscript expression.
        if let [index] = indices {
            let at = self.operand(index)?;
            return self.slot_of(array, at);
        }
        let mut at = Vec::with_capacity(indices.len());
        for index in indices {
            at.push(self.operand(index)?);
        }
        let buffer = self.i64_buffer("subscript", &at)?;
        let rank = self
            .context
            .i64_type()
            .const_int(indices.len() as u64, false);
        self.call_runtime(ARRAY_SLOT_N, &[array, rank.into(), buffer], true)?
            .ok_or_else(|| CodegenError::internal("the slot shim returned nothing".to_owned()))
    }

    fn slot_of(
        &mut self,
        array: BasicValueEnum<'ctx>,
        index: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        self.call_runtime(ARRAY_SLOT, &[array, index], true)?
            .ok_or_else(|| CodegenError::internal("the slot shim returned nothing".to_owned()))
    }

    fn load_element(
        &self,
        elem: Elem,
        slot: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let loaded = self
            .builder
            .build_load(self.element_type(elem), slot.into_pointer_value(), "elem")
            .map_err(CodegenError::from_builder)?;
        if elem != Elem::Boolean {
            return Ok(loaded);
        }
        Ok(self
            .builder
            .build_int_truncate(
                loaded.into_int_value(),
                self.context.bool_type(),
                "elem_bool",
            )
            .map_err(CodegenError::from_builder)?
            .into())
    }

    fn store_element(
        &self,
        elem: Elem,
        slot: BasicValueEnum<'ctx>,
        value: BasicValueEnum<'ctx>,
    ) -> Result<(), CodegenError> {
        let value = if elem == Elem::Boolean {
            self.builder
                .build_int_z_extend(value.into_int_value(), self.context.i8_type(), "elem_byte")
                .map_err(CodegenError::from_builder)?
                .into()
        } else {
            value
        };
        self.builder
            .build_store(slot.into_pointer_value(), value)
            .map_err(CodegenError::from_builder)?;
        Ok(())
    }

    /// `loop.cond` / `loop.body` / `loop.end`. The condition is re-evaluated at
    /// the top of every iteration, which is what makes a mutable counter work.
    fn while_loop(&mut self, cond: &TypedExpr, body: &TypedExpr) -> Result<(), CodegenError> {
        let function = self
            .builder
            .get_insert_block()
            .and_then(inkwell::basic_block::BasicBlock::get_parent)
            .ok_or_else(|| CodegenError::internal("no enclosing function".to_owned()))?;

        let cond_bb = self.context.append_basic_block(function, "loop.cond");
        let body_bb = self.context.append_basic_block(function, "loop.body");
        let end_bb = self.context.append_basic_block(function, "loop.end");

        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(cond_bb);
        let condition = self.operand(cond)?.into_int_value();
        self.builder
            .build_conditional_branch(condition, body_bb, end_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(body_bb);
        self.expr(body)?;
        // Not `body_bb`: the body may have ended somewhere else entirely, in
        // the merge block of an `if` for instance.
        self.builder
            .build_unconditional_branch(cond_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(end_bb);
        Ok(())
    }

    fn operand(&mut self, e: &TypedExpr) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        self.expr(e)?
            .ok_or_else(|| CodegenError::internal("a void expression used as a value".to_owned()))
    }

    fn apply(
        &mut self,
        target: &Target,
        args: &[TypedExpr],
        result: Type,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        match target {
            Target::Arith { op, ty } => {
                let [l, r] = self.two(args)?;
                self.arith(*op, *ty, l, r).map(Some)
            }
            Target::Compare { op, ty } => {
                let [l, r] = self.two(args)?;
                self.compare(*op, *ty, l, r).map(Some)
            }
            Target::Negate { ty } => {
                let value = self.one(args)?;
                let out = if *ty == Type::RR64 {
                    self.builder
                        .build_float_neg(value.into_float_value(), "neg")
                        .map_err(CodegenError::from_builder)?
                        .into()
                } else {
                    self.builder
                        .build_int_neg(value.into_int_value(), "neg")
                        .map_err(CodegenError::from_builder)?
                        .into()
                };
                Ok(Some(out))
            }
            // One `xor` against the i1 constant, which is what `build_not`
            // emits. Nothing branches: `NOT` has no operand it can skip.
            Target::Not => {
                let value = self.one(args)?;
                let out = self
                    .builder
                    .build_not(value.into_int_value(), "not")
                    .map_err(CodegenError::from_builder)?;
                Ok(Some(out.into()))
            }
            // The arm used to bind `..` and hardcode i64, so a Widen to RR64
            // would have emitted a `sext` into a slot LLVM expects to be a
            // double. It reads `to` now, which is why the checker may choose it.
            Target::Widen { to, .. } => {
                let value = self.one(args)?.into_int_value();
                let out: BasicValueEnum<'ctx> = if *to == Type::RR64 {
                    self.builder
                        .build_signed_int_to_float(value, self.context.f64_type(), "widen")
                        .map_err(CodegenError::from_builder)?
                        .into()
                } else {
                    self.builder
                        .build_int_s_extend(value, self.context.i64_type(), "widen")
                        .map_err(CodegenError::from_builder)?
                        .into()
                };
                Ok(Some(out))
            }
            Target::ToString { from } => {
                let value = self.one(args)?;
                let value = self.widen_boolean_for_c(*from, value)?;
                self.call_runtime(&target.symbol(), &[value], true)
            }
            Target::Concat => {
                let [l, r] = self.two(args)?;
                self.call_runtime("concat_string_string", &[l, r], true)
            }
            Target::Println { ty } => {
                if *ty == Type::Void {
                    return self.call_runtime("println_void", &[], false);
                }
                let value = self.one(args)?;
                let value = self.widen_boolean_for_c(*ty, value)?;
                self.call_runtime(&target.symbol(), &[value], false)
            }
            // Every base-exponent pair is a shim, so this is one call and no
            // instruction selection at all.
            Target::Pow { .. } => {
                let [l, r] = self.two(args)?;
                let symbol = target.symbol();
                self.call_runtime(&symbol, &[l, r], true)?
                    .ok_or_else(|| CodegenError::internal(format!("`{symbol}` returned no value")))
                    .map(Some)
            }
            Target::Print { ty } => {
                if *ty == Type::Void {
                    return self.call_runtime("print_void", &[], false);
                }
                let value = self.one(args)?;
                let value = self.widen_boolean_for_c(*ty, value)?;
                self.call_runtime(&target.symbol(), &[value], false)
            }
            // The halt does not return, but the block it sits in is still
            // terminated normally: an `if` needs both arms to reach its merge,
            // and an unreachable branch there costs nothing.
            Target::AssertFailed => {
                let value = self.one(args)?;
                self.call_runtime(&target.symbol(), &[value], false)
            }
            Target::CaseFailed => self.call_runtime(&target.symbol(), &[], false),
            Target::Mpi(op) => self.call_runtime(&target.symbol(), &[], op.returns() != Type::Void),
            Target::ArrayNew { elem, rank: 1 } => {
                let count = self.one(args)?;
                self.array_alloc(*elem, count).map(Some)
            }
            Target::ArrayNew { elem, .. } => self.array_alloc_n(*elem, args).map(Some),
            Target::ArrayLength => {
                let array = self.one(args)?;
                self.call_runtime(ARRAY_LENGTH, &[array], true)
            }
            // A dispatch function and a constructor are ordinary direct calls
            // by the time they get here; the decisions were all made already.
            Target::UserFn { name } => self.call_direct(name, args, result),
            Target::Dispatch { symbol } | Target::ObjectNew { symbol } => {
                self.call_direct(symbol, args, result)
            }
        }
    }

    fn call_direct(
        &mut self,
        symbol: &str,
        args: &[TypedExpr],
        result: Type,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        let function = *self
            .functions
            .get(symbol)
            .ok_or_else(|| CodegenError::internal(format!("unknown function `{symbol}`")))?;
        let mut lowered = Vec::with_capacity(args.len());
        for arg in args {
            lowered.push(BasicMetadataValueEnum::from(self.operand(arg)?));
        }
        let call = self
            .builder
            .build_call(function, &lowered, "call")
            .map_err(CodegenError::from_builder)?;
        Ok(if result == Type::Void {
            None
        } else {
            call.try_as_basic_value().basic()
        })
    }

    /// Fortress `Boolean` is `i1`; the C shims take `int`.
    fn widen_boolean_for_c(
        &self,
        ty: Type,
        value: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        if ty != Type::Boolean {
            return Ok(value);
        }
        let widened = self
            .builder
            .build_int_z_extend(
                value.into_int_value(),
                self.context.i32_type(),
                "bool_to_int",
            )
            .map_err(CodegenError::from_builder)?;
        Ok(widened.into())
    }

    fn call_runtime(
        &mut self,
        symbol: &str,
        args: &[BasicValueEnum<'ctx>],
        returns_value: bool,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        let function = self
            .module
            .get_function(symbol)
            .ok_or_else(|| CodegenError::internal(format!("no runtime symbol `{symbol}`")))?;
        let lowered: Vec<BasicMetadataValueEnum<'ctx>> =
            args.iter().copied().map(Into::into).collect();
        let call = self
            .builder
            .build_call(function, &lowered, symbol)
            .map_err(CodegenError::from_builder)?;
        Ok(if returns_value {
            call.try_as_basic_value().basic()
        } else {
            None
        })
    }

    fn arith(
        &mut self,
        op: ArithOp,
        ty: Type,
        l: BasicValueEnum<'ctx>,
        r: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        if ty == Type::RR64 {
            let (l, r) = (l.into_float_value(), r.into_float_value());
            // A compare and a select, not a call: `max_rr64_rr64` would be a
            // shim for two instructions, and the only thing that constructs
            // these is a reduction's fold.
            if matches!(op, ArithOp::Max | ArithOp::Min) {
                let predicate = if op == ArithOp::Max {
                    inkwell::FloatPredicate::OGT
                } else {
                    inkwell::FloatPredicate::OLT
                };
                let keep = self
                    .builder
                    .build_float_compare(predicate, l, r, "extremum")
                    .map_err(CodegenError::from_builder)?;
                return self
                    .builder
                    .build_select(keep, l, r, "extremum")
                    .map_err(CodegenError::from_builder);
            }
            let out = match op {
                ArithOp::Add => self.builder.build_float_add(l, r, "add"),
                ArithOp::Sub => self.builder.build_float_sub(l, r, "sub"),
                ArithOp::Mul => self.builder.build_float_mul(l, r, "mul"),
                ArithOp::Div => self.builder.build_float_div(l, r, "div"),
                ArithOp::Max | ArithOp::Min => {
                    return Err(CodegenError::internal("handled above".to_owned()))
                }
            };
            return Ok(out.map_err(CodegenError::from_builder)?.into());
        }
        // Max and Min first, and inline: a compare and a select, not a call.
        // `max_zz64_zz64` would be a shim for two instructions, and the only
        // thing that constructs one is a reduction's fold.
        if matches!(op, ArithOp::Max | ArithOp::Min) {
            let (li, ri) = (l.into_int_value(), r.into_int_value());
            let predicate = if op == ArithOp::Max {
                IntPredicate::SGT
            } else {
                IntPredicate::SLT
            };
            let keep = self
                .builder
                .build_int_compare(predicate, li, ri, "extremum")
                .map_err(CodegenError::from_builder)?;
            return self
                .builder
                .build_select(keep, li, ri, "extremum")
                .map_err(CodegenError::from_builder);
        }
        let (li, ri) = (l.into_int_value(), r.into_int_value());
        let out = match op {
            ArithOp::Add => self.builder.build_int_add(li, ri, "add"),
            ArithOp::Sub => self.builder.build_int_sub(li, ri, "sub"),
            ArithOp::Mul => self.builder.build_int_mul(li, ri, "mul"),
            ArithOp::Max | ArithOp::Min => {
                return Err(CodegenError::internal("handled above".to_owned()))
            }
            // A shim and not an `sdiv`, because a zero divisor and MIN/-1 both
            // fault. The operands go in unconverted; the shim is typed by width.
            ArithOp::Div => {
                let symbol = if ty == Type::ZZ32 {
                    "fortress_div_zz32"
                } else {
                    "fortress_div_zz64"
                };
                return self.call_runtime(symbol, &[l, r], true)?.ok_or_else(|| {
                    CodegenError::internal(format!("`{symbol}` returned no value"))
                });
            }
        };
        Ok(out.map_err(CodegenError::from_builder)?.into())
    }

    fn compare(
        &mut self,
        op: CompareOp,
        ty: Type,
        l: BasicValueEnum<'ctx>,
        r: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        if ty == Type::RR64 {
            use inkwell::FloatPredicate as P;
            let predicate = match op {
                CompareOp::Lt => P::OLT,
                CompareOp::Gt => P::OGT,
                CompareOp::Le => P::OLE,
                CompareOp::Ge => P::OGE,
                CompareOp::Eq => P::OEQ,
                CompareOp::Ne => P::ONE,
            };
            let out = self
                .builder
                .build_float_compare(predicate, l.into_float_value(), r.into_float_value(), "cmp")
                .map_err(CodegenError::from_builder)?;
            return Ok(out.into());
        }
        let predicate = match op {
            CompareOp::Lt => IntPredicate::SLT,
            CompareOp::Gt => IntPredicate::SGT,
            CompareOp::Le => IntPredicate::SLE,
            CompareOp::Ge => IntPredicate::SGE,
            CompareOp::Eq => IntPredicate::EQ,
            CompareOp::Ne => IntPredicate::NE,
        };
        let out = self
            .builder
            .build_int_compare(predicate, l.into_int_value(), r.into_int_value(), "cmp")
            .map_err(CodegenError::from_builder)?;
        Ok(out.into())
    }

    fn if_expr(
        &mut self,
        cond: &TypedExpr,
        then_branch: &TypedExpr,
        else_branch: Option<&TypedExpr>,
        result: Type,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        let condition = self.operand(cond)?.into_int_value();
        let function = self
            .builder
            .get_insert_block()
            .and_then(inkwell::basic_block::BasicBlock::get_parent)
            .ok_or_else(|| CodegenError::internal("no enclosing function".to_owned()))?;

        let then_bb = self.context.append_basic_block(function, "then");
        let else_bb = self.context.append_basic_block(function, "else");
        let merge_bb = self.context.append_basic_block(function, "merge");

        self.builder
            .build_conditional_branch(condition, then_bb, else_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(then_bb);
        let then_value = self.expr(then_branch)?;
        let then_exit = self.builder.get_insert_block();
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(else_bb);
        let else_value = match else_branch {
            Some(e) => self.expr(e)?,
            None => None,
        };
        let else_exit = self.builder.get_insert_block();
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(CodegenError::from_builder)?;

        self.builder.position_at_end(merge_bb);
        let Some(ty) = self.basic_type(result) else {
            return Ok(None);
        };
        let (Some(then_value), Some(else_value), Some(then_exit), Some(else_exit)) =
            (then_value, else_value, then_exit, else_exit)
        else {
            return Ok(None);
        };

        let phi = self
            .builder
            .build_phi(ty, "if")
            .map_err(CodegenError::from_builder)?;
        phi.add_incoming(&[(&then_value, then_exit), (&else_value, else_exit)]);
        Ok(Some(phi.as_basic_value()))
    }

    /// `typecase`: the same 32-bit tag load at offset 0 that `dispatch_node`
    /// does, and one switch entry per concrete tag. The checker already removed
    /// every tag an earlier arm claimed, so the arms cannot overlap here and
    /// there is nothing to order at run time.
    fn typecase(
        &mut self,
        subject: &TypedExpr,
        arms: &[TypedTypeCaseArm],
        else_branch: &TypedExpr,
        result: Type,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        let value = self.operand(subject)?;
        let function = self.current_function()?;
        let i32t = self.context.i32_type();
        let tag = self
            .builder
            .build_load(i32t, value.into_pointer_value(), "tag")
            .map_err(CodegenError::from_builder)?
            .into_int_value();

        let else_bb = self.context.append_basic_block(function, "typecase.else");
        let merge_bb = self.context.append_basic_block(function, "typecase.merge");
        let mut cases = Vec::new();
        let mut blocks = Vec::with_capacity(arms.len());
        for arm in arms {
            let bb = self.context.append_basic_block(function, "typecase.arm");
            for tag_value in &arm.tags {
                cases.push((i32t.const_int(u64::from(*tag_value), false), bb));
            }
            blocks.push(bb);
        }
        self.builder
            .build_switch(tag, else_bb, &cases)
            .map_err(CodegenError::from_builder)?;

        let mut incoming: Vec<(BasicValueEnum<'ctx>, BasicBlock<'ctx>)> = Vec::new();
        for (arm, bb) in arms.iter().zip(blocks) {
            self.builder.position_at_end(bb);
            // The narrowed binding is the same pointer: a trait typed value IS
            // a pointer to the concrete object, so narrowing costs no
            // instruction at all.
            self.scopes.push(HashMap::new());
            if let Some(binder) = &arm.binder {
                self.bind(binder, value);
            }
            let lowered = self.expr(&arm.body);
            self.scopes.pop();
            if let Some(produced) = lowered? {
                if let Some(exit) = self.builder.get_insert_block() {
                    incoming.push((produced, exit));
                }
            }
            self.branch_to(merge_bb)?;
        }

        self.builder.position_at_end(else_bb);
        let otherwise = self.expr(else_branch)?;
        if let Some(produced) = otherwise {
            if let Some(exit) = self.builder.get_insert_block() {
                incoming.push((produced, exit));
            }
        }
        self.branch_to(merge_bb)?;

        self.builder.position_at_end(merge_bb);
        self.merge_phi(result, &incoming, "typecase")
    }

    /// `label L ... end L`: one merge block, and every `exit L` is an incoming
    /// edge of its phi. A forward jump inside one function, so there is no
    /// unwinding, no personality function and no landing pad.
    fn label(
        &mut self,
        name: &str,
        body: &TypedExpr,
        result: Type,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        let function = self.current_function()?;
        let end_bb = self.context.append_basic_block(function, "label.end");
        self.labels.push(LabelFrame {
            name: name.to_owned(),
            end: end_bb,
            ty: result,
            incoming: Vec::new(),
        });

        let lowered = self.expr(body);
        let frame = self.labels.pop();
        let lowered = lowered?;

        let mut incoming = frame.map(|f| f.incoming).unwrap_or_default();
        // The fallthrough edge. It may be leaving a block nothing can reach --
        // the body's tail was an `exit` -- and that costs one dead incoming
        // rather than a reachability question.
        if let Some(produced) = lowered {
            if let Some(exit) = self.builder.get_insert_block() {
                incoming.push((produced, exit));
            }
        }
        self.branch_to(end_bb)?;
        self.builder.position_at_end(end_bb);
        self.merge_phi(result, &incoming, "label")
    }

    /// `exit L with e`: record the value as an incoming edge, branch, and
    /// continue lowering into a block nothing branches to. Whatever follows an
    /// `exit` in the source is dead, and putting it in an unreachable block is
    /// what keeps every enclosing construct -- `if`'s phi, a block's tail --
    /// working with no notion of "already terminated".
    fn exit(
        &mut self,
        name: &str,
        value: Option<&TypedExpr>,
        result: Type,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        let lowered = match value {
            Some(e) => self.expr(e)?,
            None => None,
        };
        let Some(index) = self.labels.iter().rposition(|f| f.name == name) else {
            return Err(CodegenError::internal(format!("no open label `{name}`")));
        };
        let (end, frame_ty) = {
            let frame = self
                .labels
                .get(index)
                .ok_or_else(|| CodegenError::internal(format!("no open label `{name}`")))?;
            (frame.end, frame.ty)
        };
        if let (Some(produced), Some(block)) = (lowered, self.builder.get_insert_block()) {
            if let Some(frame) = self.labels.get_mut(index) {
                frame.incoming.push((produced, block));
            }
        }
        self.builder
            .build_unconditional_branch(end)
            .map_err(CodegenError::from_builder)?;

        let function = self.current_function()?;
        let dead = self.context.append_basic_block(function, "exit.after");
        self.builder.position_at_end(dead);
        let _ = frame_ty;
        // A value for the enclosing expression to carry, out of a block that
        // cannot run. `if c then exit L with 1 else 2 end` needs its phi to
        // have an incoming from this side, and this is it.
        Ok(self.basic_type(result).map(|ty| ty.const_zero()))
    }

    fn current_function(&self) -> Result<FunctionValue<'ctx>, CodegenError> {
        self.builder
            .get_insert_block()
            .and_then(inkwell::basic_block::BasicBlock::get_parent)
            .ok_or_else(|| CodegenError::internal("no enclosing function".to_owned()))
    }

    fn branch_to(&self, target: BasicBlock<'ctx>) -> Result<(), CodegenError> {
        self.builder
            .build_unconditional_branch(target)
            .map_err(CodegenError::from_builder)?;
        Ok(())
    }

    /// The phi at a merge with any number of incoming edges. None of them means
    /// nothing reaches the merge, which cannot happen while every arm branches
    /// here unconditionally.
    fn merge_phi(
        &mut self,
        result: Type,
        incoming: &[(BasicValueEnum<'ctx>, BasicBlock<'ctx>)],
        name: &str,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        let Some(ty) = self.basic_type(result) else {
            return Ok(None);
        };
        if incoming.is_empty() {
            return Ok(None);
        }
        let phi = self
            .builder
            .build_phi(ty, name)
            .map_err(CodegenError::from_builder)?;
        let edges: Vec<(&dyn inkwell::values::BasicValue<'ctx>, BasicBlock<'ctx>)> = incoming
            .iter()
            .map(|(value, block)| (value as &dyn inkwell::values::BasicValue<'ctx>, *block))
            .collect();
        phi.add_incoming(&edges);
        Ok(Some(phi.as_basic_value()))
    }

    fn block(
        &mut self,
        items: &[TypedBlockItem],
        tail: Option<&TypedExpr>,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        self.scopes.push(HashMap::new());
        let result = self.block_inner(items, tail);
        self.scopes.pop();
        result
    }

    fn block_inner(
        &mut self,
        items: &[TypedBlockItem],
        tail: Option<&TypedExpr>,
    ) -> Result<Option<BasicValueEnum<'ctx>>, CodegenError> {
        for item in items {
            match item {
                TypedBlockItem::Binding {
                    name,
                    ty,
                    value,
                    mutable,
                    ..
                } => {
                    let lowered = self.operand(value)?;
                    let slot = if *mutable {
                        let cell_ty = self.basic_type(*ty).ok_or_else(|| {
                            CodegenError::internal(format!("`{name}` has no storage type"))
                        })?;
                        let pointer = self.entry_alloca(name, cell_ty)?;
                        self.builder
                            .build_store(pointer, lowered)
                            .map_err(CodegenError::from_builder)?;
                        Slot::Cell {
                            pointer,
                            ty: cell_ty,
                        }
                    } else {
                        lowered.set_name(name);
                        Slot::Value(lowered)
                    };
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.insert(name.clone(), slot);
                    }
                }
                TypedBlockItem::Assign {
                    target, op, value, ..
                } => {
                    self.assign(target, *op, value)?;
                }
                TypedBlockItem::Expr(e) => {
                    self.expr(e)?;
                }
            }
        }
        match tail {
            Some(e) => self.expr(e),
            None => Ok(None),
        }
    }

    /// `x := e`, and `x op= e`, which is where the compound form is finally
    /// split. It could not be split earlier: `l := l + e` makes the target a
    /// READ, and reduction.tex:35 disqualifies a reduction variable that is
    /// read. Splitting it here is also what makes a reduction need no special
    /// case at all -- the body's `l` is bound to a private accumulator cell,
    /// and this is an ordinary load, add and store through it.
    fn assign(
        &mut self,
        target: &AssignTarget,
        op: Option<ArithOp>,
        value: &TypedExpr,
    ) -> Result<(), CodegenError> {
        let lowered = self.operand(value)?;
        match target {
            AssignTarget::Var { name, ty } => {
                let Some(Slot::Cell {
                    pointer,
                    ty: cell_ty,
                }) = self.lookup(name)
                else {
                    return Err(CodegenError::internal(format!(
                        "`{name}` was assigned to but has no storage"
                    )));
                };
                let stored = match op {
                    None => lowered,
                    Some(op) => {
                        let current = self
                            .builder
                            .build_load(cell_ty, pointer, name)
                            .map_err(CodegenError::from_builder)?;
                        self.arith(op, *ty, current, lowered)?
                    }
                };
                self.builder
                    .build_store(pointer, stored)
                    .map_err(CodegenError::from_builder)?;
                Ok(())
            }
            AssignTarget::Element {
                base,
                indices,
                elem,
            } => {
                // One `slot` call, so the base and every index are evaluated
                // once for the read and the write alike.
                let slot = self.slot(base, indices)?;
                let stored = match op {
                    None => lowered,
                    Some(op) => {
                        let current = self.load_element(*elem, slot)?;
                        self.arith(op, elem.as_type(), current, lowered)?
                    }
                };
                self.store_element(*elem, slot, stored)
            }
            // A direct store into the receiver's own block. The specification
            // calls the setter here; there is no setter machinery to call, and
            // the deviation is recorded on `AssignTarget::Field`. Boehm needs
            // no write barrier -- the block is scanned, so a pointer stored
            // into it is seen by the next collection.
            AssignTarget::Field { base, index, ty } => {
                let slot = self.field_pointer(base, *index)?;
                let stored = match op {
                    None => lowered,
                    Some(op) => {
                        let cell = self.basic_type(*ty).ok_or_else(|| {
                            CodegenError::internal("a field with no storage type".to_owned())
                        })?;
                        let current = self
                            .builder
                            .build_load(cell, slot, "field")
                            .map_err(CodegenError::from_builder)?;
                        self.arith(op, *ty, current, lowered)?
                    }
                };
                self.builder
                    .build_store(slot, stored)
                    .map_err(CodegenError::from_builder)?;
                Ok(())
            }
        }
    }

    fn one(&mut self, args: &[TypedExpr]) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let [a] = args else {
            return Err(CodegenError::internal(format!(
                "expected 1 argument, got {}",
                args.len()
            )));
        };
        self.operand(a)
    }

    fn two(&mut self, args: &[TypedExpr]) -> Result<[BasicValueEnum<'ctx>; 2], CodegenError> {
        let [a, b] = args else {
            return Err(CodegenError::internal(format!(
                "expected 2 arguments, got {}",
                args.len()
            )));
        };
        Ok([self.operand(a)?, self.operand(b)?])
    }
}

fn target_machine(cpu: &str) -> Result<TargetMachine, CodegenError> {
    LlvmTarget::initialize_native(&InitializationConfig::default())
        .map_err(|detail| CodegenError::TargetUnavailable { detail })?;

    let triple = TargetMachine::get_default_triple();
    let target = LlvmTarget::from_triple(&triple).map_err(|e| CodegenError::TargetUnavailable {
        detail: e.to_string(),
    })?;

    // `native` is the only setting that reads the machine underneath, and it
    // is opt in for exactly that reason.
    let (name, features) = if cpu == "native" {
        (
            TargetMachine::get_host_cpu_name()
                .to_string_lossy()
                .into_owned(),
            TargetMachine::get_host_cpu_features()
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        (cpu.to_owned(), String::new())
    };

    target
        .create_target_machine(
            &triple,
            &name,
            &features,
            OptimizationLevel::None,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| CodegenError::TargetUnavailable {
            detail: format!(
                "no target machine for {} on {}",
                name,
                triple.as_str().to_string_lossy()
            ),
        })
}

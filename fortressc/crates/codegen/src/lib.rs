//! Typed AST to LLVM IR.
//!
//! Every operator and call arrives already resolved to one concrete
//! [`Target`], so nothing here dispatches: lowering is a translation, not a
//! decision. Failures are compiler bugs, not user errors.

use std::collections::HashMap;
use std::path::Path;

use fortress_types::{
    ArithOp, AssignTarget, CompareOp, Elem, Target, Type, TypedBlockItem, TypedComponent,
    TypedExpr, TypedExprKind, TypedFn, ARRAY_ALLOC, ARRAY_LENGTH, ARRAY_SLOT,
};
use inkwell::attributes::AttributeLoc;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target as LlvmTarget, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};

mod error;
pub use error::CodegenError;

/// The Fortress entry point. `main` calls it and returns 0.
const ENTRY: &str = "run";

/// Starts the collector. Emitted as the first instruction of `main`.
const RUNTIME_INIT: &str = "fortress_runtime_init";

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
        scopes: Vec::new(),
    };
    lowering.declare_runtime();
    if component.uses_mpi {
        lowering.declare_mpi();
    }
    lowering.declare_functions(component)?;
    for f in &component.functions {
        lowering.define_function(f)?;
    }
    lowering.emit_main()?;

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
    scopes: Vec<HashMap<String, Slot<'ctx>>>,
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

impl<'ctx> Lowering<'ctx> {
    fn ptr(&self) -> BasicTypeEnum<'ctx> {
        self.context.ptr_type(AddressSpace::default()).into()
    }

    fn basic_type(&self, ty: Type) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            Type::ZZ32 => Some(self.context.i32_type().into()),
            Type::ZZ64 => Some(self.context.i64_type().into()),
            Type::RR64 => Some(self.context.f64_type().into()),
            Type::Boolean => Some(self.context.bool_type().into()),
            Type::String | Type::Array(_) => Some(self.ptr()),
            Type::Void => None,
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

        let printlns: [(&str, Option<BasicMetadataTypeEnum<'ctx>>); 6] = [
            ("println_string", Some(ptr.into())),
            ("println_zz32", Some(i32t.into())),
            ("println_zz64", Some(i64t.into())),
            ("println_rr64", Some(f64t.into())),
            ("println_boolean", Some(i32t.into())),
            ("println_void", None),
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

        self.module.add_function(
            RUNTIME_INIT,
            void.fn_type(&[], false),
            Some(Linkage::External),
        );
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

    /// `main` exists so the ELF has a C entry point. It starts the runtime,
    /// calls `run` and returns 0; a Fortress program's exit status is not its
    /// return value.
    fn emit_main(&mut self) -> Result<(), CodegenError> {
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
            TypedExprKind::Index { base, index, elem } => {
                let slot = self.slot(base, index)?;
                self.load_element(*elem, slot).map(Some)
            }
            TypedExprKind::While { cond, body } => self.while_loop(cond, body).map(|()| None),
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
        index: &TypedExpr,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        let array = self.operand(base)?;
        let at = self.operand(index)?;
        self.slot_of(array, at)
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
            Target::Widen { .. } => {
                let value = self.one(args)?;
                let out = self
                    .builder
                    .build_int_s_extend(value.into_int_value(), self.context.i64_type(), "widen")
                    .map_err(CodegenError::from_builder)?;
                Ok(Some(out.into()))
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
            Target::Mpi(op) => self.call_runtime(&target.symbol(), &[], op.returns() != Type::Void),
            Target::ArrayNew { elem } => {
                let count = self.one(args)?;
                self.array_alloc(*elem, count).map(Some)
            }
            Target::ArrayLength => {
                let array = self.one(args)?;
                self.call_runtime(ARRAY_LENGTH, &[array], true)
            }
            Target::UserFn { name } => {
                let function = *self
                    .functions
                    .get(name)
                    .ok_or_else(|| CodegenError::internal(format!("unknown function `{name}`")))?;
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
        }
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
            let out = match op {
                ArithOp::Add => self.builder.build_float_add(l, r, "add"),
                ArithOp::Sub => self.builder.build_float_sub(l, r, "sub"),
                ArithOp::Mul => self.builder.build_float_mul(l, r, "mul"),
                ArithOp::Div => self.builder.build_float_div(l, r, "div"),
            };
            return Ok(out.map_err(CodegenError::from_builder)?.into());
        }
        let (l, r) = (l.into_int_value(), r.into_int_value());
        let out = match op {
            ArithOp::Add => self.builder.build_int_add(l, r, "add"),
            ArithOp::Sub => self.builder.build_int_sub(l, r, "sub"),
            ArithOp::Mul => self.builder.build_int_mul(l, r, "mul"),
            ArithOp::Div => self.builder.build_int_signed_div(l, r, "div"),
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
                TypedBlockItem::Assign { target, value, .. } => {
                    self.assign(target, value)?;
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

    fn assign(&mut self, target: &AssignTarget, value: &TypedExpr) -> Result<(), CodegenError> {
        let lowered = self.operand(value)?;
        match target {
            AssignTarget::Var { name, .. } => {
                let Some(Slot::Cell { pointer, .. }) = self.lookup(name) else {
                    return Err(CodegenError::internal(format!(
                        "`{name}` was assigned to but has no storage"
                    )));
                };
                self.builder
                    .build_store(pointer, lowered)
                    .map_err(CodegenError::from_builder)?;
                Ok(())
            }
            AssignTarget::Element { base, index, elem } => {
                let slot = self.slot(base, index)?;
                self.store_element(*elem, slot, lowered)
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

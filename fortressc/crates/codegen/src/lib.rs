//! Typed AST to LLVM IR.
//!
//! Every operator and call arrives already resolved to one concrete
//! [`Target`], so nothing here dispatches: lowering is a translation, not a
//! decision. Failures are compiler bugs, not user errors.

use std::collections::HashMap;
use std::path::Path;

use fortress_types::{
    ArithOp, CompareOp, Target, Type, TypedBlockItem, TypedComponent, TypedExpr, TypedExprKind,
    TypedFn,
};
use inkwell::attributes::AttributeLoc;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target as LlvmTarget, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue};
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
    /// M1 has no mutation, so bindings are plain SSA values; no `alloca`.
    scopes: Vec<HashMap<String, BasicValueEnum<'ctx>>>,
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
            Type::String => Some(self.ptr()),
            Type::Void => None,
        }
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
            scope.insert(param.name.clone(), value);
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

    fn lookup(&self, name: &str) -> Option<BasicValueEnum<'ctx>> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
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
            TypedExprKind::Var(name) => self
                .lookup(name)
                .map(Some)
                .ok_or_else(|| CodegenError::internal(format!("unbound name `{name}`"))),
            TypedExprKind::Apply { target, args } => self.apply(target, args, e.ty),
            TypedExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.if_expr(cond, then_branch, else_branch.as_deref(), e.ty),
            TypedExprKind::Block { items, tail } => self.block(items, tail.as_deref()),
        }
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
                TypedBlockItem::Binding { name, value, .. } => {
                    let lowered = self.operand(value)?;
                    lowered.set_name(name);
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.insert(name.clone(), lowered);
                    }
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

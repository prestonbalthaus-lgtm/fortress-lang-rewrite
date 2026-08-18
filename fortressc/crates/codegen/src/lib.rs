//! Typed AST to LLVM IR.
//!
//! Every operator and call arrives already resolved to one concrete target, so
//! nothing here dispatches. Failures are compiler bugs, not user errors.

use std::path::Path;

use fortress_ast::Component;
use inkwell::context::Context;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::OptimizationLevel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    ModuleVerificationFailed { detail: String },
    TargetUnavailable { detail: String },
    ObjectWriteFailed { detail: String },
    LinkerFailed { status: Option<i32>, stderr: String },
}

impl core::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ModuleVerificationFailed { detail } => {
                write!(f, "LLVM rejected the generated module: {detail}")
            }
            Self::TargetUnavailable { detail } => write!(f, "no usable LLVM target: {detail}"),
            Self::ObjectWriteFailed { detail } => {
                write!(f, "could not write object file: {detail}")
            }
            Self::LinkerFailed { status, stderr } => {
                write!(f, "linker failed (status {status:?}): {stderr}")
            }
        }
    }
}

impl std::error::Error for CodegenError {}

/// The constant every program currently compiles to. Lowering the AST is the
/// next milestone; until then this is the placeholder that keeps the pipeline
/// end to end.
const PLACEHOLDER_EXIT_CODE: u64 = 42;

/// Lowers `component` to a native object file at `object_path`.
///
/// The component is accepted and discarded: the parser produces a real AST and
/// hands it across this boundary, but nothing here reads it yet.
///
/// `Module::verify` runs unconditionally: catching malformed IR here rather
/// than as a cryptic link failure several steps later is worth the cost while
/// the compiler is young.
pub fn emit_object(component: &Component, object_path: &Path) -> Result<(), CodegenError> {
    let _ = component;
    let context = Context::create();
    let module = context.create_module("fortress");
    let builder = context.create_builder();

    let i32_type = context.i32_type();
    let main_type = i32_type.fn_type(&[], false);
    let main = module.add_function("main", main_type, None);
    let entry = context.append_basic_block(main, "entry");
    builder.position_at_end(entry);

    let exit_code = i32_type.const_int(PLACEHOLDER_EXIT_CODE, false);
    builder
        .build_return(Some(&exit_code))
        .map_err(|e| CodegenError::ModuleVerificationFailed {
            detail: e.to_string(),
        })?;

    module
        .verify()
        .map_err(|e| CodegenError::ModuleVerificationFailed {
            detail: e.to_string(),
        })?;

    write_object(&module, object_path)
}

fn write_object(module: &inkwell::module::Module<'_>, path: &Path) -> Result<(), CodegenError> {
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|detail| CodegenError::TargetUnavailable { detail })?;

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| CodegenError::TargetUnavailable {
        detail: e.to_string(),
    })?;

    let machine = target
        .create_target_machine(
            &triple,
            &TargetMachine::get_host_cpu_name().to_string_lossy(),
            &TargetMachine::get_host_cpu_features().to_string_lossy(),
            OptimizationLevel::None,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| CodegenError::TargetUnavailable {
            detail: format!(
                "no target machine for {}",
                triple.as_str().to_string_lossy()
            ),
        })?;

    machine
        .write_to_file(module, FileType::Object, path)
        .map_err(|e| CodegenError::ObjectWriteFailed {
            detail: e.to_string(),
        })
}

/// Emits the LLVM IR as text. Used by tests and `--emit-ir`.
pub fn emit_ir(component: &Component) -> Result<String, CodegenError> {
    let _ = component;
    let context = Context::create();
    let module = context.create_module("fortress");
    let builder = context.create_builder();

    let i32_type = context.i32_type();
    let main = module.add_function("main", i32_type.fn_type(&[], false), None);
    let entry = context.append_basic_block(main, "entry");
    builder.position_at_end(entry);
    builder
        .build_return(Some(&i32_type.const_int(PLACEHOLDER_EXIT_CODE, false)))
        .map_err(|e| CodegenError::ModuleVerificationFailed {
            detail: e.to_string(),
        })?;

    module
        .verify()
        .map_err(|e| CodegenError::ModuleVerificationFailed {
            detail: e.to_string(),
        })?;

    Ok(module.print_to_string().to_string())
}

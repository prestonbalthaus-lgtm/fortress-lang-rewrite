//! Typed AST to LLVM IR.
//!
//! Every operator and call arrives already resolved to one concrete target, so
//! nothing here dispatches. Failures are compiler bugs, not user errors.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    ModuleVerificationFailed { detail: String },
    LinkerFailed { status: Option<i32>, stderr: String },
}

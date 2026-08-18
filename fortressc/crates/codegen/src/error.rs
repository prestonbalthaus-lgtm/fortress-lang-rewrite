#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    ModuleVerificationFailed {
        detail: String,
    },
    TargetUnavailable {
        detail: String,
    },
    ObjectWriteFailed {
        detail: String,
    },
    /// A broken invariant in the compiler, not in the user's program.
    Internal {
        detail: String,
    },
}

impl CodegenError {
    pub(crate) fn internal(detail: String) -> Self {
        Self::Internal { detail }
    }

    pub(crate) fn from_builder(e: inkwell::builder::BuilderError) -> Self {
        Self::Internal {
            detail: e.to_string(),
        }
    }
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
            Self::Internal { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for CodegenError {}

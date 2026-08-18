//! `fortressc`: source in, ELF out.
//!
//! The pipeline is a tracer bullet. The lexer is real and its failures are real
//! diagnostics; everything after it is a placeholder that emits a constant, so
//! that the path from a `.fss` file to a running binary exists and is tested
//! before the parser, types and codegen are thickened into it.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// A diagnostic against the user's source.
const EXIT_USER_ERROR: u8 = 1;
/// A compiler bug: malformed IR, a failed linker, an internal invariant broken.
const EXIT_INTERNAL_ERROR: u8 = 70;

struct Options {
    source: PathBuf,
    output: PathBuf,
    emit_ir: bool,
}

fn parse_args(args: &[String]) -> Option<Options> {
    let mut source: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut emit_ir = false;
    let mut rest = args.iter();

    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-o" => output = Some(PathBuf::from(rest.next()?)),
            "--emit-ir" => emit_ir = true,
            flag if flag.starts_with('-') => return None,
            path => source = Some(PathBuf::from(path)),
        }
    }

    let source = source?;
    let output = output.unwrap_or_else(|| source.with_extension(""));
    Some(Options {
        source,
        output,
        emit_ir,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(options) = parse_args(&args) else {
        eprintln!("usage: fortressc <source.fss> [-o <output>] [--emit-ir]");
        return ExitCode::from(EXIT_USER_ERROR);
    };

    match compile(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::User(message)) => {
            eprintln!("{}: {message}", options.source.display());
            ExitCode::from(EXIT_USER_ERROR)
        }
        Err(Failure::Internal(message)) => {
            eprintln!("fortressc: internal error: {message}");
            ExitCode::from(EXIT_INTERNAL_ERROR)
        }
    }
}

enum Failure {
    User(String),
    Internal(String),
}

fn compile(options: &Options) -> Result<(), Failure> {
    let source = std::fs::read_to_string(&options.source)
        .map_err(|e| Failure::User(format!("cannot read source: {e}")))?;

    // Lexing, parsing and type checking are real stages and their diagnostics
    // are user errors. Codegen is still the placeholder: it accepts the typed
    // AST and discards it.
    let tokens = fortress_lexer::lex(&source).map_err(|e| Failure::User(e.to_string()))?;
    let component = fortress_parser::parse(&tokens).map_err(|e| Failure::User(e.to_string()))?;
    let typed = fortress_types::check(&component).map_err(|e| Failure::User(e.to_string()))?;

    eprintln!(
        "fortressc: lexed {} tokens, parsed and typechecked `{}` with {} function(s)",
        tokens.len(),
        typed.name,
        typed.functions.len()
    );

    if options.emit_ir {
        let ir = fortress_codegen::emit_ir(&typed).map_err(|e| Failure::Internal(e.to_string()))?;
        print!("{ir}");
        return Ok(());
    }

    let object = options.output.with_extension("o");
    fortress_codegen::emit_object(&typed, &object).map_err(|e| Failure::Internal(e.to_string()))?;
    link(&object, &options.output)?;
    let _ = std::fs::remove_file(&object);
    Ok(())
}

/// `lld` is not installed on every box, and `cc` is. The linker driver only has
/// to find crt startup files and libc.
fn link(object: &Path, output: &Path) -> Result<(), Failure> {
    let result = Command::new("cc")
        .arg(object)
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|e| Failure::Internal(format!("could not run cc: {e}")))?;

    if result.status.success() {
        return Ok(());
    }
    Err(Failure::Internal(format!(
        "linker failed (status {:?}): {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr)
    )))
}

//! `fortressc`: source in, ELF out.
//!
//! Lexing, parsing, type checking and codegen are all real, and their failures
//! are real diagnostics. Linking is delegated to a C compiler driver, which is
//! what `--cc` overrides: on a cluster that driver is `mpicc`, and pointing at
//! it is how the compiler stays ignorant of where the local MPI lives.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// A diagnostic against the user's source.
const EXIT_USER_ERROR: u8 = 1;
/// A compiler bug: malformed IR, a failed linker, an internal invariant broken.
const EXIT_INTERNAL_ERROR: u8 = 70;

/// The default link driver. `lld` is not installed everywhere and `cc` is; it
/// only has to find the crt startup files and libc.
const DEFAULT_CC: &str = "cc";

struct Options {
    source: PathBuf,
    output: PathBuf,
    emit_ir: bool,
    emit_obj: bool,
    cc: String,
    cpu: String,
}

fn parse_args(args: &[String]) -> Option<Options> {
    let mut source: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut emit_ir = false;
    let mut emit_obj = false;
    let mut cc = DEFAULT_CC.to_owned();
    let mut cpu = fortress_codegen::DEFAULT_CPU.to_owned();
    let mut rest = args.iter();

    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-o" => output = Some(PathBuf::from(rest.next()?)),
            "--emit-ir" => emit_ir = true,
            "--emit-obj" => emit_obj = true,
            "--cc" => cc = rest.next()?.clone(),
            "--target-cpu" => cpu = rest.next()?.clone(),
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
        emit_obj,
        cc,
        cpu,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(options) = parse_args(&args) else {
        eprintln!(
            "usage: fortressc <source.fss> [-o <output>] [--emit-ir] [--emit-obj] \
                  [--cc <driver>] [--target-cpu <{}>]",
            fortress_codegen::SUPPORTED_CPUS.join("|")
        );
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
    if !fortress_codegen::SUPPORTED_CPUS.contains(&options.cpu.as_str()) {
        return Err(Failure::User(format!(
            "unknown target CPU `{}`; accepted: {}",
            options.cpu,
            fortress_codegen::SUPPORTED_CPUS.join(", ")
        )));
    }

    let source = std::fs::read_to_string(&options.source)
        .map_err(|e| Failure::User(format!("cannot read source: {e}")))?;

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
        let ir = fortress_codegen::emit_ir(&typed, &options.cpu)
            .map_err(|e| Failure::Internal(e.to_string()))?;
        print!("{ir}");
        return Ok(());
    }

    // The object lands at exactly `-o` under `--emit-obj`: the cluster build
    // splits compiling from linking, and the link half runs elsewhere, under a
    // different C library, against the local MPI.
    if options.emit_obj {
        return fortress_codegen::emit_object(&typed, &options.output, &options.cpu)
            .map_err(|e| Failure::Internal(e.to_string()));
    }

    let object = options.output.with_extension("o");
    fortress_codegen::emit_object(&typed, &object, &options.cpu)
        .map_err(|e| Failure::Internal(e.to_string()))?;
    link(options, &object, typed.uses_mpi)?;
    let _ = std::fs::remove_file(&object);
    Ok(())
}

/// The runtime, embedded so the compiler does not have to locate a file at run
/// time.
const RUNTIME_SHIMS: &str = include_str!("../../../runtime/shims.c");
/// The MPI half, kept separate because it includes `<mpi.h>` and so can only be
/// compiled where an MPI exists. It goes into the link only when the program
/// calls an MPI builtin.
const MPI_SHIMS: &str = include_str!("../../../runtime/mpi_shims.c");

/// A source file written next to the output for the duration of the link, and
/// removed whether the link succeeds or not.
struct ScratchSource {
    path: PathBuf,
}

impl ScratchSource {
    fn write(output: &Path, extension: &str, contents: &str) -> Result<Self, Failure> {
        let path = output.with_extension(extension);
        std::fs::write(&path, contents)
            .map_err(|e| Failure::Internal(format!("could not write {}: {e}", path.display())))?;
        Ok(Self { path })
    }
}

impl Drop for ScratchSource {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn link(options: &Options, object: &Path, uses_mpi: bool) -> Result<(), Failure> {
    let mut sources = vec![ScratchSource::write(
        &options.output,
        "shims.c",
        RUNTIME_SHIMS,
    )?];
    if uses_mpi {
        sources.push(ScratchSource::write(
            &options.output,
            "mpi_shims.c",
            MPI_SHIMS,
        )?);
    }

    let mut command = Command::new(&options.cc);
    command.arg(object);
    for source in &sources {
        command.arg(&source.path);
    }
    // The libraries last, because a library has to follow the objects that
    // need it. `-lm` is for the one shim that calls `pow`; glibc folds libm
    // into libc, but the flag is what makes the link work anywhere else.
    let result = command
        .arg("-o")
        .arg(&options.output)
        .arg("-lgc")
        .arg("-lm")
        .output()
        .map_err(|e| Failure::Internal(format!("could not run `{}`: {e}", options.cc)))?;

    if result.status.success() {
        return Ok(());
    }
    Err(Failure::Internal(format!(
        "linker `{}` failed (status {:?}): {}",
        options.cc,
        result.status.code(),
        String::from_utf8_lossy(&result.stderr)
    )))
}

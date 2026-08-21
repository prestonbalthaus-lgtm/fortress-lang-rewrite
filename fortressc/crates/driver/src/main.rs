//! `fortressc`: source in, ELF out.
//!
//! Lexing, parsing, type checking and codegen are all real, and their failures
//! are real diagnostics. Linking is delegated to a C compiler driver, which is
//! what `--cc` overrides: on a cluster that driver is `mpicc`, and pointing at
//! it is how the compiler stays ignorant of where the local MPI lives.

use fortress_ast::Span;
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
    resolve_imports: bool,
    check_exports: bool,
    emit_obj: bool,
    cc: String,
    cpu: String,
}

fn parse_args(args: &[String]) -> Option<Options> {
    let mut source: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut emit_ir = false;
    // ON BY DEFAULT since phase 2. It costs seven api files that were reaching
    // the terminus only because their imports were inert, and that is the
    // decision: accuracy over inflation. `--no-resolve-imports` is what the
    // census and the gates use to take the comparison.
    let mut resolve_imports = true;
    // OFF by default, and the reason is measured rather than cautious: see the
    // note in `conform`. Turning it on is a decision with a blast radius.
    let mut check_exports = false;
    let mut emit_obj = false;
    let mut cc = DEFAULT_CC.to_owned();
    let mut cpu = fortress_codegen::DEFAULT_CPU.to_owned();
    let mut rest = args.iter();

    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-o" => output = Some(PathBuf::from(rest.next()?)),
            "--emit-ir" => emit_ir = true,
            "--resolve-imports" => resolve_imports = true,
            "--no-resolve-imports" => resolve_imports = false,
            "--check-exports" => check_exports = true,
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
        resolve_imports,
        check_exports,
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
             [--no-resolve-imports] [--check-exports] \
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
        Err(Failure::Diagnostic(rendered)) => {
            eprintln!("{rendered}");
            ExitCode::from(EXIT_USER_ERROR)
        }
        Err(Failure::Internal(message)) => {
            eprintln!("fortressc: internal error: {message}");
            ExitCode::from(EXIT_INTERNAL_ERROR)
        }
    }
}

mod conform;
mod resolve;

enum Failure {
    User(String),
    /// Already rendered: path, `line:col`, the message, and the source excerpt.
    /// `main` prints it as it stands rather than prefixing the path again.
    Diagnostic(String),
    Internal(String),
}

/// A byte offset's position in the source, one-based, in CHARACTERS.
///
/// The line terminators are the LEXER's four and not `str::lines()`'s one
/// (`lexer/src/raw.rs:32-34`): `\n`, `\r\n` as a single break, a lone `\r`,
/// U+2028 and U+2029. 28 corpus files are CRLF and two carry U+2028/U+2029, so
/// a converter built on `.lines()` disagrees with the lexer about where a line
/// begins. The column counts CHARACTERS because non-ASCII is legal inside
/// comments and strings, and one Greek comment ahead of the code is three
/// columns of drift.
struct Position {
    line: usize,
    column: usize,
}

const fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn tail(s: &str, from: usize) -> &str {
    s.get(from..).unwrap_or("")
}

/// Walks to `offset`, returning its position and the byte offset where its line
/// begins. A linear scan per diagnostic, which is free: the compiler emits at
/// most one.
fn locate(source: &str, offset: usize) -> (Position, usize) {
    let mut line = 1;
    let mut column = 1;
    let mut line_start = 0;
    let mut chars = source.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if index >= offset {
            break;
        }
        if is_line_terminator(c) {
            let mut next = index + c.len_utf8();
            if c == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
                chars.next();
                next += 1;
            }
            line += 1;
            column = 1;
            line_start = next;
        } else {
            column += 1;
        }
    }
    (Position { line, column }, line_start)
}

fn line_text(source: &str, line_start: usize) -> &str {
    let rest = tail(source, line_start);
    match rest.char_indices().find(|(_, c)| is_line_terminator(*c)) {
        Some((end, _)) => rest.get(..end).unwrap_or(rest),
        None => rest,
    }
}

/// `path:line:col: message`, then the source line and a caret under the span.
/// A span that runs past the end of its line -- which every declaration span
/// does, `Span::new(name.start, end.end)` -- gets one caret rather than a run
/// that would need a second line to draw.
fn excerpt(source: &str, span: Span) -> String {
    let (start, line_start) = locate(source, span.start);
    let text = line_text(source, line_start);
    let gutter = start.line.to_string();
    let pad = " ".repeat(gutter.len());
    let width = tail(source, span.start)
        .char_indices()
        .take_while(|(i, c)| *i < span.end.saturating_sub(span.start) && !is_line_terminator(*c))
        .count()
        .max(1);
    format!(
        "\n{pad} |\n{gutter} | {text}\n{pad} | {}{}",
        " ".repeat(start.column.saturating_sub(1)),
        "^".repeat(width)
    )
}

fn render(
    path: &Path,
    source: &str,
    span: Option<Span>,
    message: &str,
    notes: &[(Span, &'static str)],
) -> String {
    let Some(span) = span else {
        return format!("{}: {message}", path.display());
    };
    let (position, _) = locate(source, span.start);
    let mut out = format!(
        "{}:{}:{}: {message}{}",
        path.display(),
        position.line,
        position.column,
        excerpt(source, span)
    );
    for (note_span, label) in notes {
        let (note_position, _) = locate(source, note_span.start);
        out.push_str(&format!(
            "\n{}:{}:{}: note: {label}{}",
            path.display(),
            note_position.line,
            note_position.column,
            excerpt(source, *note_span)
        ));
    }
    out
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

    // The diagnostic renderer is the semantics lane's; the import resolution
    // step is the frontend lane's, and it sits between the parse and the check
    // because `registry.concrete` and every type tag freeze in `Checker::new`.
    let path = options.source.as_path();
    let tokens = fortress_lexer::lex(&source)
        .map_err(|e| Failure::Diagnostic(render(path, &source, e.span(), &e.to_string(), &[])))?;
    let component = fortress_parser::parse(&tokens)
        .map_err(|e| Failure::Diagnostic(render(path, &source, e.span(), &e.to_string(), &[])))?;
    let resolved = if options.resolve_imports {
        Some(resolve::resolve(&component, &options.source))
    } else {
        None
    };
    let component = resolved.as_ref().map_or(&component, |r| &r.component);
    // BEFORE the check, so a conformance failure is reported against what the
    // source says rather than against whatever the checker made of it.
    if options.check_exports {
        let mut violations = Vec::new();
        for exported in &component.exports {
            let Some(api) = resolve::find_api(exported, &options.source) else {
                continue;
            };
            violations.extend(conform::violations(component, &api, exported));
        }
        if let Some(first) = violations.first() {
            return Err(Failure::User(format!(
                "{first}{}",
                if violations.len() > 1 {
                    format!(" (and {} more)", violations.len() - 1)
                } else {
                    String::new()
                }
            )));
        }
    }
    let typed = fortress_types::check(component).map_err(|e| {
        Failure::Diagnostic(render(
            path,
            &source,
            Some(e.span()),
            &e.to_string(),
            &e.notes(),
        ))
    })?;

    if typed.is_api {
        eprintln!(
            "fortressc: lexed {} tokens, checked the api `{}`: {} declaration(s), \
             headers resolved and bounds discharged",
            tokens.len(),
            typed.name,
            component.decls.len()
        );
    } else {
        eprintln!(
            "fortressc: lexed {} tokens, parsed and typechecked `{}` with {} function(s)",
            tokens.len(),
            typed.name,
            typed.functions.len()
        );
    }
    if let Some(r) = resolved.as_ref() {
        eprintln!(
            "fortressc: resolved {} api(s){}{}",
            r.loaded.len(),
            if r.missing.is_empty() {
                String::new()
            } else {
                format!("; not on the source path: {}", r.missing.join(", "))
            },
            if r.unreadable.is_empty() {
                String::new()
            } else {
                format!("; found but unreadable: {}", r.unreadable.join(", "))
            }
        );
    }

    // AN API IS CHECKED AND NOT EMITTED. Signatures have no code, so there is
    // no object, no IR and no link -- and exit 0 is the right answer, because
    // nothing about the file was wrong.
    if typed.is_api {
        return Ok(());
    }

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

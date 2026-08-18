//! The whole pipeline, as gates rather than demos: a `.fss` file becomes an ELF
//! that runs and prints the right answer.

// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules; an
// integration test is its own crate, so the workspace denies apply here.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests")
        .join(name)
}

fn output_path(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("fortressc-e2e-{tag}-{}", std::process::id()));
    p
}

fn compile_fixture(name: &str, tag: &str) -> PathBuf {
    let out = output_path(tag);
    let status = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture(name))
        .arg("-o")
        .arg(&out)
        .status()
        .expect("could not run fortressc");
    assert!(status.success(), "fortressc failed: {status:?}");
    assert!(
        out.exists(),
        "fortressc produced no binary at {}",
        out.display()
    );
    out
}

fn run(binary: &PathBuf) -> Output {
    Command::new(binary)
        .output()
        .expect("could not run the produced binary")
}

/// The milestone.
#[test]
fn the_acceptance_program_prints_the_right_factorial() {
    let binary = compile_fixture("fact.fss", "fact");
    let out = run(&binary);

    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "fact(20) = 2432902008176640000\n",
        "M1's exit criterion: native 64-bit recursion, printed exactly"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

#[test]
fn a_fortress_source_file_becomes_a_running_native_binary() {
    let binary = compile_fixture("skeleton.fss", "run");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "the pipe exists\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

#[test]
fn the_produced_binary_links_nothing_but_libc() {
    let binary = compile_fixture("skeleton.fss", "ldd");
    let out = Command::new("ldd")
        .arg(&binary)
        .output()
        .expect("could not run ldd");
    let deps = String::from_utf8_lossy(&out.stdout).to_lowercase();

    // The entire point of the project: no JVM anywhere in the output, and no
    // LLVM runtime dependency leaking into compiled programs either.
    for forbidden in ["jvm", "libjava", "libllvm", "libstdc++"] {
        assert!(
            !deps.contains(forbidden),
            "produced binary links {forbidden}:\n{deps}"
        );
    }
    assert!(deps.contains("libc.so"), "expected libc:\n{deps}");
    let _ = std::fs::remove_file(&binary);
}

#[test]
fn the_emitted_ir_lowers_the_program_rather_than_a_constant() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("fact.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let ir = String::from_utf8_lossy(&out.stdout);

    for expected in [
        "define i64 @f(i64 %x)", // the user function, typed
        "call i64 @f(",          // recursion
        "icmp slt i64",          // x < 2
        "mul i64",               // the folded juxtaposition
        "@concat_string_string", // the folded string juxtaposition
        "@to_string_zz64",       // the explicit conversion the checker inserted
        "phi i64",               // the if, as a value
    ] {
        assert!(ir.contains(expected), "missing {expected} in:\n{ir}");
    }
    assert!(
        !ir.contains("ret i32 42"),
        "the placeholder constant is still being emitted"
    );
}

#[test]
fn the_driver_reports_that_it_typechecked() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("fact.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("typechecked `fact` with 2 function(s)"),
        "the typed AST must reach the driver:\n{stderr}"
    );
}

#[test]
fn a_lex_error_is_a_user_diagnostic_not_a_compiler_bug() {
    let bad = output_path("bad").with_extension("fss");
    std::fs::write(&bad, "component a\n\tb\nend\n").expect("could not write fixture");

    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&bad)
        .arg("-o")
        .arg(output_path("bad-out"))
        .output()
        .expect("could not run fortressc");

    assert_eq!(
        out.status.code(),
        Some(1),
        "a tab is a user error, exit 1 not 70"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("tab characters"),
        "expected the tab diagnostic:\n{stderr}"
    );
    let _ = std::fs::remove_file(&bad);
}

#[test]
fn a_parse_error_is_a_user_diagnostic_not_a_compiler_bug() {
    let bad = output_path("badparse").with_extension("fss");
    // `x- 1` is a postfix operator followed by a juxtaposition: real Fortress,
    // outside M1, and a user error rather than an internal one.
    std::fs::write(&bad, "component a\nf() = x- 1\nend\n").expect("could not write fixture");

    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&bad)
        .arg("-o")
        .arg(output_path("badparse-out"))
        .output()
        .expect("could not run fortressc");

    assert_eq!(
        out.status.code(),
        Some(1),
        "a parse error is exit 1, not 70"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("postfix"),
        "expected the postfix diagnostic:\n{stderr}"
    );
    let _ = std::fs::remove_file(&bad);
}

#[test]
fn a_type_error_is_a_user_diagnostic_and_names_the_fix() {
    let bad = output_path("badtype").with_extension("fss");
    // A ZZ32 value in a ZZ64 slot: the locked rule, end to end.
    std::fs::write(&bad, "component a\nf(x:ZZ32):ZZ64 = x\nend\n").expect("write fixture");

    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&bad)
        .arg("-o")
        .arg(output_path("badtype-out"))
        .output()
        .expect("could not run fortressc");

    assert_eq!(out.status.code(), Some(1), "a type error is exit 1, not 70");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not implicitly converted") && stderr.contains("widen"),
        "the diagnostic should name the fix:\n{stderr}"
    );
    let _ = std::fs::remove_file(&bad);
}

// ------------------------------------------------------------ M2a: the MPI
// boundary. Nothing here needs an MPI installation: these gate the compiler's
// half of the contract, which is that generated code names only the
// `fortress_mpi_` shims and that the driver links the shim exactly when the
// program uses one. Linking and running against a real OpenMPI is
// `tools/mpi-gate.sh`, which needs the Apptainer image.

fn emit_ir(fixture_name: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture(fixture_name))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    assert!(out.status.success(), "fortressc failed on {fixture_name}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn an_mpi_program_calls_the_prefixed_shims_and_nothing_else() {
    let ir = emit_ir("mpi_hello.fss");
    for expected in [
        "declare void @fortress_mpi_init()",
        "declare i32 @fortress_mpi_comm_rank()",
        "declare i32 @fortress_mpi_comm_size()",
        "declare void @fortress_mpi_finalize()",
        "call void @fortress_mpi_init()",
        "call i32 @fortress_mpi_comm_rank()",
        "call i32 @fortress_mpi_comm_size()",
        "call void @fortress_mpi_finalize()",
    ] {
        assert!(ir.contains(expected), "missing {expected} in:\n{ir}");
    }
}

/// The reason the shim exists. `MPI_COMM_WORLD` is a macro, and its expansion
/// differs between OpenMPI and MPICH, so it must never be baked into IR.
#[test]
fn no_mpi_implementation_detail_reaches_the_ir() {
    let ir = emit_ir("mpi_hello.fss");
    for forbidden in [
        "MPI_COMM_WORLD",
        "ompi_mpi_comm_world",
        "@MPI_Init",
        "@MPI_Comm_rank",
    ] {
        assert!(
            !ir.contains(forbidden),
            "{forbidden} leaked into the IR:\n{ir}"
        );
    }
}

#[test]
fn a_program_that_does_not_use_mpi_declares_no_mpi_symbol() {
    let ir = emit_ir("fact.fss");
    assert!(
        !ir.contains("fortress_mpi"),
        "a non-MPI program must not reference the MPI runtime:\n{ir}"
    );
}

#[test]
fn emit_obj_writes_a_relocatable_object_at_the_output_path() {
    let out = output_path("emitobj");
    let status = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("mpi_hello.fss"))
        .arg("--emit-obj")
        .arg("-o")
        .arg(&out)
        .status()
        .expect("could not run fortressc");
    assert!(status.success(), "fortressc --emit-obj failed: {status:?}");

    let bytes = std::fs::read(&out).expect("no object at the output path");
    assert_eq!(
        bytes.get(..4),
        Some(&[0x7f, b'E', b'L', b'F'][..]),
        "--emit-obj must write an ELF object at exactly -o, not a linked binary"
    );
    let _ = std::fs::remove_file(&out);
}

/// A stand-in link driver that records its arguments. This is how `--cc` and
/// the conditional shim injection are gated without an MPI installation.
fn link_arguments(fixture_name: &str, tag: &str) -> String {
    let log = output_path(tag).with_extension("log");
    let fake_cc = output_path(tag).with_extension("cc");
    std::fs::write(
        &fake_cc,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {}\nexit 0\n",
            log.display()
        ),
    )
    .expect("could not write the stand-in cc");
    let mut perms = std::fs::metadata(&fake_cc).expect("stat").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&fake_cc, perms).expect("chmod");
    let _ = std::fs::remove_file(&log);

    let status = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture(fixture_name))
        .arg("-o")
        .arg(output_path(tag))
        .arg("--cc")
        .arg(&fake_cc)
        .status()
        .expect("could not run fortressc");
    assert!(status.success(), "fortressc failed with --cc: {status:?}");

    let recorded = std::fs::read_to_string(&log).expect("--cc was not invoked");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&fake_cc);
    let _ = std::fs::remove_file(output_path(tag));
    recorded
}

/// One argument per line, so `.shims.c` and `.mpi_shims.c` are distinguishable.
fn linked_a(args: &str, suffix: &str) -> bool {
    args.lines().any(|line| line.ends_with(suffix))
}

#[test]
fn the_mpi_shim_is_linked_into_a_program_that_uses_mpi() {
    let args = link_arguments("mpi_hello.fss", "cc-mpi");
    assert!(
        linked_a(&args, ".mpi_shims.c"),
        "the MPI shim was not linked:\n{args}"
    );
    assert!(
        linked_a(&args, ".shims.c"),
        "the base runtime was not linked:\n{args}"
    );
}

#[test]
fn the_mpi_shim_stays_out_of_a_program_that_does_not_use_mpi() {
    let args = link_arguments("fact.fss", "cc-plain");
    assert!(
        !linked_a(&args, ".mpi_shims.c"),
        "a non-MPI program must not drag in the MPI runtime:\n{args}"
    );
    assert!(
        linked_a(&args, ".shims.c"),
        "the base runtime was not linked:\n{args}"
    );
}

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

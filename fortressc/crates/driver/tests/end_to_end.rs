//! The tracer bullet, as a gate rather than a demo: a `.fss` file becomes an
//! ELF that runs and exits with the skeleton constant.

// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules; an
// integration test is its own crate, so the workspace denies apply here.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

const SKELETON_EXIT_CODE: i32 = 42;

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

#[test]
fn a_fortress_source_file_becomes_a_running_native_binary() {
    let binary = compile_fixture("skeleton.fss", "run");
    let status = Command::new(&binary)
        .status()
        .expect("could not run the produced binary");
    assert_eq!(
        status.code(),
        Some(SKELETON_EXIT_CODE),
        "the binary must exit with the skeleton constant"
    );
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
fn the_emitted_ir_returns_the_constant() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("skeleton.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let ir = String::from_utf8_lossy(&out.stdout);
    assert!(ir.contains("define i32 @main()"), "unexpected IR:\n{ir}");
    assert!(ir.contains("ret i32 42"), "unexpected IR:\n{ir}");
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
fn the_m1_acceptance_program_survives_the_whole_pipeline() {
    // Lexer and parser are real here; only codegen is still a placeholder.
    let binary = compile_fixture("fact.fss", "fact");
    let status = Command::new(&binary)
        .status()
        .expect("could not run the produced binary");
    assert_eq!(status.code(), Some(SKELETON_EXIT_CODE));
    let _ = std::fs::remove_file(&binary);
}

#[test]
fn the_driver_reports_what_it_parsed() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("fact.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("parsed component `fact` with 2 declaration(s)"),
        "the AST must actually reach the driver:\n{stderr}"
    );
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

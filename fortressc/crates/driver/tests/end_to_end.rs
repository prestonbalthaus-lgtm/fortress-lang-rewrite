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

/// The stderr of a compile that must be refused. Exit 1 and nothing else: 70
/// is an internal error and 101 is a panic, and both mean the compiler broke
/// rather than reported.
fn refusal(name: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture(name))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(1), "{message}");
    message
}

/// The IR the compiler emits for a fixture, as text. `--emit-ir` writes to
/// stdout and never links.
fn emitted_ir(name: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture(name))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    assert!(out.status.success(), "fortressc --emit-ir failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
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
fn the_produced_binary_carries_no_vm_runtime() {
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

// ------------------------------------------------- M3: the target CPU
// The build host and the compute node are not the same machine, so the CPU the
// object is built for is a decision rather than whatever `cargo` happened to
// run on.

fn emit_ir_with(fixture_name: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture(fixture_name))
        .arg("--emit-ir")
        .args(extra)
        .output()
        .expect("could not run fortressc")
}

#[test]
fn the_default_target_cpu_is_x86_64_v3_not_whatever_built_it() {
    let out = emit_ir_with("fact.fss", &[]);
    let ir = String::from_utf8_lossy(&out.stdout);
    assert!(
        ir.contains("\"target-cpu\"=\"x86-64-v3\""),
        "the default must be a named baseline, not the host:\n{ir}"
    );
}

#[test]
fn target_cpu_selects_the_cluster_part() {
    let out = emit_ir_with("fact.fss", &["--target-cpu", "skylake-avx512"]);
    let ir = String::from_utf8_lossy(&out.stdout);
    assert!(
        ir.contains("\"target-cpu\"=\"skylake-avx512\""),
        "--target-cpu did not reach the emitted functions:\n{ir}"
    );
}

#[test]
fn the_module_carries_a_triple_and_a_data_layout() {
    let out = emit_ir_with("fact.fss", &[]);
    let ir = String::from_utf8_lossy(&out.stdout);
    assert!(ir.contains("target datalayout ="), "no data layout:\n{ir}");
    assert!(ir.contains("target triple ="), "no triple:\n{ir}");
}

/// LLVM answers an unknown processor name with a warning on stderr and then
/// silently builds for the baseline. That is a wrong binary, not a failed one,
/// so the driver refuses the name itself.
#[test]
fn an_unknown_target_cpu_is_refused_rather_than_silently_ignored() {
    let out = emit_ir_with("fact.fss", &["--target-cpu", "skylake-avx1024"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "an unknown CPU must be a user error"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("x86-64-v3") && stderr.contains("skylake-avx512"),
        "the diagnostic should list what is accepted:\n{stderr}"
    );
}

#[test]
fn native_is_available_for_a_build_that_runs_where_it_was_built() {
    let out = emit_ir_with("fact.fss", &["--target-cpu", "native"]);
    assert_eq!(out.status.code(), Some(0), "`native` must be accepted");
    let ir = String::from_utf8_lossy(&out.stdout);
    assert!(
        ir.contains("\"target-cpu\"=") && !ir.contains("\"target-cpu\"=\"native\""),
        "`native` must resolve to a real part name:\n{ir}"
    );
}

// --------------------------------------------------- M3a: the collector
// Every heap allocation in a Fortress program goes through `fortress_alloc` in
// runtime/shims.c. M1 accepted that it never freed; M3a replaces the body with
// a collector, which is why the allocation path was centralised in the first
// place. The proof that memory stays flat is `tools/memory-gate.sh`, which
// needs an RSS measurement cargo cannot make.

/// M4 made the collector a STATIC archive, so its symbols are defined inside
/// the binary rather than left for the loader. `nm -u` finds nothing now, and
/// `ldd` showing no libgc is the assertion rather than the failure: a Fortress
/// binary carries its collector and needs no library path to run, which is what
/// makes it launchable under srun on a compute node.
#[test]
fn allocation_goes_through_the_collector() {
    let binary = compile_fixture("skeleton.fss", "gcsym");
    let symbols = Command::new("nm")
        .arg(&binary)
        .output()
        .expect("could not run nm");
    let defined = String::from_utf8_lossy(&symbols.stdout);
    assert!(
        defined
            .lines()
            .any(|line| line.contains(" T GC_malloc_atomic")),
        "the collector is not linked into the binary"
    );

    let deps = Command::new("ldd")
        .arg(&binary)
        .output()
        .expect("could not run ldd");
    let deps = String::from_utf8_lossy(&deps.stdout);
    assert!(
        !deps.contains("libgc"),
        "the collector must be static, not a runtime dependency:\n{deps}"
    );
    let _ = std::fs::remove_file(&binary);
}

/// The collector has to be started before the first allocation, and `main` is
/// the only place that is guaranteed to run first.
#[test]
fn generated_main_starts_the_runtime_before_the_program() {
    let ir = emit_ir("skeleton.fss");
    let main = ir
        .split("define i32 @main()")
        .nth(1)
        .unwrap_or_else(|| panic!("no main in:\n{ir}"));
    let init = main
        .find("call void @fortress_runtime_init()")
        .unwrap_or_else(|| panic!("main does not start the runtime:\n{main}"));
    let run = main
        .find("call void @run()")
        .unwrap_or_else(|| panic!("main does not call run:\n{main}"));
    assert!(
        init < run,
        "the runtime must start before the program:\n{main}"
    );
}

#[test]
fn the_soak_program_runs_to_completion() {
    let binary = compile_fixture("gcsoak_lite.fss", "soak");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "done\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

// ------------------------------------------------ M3b: arrays and iteration

/// The milestone: allocate, populate with a loop, read back, and sum.
/// 0^2 + ... + 99^2 is 99*100*199/6.
#[test]
fn an_array_program_populates_itself_and_computes_the_right_sum() {
    let binary = compile_fixture("arraysum.fss", "arraysum");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "length = 100\nsum = 328350\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// Out of bounds is a fact about the program and should read like one. A
/// segmentation fault is not a diagnostic.
#[test]
fn an_out_of_bounds_subscript_halts_cleanly_rather_than_faulting() {
    let binary = compile_fixture("oob.fss", "oob");
    let out = run(&binary);
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected a clean exit; a `None` code means it was killed by a signal"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("out of bounds") && stderr.contains("(5, 3)"),
        "the diagnostic should name the index and the length:\n{stderr}"
    );
    let _ = std::fs::remove_file(&binary);
}

/// A mutable declared inside a loop body gets one stack slot, not one per
/// iteration. Without an entry-block `alloca` this overflows the stack.
#[test]
fn a_mutable_declared_in_a_loop_body_does_not_grow_the_stack() {
    let binary = compile_fixture("loopalloca.fss", "loopalloca");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1000000\n");
    assert_eq!(
        out.status.code(),
        Some(0),
        "a signal here is a stack overflow"
    );
    let _ = std::fs::remove_file(&binary);
}

#[test]
fn a_while_loop_lowers_to_the_three_expected_blocks() {
    let ir = emit_ir("loopalloca.fss");
    for expected in ["loop.cond:", "loop.body:", "loop.end:"] {
        assert!(ir.contains(expected), "missing {expected} in:\n{ir}");
    }
}

#[test]
fn every_alloca_sits_in_the_entry_block() {
    let ir = emit_ir("loopalloca.fss");
    let last_alloca = ir
        .rfind("alloca")
        .unwrap_or_else(|| panic!("no alloca in:\n{ir}"));
    let first_loop = ir
        .find("loop.cond:")
        .unwrap_or_else(|| panic!("no loop in:\n{ir}"));
    assert!(
        last_alloca < first_loop,
        "an alloca after the first loop label means one per iteration:\n{ir}"
    );
}

#[test]
fn a_subscript_goes_through_the_bounds_checked_slot_shim() {
    let ir = emit_ir("arraysum.fss");
    for expected in [
        "call ptr @fortress_array_alloc(",
        "call ptr @fortress_array_slot(",
        "call i64 @fortress_array_length(",
    ] {
        assert!(ir.contains(expected), "missing {expected} in:\n{ir}");
    }
}

// ------------------------------------ M3c: traits, objects and dispatch

/// The matrix, with the expected answer computed here rather than read out of
/// the program: the four cells of Ink x Face, then a statically concrete call,
/// then two field reads and one on a freshly built object.
#[test]
fn every_cell_of_the_dispatch_matrix_reaches_its_own_declaration() {
    let binary = compile_fixture("dispatch.fss", "dispatch");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "3000\n2000\n1000\n4000\n3000\n5\nsq\n9\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The regression that a "one applicable declaration statically" shortcut would
/// introduce: it would bind both calls to `name(Ink)` and print 2, 2, 2.
#[test]
fn the_run_time_type_decides_and_not_the_static_one() {
    let binary = compile_fixture("specificity.fss", "specificity");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n1\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

#[test]
fn an_ambiguous_call_is_refused_at_compile_time_and_names_both_declarations() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("ambiguous.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "an ambiguity is a user diagnostic, not a compiler bug:\n{message}"
    );
    assert!(message.contains("is ambiguous for (OL, OR)"), "{message}");
    assert!(
        message.contains("the declarations at"),
        "the diagnostic must name both declarations:\n{message}"
    );
}

/// Generated code names the shims and nothing else: no `GC_malloc`, no second
/// allocation path, and the tag written where the collector cannot mistake it
/// for a pointer.
#[test]
fn an_object_is_allocated_through_the_scanned_shim_and_dispatch_loads_a_tag() {
    let ir = emit_ir("dispatch.fss");
    for expected in [
        "call ptr @fortress_object_alloc",
        "@fortress_dispatch_failed",
        "switch i32 %tag",
        "unreachable",
    ] {
        assert!(ir.contains(expected), "missing {expected} in:\n{ir}");
    }
    for forbidden in ["GC_malloc", "@malloc"] {
        assert!(!ir.contains(forbidden), "{forbidden} reached the IR:\n{ir}");
    }
}

#[test]
fn a_dispatch_leaf_is_a_direct_call_so_callees_stay_inlinable() {
    let ir = emit_ir("dispatch.fss");
    for expected in [
        "call i32 @\"draw$Solid_Round\"",
        "call i32 @\"draw$Solid_Face\"",
        "call i32 @\"draw$Dotted_Square\"",
        "call i32 @\"draw$Ink_Face\"",
    ] {
        assert!(ir.contains(expected), "missing {expected} in:\n{ir}");
    }
    assert!(
        !ir.contains("indirectbr"),
        "no leaf may become an indirect branch:\n{ir}"
    );
}

// ------------------------------------ M3d: generics by monomorphization

#[test]
fn a_generic_is_stamped_out_once_per_static_argument() {
    let binary = compile_fixture("generics.fss", "generics");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "7\nhi\n2\n3\nno\nsecond\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The whole reason for monomorphization rather than erasure or boxing.
#[test]
fn a_zz64_instantiation_stores_an_i64_and_not_a_pointer() {
    let ir = emit_ir("generics.fss");
    assert!(
        ir.contains(r#"%"Cell$ZZ64$e" = type { i32, i32, i64, i32 }"#),
        "ZZ64 must be stored unboxed:\n{ir}"
    );
    assert!(
        ir.contains(r#"%"Cell$String$e" = type { i32, i32, ptr, i32 }"#),
        "String is genuinely a pointer:\n{ir}"
    );
    assert!(
        ir.contains(r#"define i64 @"pick$ZZ64$e"(i64"#),
        "the instantiation takes and returns raw i64:\n{ir}"
    );
}

/// The M3c interaction: each instantiation is a concrete type under the trait,
/// so each needs its own tag and its own switch arm. Without the phase split the
/// table would have been built before these types existed.
#[test]
fn every_instantiation_under_a_trait_gets_a_dispatch_arm() {
    let binary = compile_fixture("genericdispatch.fss", "genericdispatch");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n3\n4\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);

    let ir = emit_ir("genericdispatch.fss");
    assert!(ir.contains("switch i32 %tag"), "no dispatch emitted:\n{ir}");
    for expected in [
        r#"call i32 @"area$Box$ZZ64$e""#,
        r#"call i32 @"area$Box$String$e""#,
    ] {
        assert!(ir.contains(expected), "missing {expected} in:\n{ir}");
    }
}

#[test]
fn polymorphic_recursion_is_refused_rather_than_compiled_or_hung_on() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("polyrec.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a ceiling is a user diagnostic, not a compiler bug:\n{message}"
    );
    assert!(
        message.contains("4096"),
        "the limit must be named:\n{message}"
    );
}

/// The same ceiling on the stamp path, which had no witness of its own. The
/// failure this pins is a HANG rather than a wrong answer: a generic method
/// demanding itself at a strictly larger type generates stamps without end.
#[test]
fn a_generic_method_that_stamps_itself_larger_stops_at_the_ceiling() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("stampceiling.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a ceiling is a user diagnostic, not a compiler bug:\n{message}"
    );
    assert!(
        message.contains("4096"),
        "the limit must be named:\n{message}"
    );
}

/// Tags are switch keys, and switch arms follow tag order. If instantiations
/// were numbered as a worklist discovered them, the emitted module would depend
/// on traversal order rather than on the source.
#[test]
fn two_builds_of_a_generic_program_are_byte_identical() {
    let first = output_path("determinism-a").with_extension("o");
    let second = output_path("determinism-b").with_extension("o");
    for path in [&first, &second] {
        let status = Command::new(env!("CARGO_BIN_EXE_fortressc"))
            .arg(fixture("genericdispatch.fss"))
            .arg("--emit-obj")
            .arg("-o")
            .arg(path)
            .status()
            .expect("could not run fortressc");
        assert!(status.success());
    }
    let a = std::fs::read(&first).expect("first object");
    let b = std::fs::read(&second).expect("second object");
    assert_eq!(a, b, "the emitted object is not reproducible");
    let _ = std::fs::remove_file(&first);
    let _ = std::fs::remove_file(&second);
}

/// The most common function shape in the corpus, and it had never compiled.
#[test]
fn a_void_function_compiles_links_and_runs() {
    let binary = compile_fixture("unitvoid.fss", "unitvoid");
    let out = run(&binary);
    assert!(out.status.success(), "exited {:?}", out.status);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "hello from a void function\n"
    );
}

/// M3f: an identifier with no local binding, juxtaposed with one operand, is a
/// function application. This is the whole reason `println "Hello"` is 48 files
/// of the missing-name histogram.
#[test]
fn juxtaposition_of_a_function_is_application() {
    let binary = compile_fixture("juxtapply.fss", "juxtapply");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Hello\n42\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The guard. A parameter that shadows a function name is a value, so `f y` is
/// multiplication and not a call. Dropping the `lookup` test silently changes
/// what this program computes, which is why it is a test and not a comment.
#[test]
fn a_shadowed_function_name_is_not_application() {
    let binary = compile_fixture("juxtshadow.fss", "juxtshadow");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
    let _ = std::fs::remove_file(&binary);
}

/// An integer literal that takes RR64 from context is a float constant. Typed
/// RR64 but lowered as an i64 it reached `arith`, which requires a float value,
/// and the compiler panicked on `halve(x: RR64): RR64 = x/2` -- ordinary
/// Fortress, and two corpus files. Found by the full driver sweep.
#[test]
fn an_integer_literal_in_float_position_is_a_float_constant() {
    let binary = compile_fixture("rr64literal.fss", "rr64literal");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1.75\n");
    let _ = std::fs::remove_file(&binary);
}

/// The one property of chaining that is observable from inside the language:
/// the middle operand is evaluated exactly once. This subset has no mutable
/// global and no closure, so the counter is a print.
#[test]
fn a_chain_evaluates_its_middle_operand_once() {
    let binary = compile_fixture("chainonce.fss", "chainonce");
    let out = run(&binary);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.matches("MID").count(),
        1,
        "the middle operand ran more than once: {stdout}"
    );
    assert!(stdout.contains("YES"), "the chain was false: {stdout}");
    let _ = std::fs::remove_file(&binary);
}

#[test]
fn a_chain_mixing_equivalence_with_one_sense_is_true() {
    let binary = compile_fixture("chainmixed.fss", "chainmixed");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "YES\n");
    let _ = std::fs::remove_file(&binary);
}

// -------------------------------- M3j: methods on and of generic types

/// A ground method on a generic owner. Two things had to be fixed for this to
/// compile at all, and both are visible in the output rather than in a comment:
/// the return type is substituted (`unknown type T` before), and the slot map
/// is no longer keyed by span -- two instantiations of one template are clones
/// and share it, so `get` resolved to one signature for both cells.
#[test]
fn a_method_on_a_generic_owner_is_substituted_per_instantiation() {
    let binary = compile_fixture("genericowner.fss", "genericowner");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\nhi\n7\nhi\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The same fixture, seen from the object side: the two instantiations really
/// are two functions over two layouts, which is what a shared slot destroyed.
#[test]
fn each_instantiation_of_a_method_gets_its_own_symbol() {
    let ir = emit_ir("genericowner.fss");
    assert!(
        ir.contains(r#"define i32 @"Cell$ZZ32$e$m$get""#),
        "the ZZ32 cell needs its own method returning i32:\n{ir}"
    );
    assert!(
        ir.contains(r#"define ptr @"Cell$String$e$m$get""#),
        "the String cell needs its own method returning ptr:\n{ir}"
    );
}

/// A `self` parameter lifts a member into the TOP-LEVEL overload set of its
/// name, alongside a real top-level declaration of it. Six numbers, and each
/// one is a different rule: the override, the inherited default, the top-level
/// member of the same set, `self` written second, and dispatch deciding on the
/// run-time type twice.
#[test]
fn a_functional_method_joins_the_top_level_overload_set() {
    let binary = compile_fixture("functionalmethod.fss", "functionalmethod");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "16\n0\n107\n15\n9\n0\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The symbol rule. A functional method is owner qualified and never bare,
/// because the set it joins may already hold a real top-level `area`; and the
/// overload count has to span both kinds or two members take one symbol.
#[test]
fn a_functional_method_symbol_is_owner_qualified() {
    let ir = emit_ir("functionalmethod.fss");
    for symbol in [
        r#"@"Square$f$area$Square""#,
        r#"@"Shape$f$area$Shape""#,
        r#"@"area$zz32""#,
    ] {
        assert!(ir.contains(symbol), "missing {symbol}:\n{ir}");
    }
}

/// Generic dotted methods, by over-approximation. Expansion has no types, so
/// `o.f[\ZZ32\]()` stamps `f` into every type declaring a generic `f` of
/// matching arity; the five numbers are the receiver deciding, twice of it at
/// run time through a trait.
#[test]
fn a_generic_dotted_method_dispatches_on_its_receiver() {
    let binary = compile_fixture("genericmethod.fss", "genericmethod");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n1\n2\n6\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The over-approximation, stated in symbols rather than in prose. `Unused`
/// never appears at a call site and still gets both stamps, because the pass
/// that makes them cannot see a receiver; `Spare` declares `f` at an arity
/// nothing demands and gets none.
#[test]
fn a_stamp_lands_on_every_matching_type_and_no_other() {
    let ir = emit_ir("genericmethod.fss");
    for symbol in [
        r#"@"O$m$f$ZZ32$e""#,
        r#"@"P$m$f$ZZ32$e""#,
        r#"@"Unused$m$f$ZZ32$e""#,
        r#"@"Unused$m$f$String$e""#,
    ] {
        assert!(ir.contains(symbol), "missing {symbol}:\n{ir}");
    }
    assert!(
        !ir.contains(r#"@"Spare$m$f"#),
        "an arity that matches nothing must take no stamp:\n{ir}"
    );
}

// ------------------------------ M3k: primitive operators and builtins

/// AND and OR short circuit. The truth table cannot show it -- `true AND
/// false` is false either way -- so the witness is a right operand that
/// prints, and its output being absent.
#[test]
fn and_and_or_never_evaluate_a_right_operand_they_do_not_need() {
    let binary = compile_fixture("logical.fss", "logical");
    let out = run(&binary);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout,
        "true\nfalse\ntrue\nfalse\nfalse\ntrue\nfalse\ntrue\nfalse\ntrue\ntrue\n"
    );
    assert_eq!(
        stdout.matches("RHS").count(),
        0,
        "a right operand ran that should not have: {stdout}"
    );
    let _ = std::fs::remove_file(&binary);
}

/// And the shape underneath it: a conditional branch and a phi. A `select`
/// would compute both sides, which is the thing being ruled out.
#[test]
fn a_short_circuit_is_a_branch_and_a_phi_and_not_a_select() {
    let ir = emit_ir("logical.fss");
    assert!(ir.contains("br i1"), "no conditional branch emitted:\n{ir}");
    assert!(ir.contains("phi i1"), "no phi over the two arms:\n{ir}");
    assert!(
        !ir.contains("select i1"),
        "a select evaluates both operands:\n{ir}"
    );
}

/// `^` is left associative and above juxtaposition. `2^3^2` is 64 under left
/// association and 512 under right, so the number is the whole assertion.
#[test]
fn exponentiation_is_left_associative_and_binds_above_juxtaposition() {
    let binary = compile_fixture("exponent.fss", "exponent");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1024\n64\n18\n18\n5\n0.00390625\n256\n0.00390625\n256\n"
    );
    let _ = std::fs::remove_file(&binary);
}

#[test]
fn print_writes_no_newline_and_ignore_still_evaluates() {
    let binary = compile_fixture("builtins.fss", "builtins");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "ab1true\nSIDE\ndone\n"
    );
    let _ = std::fs::remove_file(&binary);
}

/// A negative integer exponent has no integer answer, and a failed assert has
/// nothing left to do. Both halt with a diagnostic and exit 1 rather than
/// inventing a value or carrying on.
#[test]
fn a_negative_exponent_and_a_failed_assert_both_halt_cleanly() {
    for (fixture, phrase) in [
        ("negexponent.fss", "negative exponent"),
        ("assertfail.fss", "assertion failed"),
    ] {
        let name = fixture.trim_end_matches(".fss");
        let binary = compile_fixture(fixture, name);
        let out = run(&binary);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{fixture} must halt with exit 1: {stderr}"
        );
        assert!(stderr.contains(phrase), "{fixture}: {stderr}");
        let _ = std::fs::remove_file(&binary);
    }
}

// ---------------------------------------------- M4: parallel execution

/// The correctness claim, and it is the whole array rather than a sample:
/// a million elements dumped in index order must be byte for byte what a
/// serial fill produces, at every worker count.
#[test]
fn a_parallel_fill_is_byte_identical_to_a_serial_one() {
    let binary = compile_fixture("parallelfill.fss", "parallelfill");
    let run_with = |workers: &str| {
        let out = Command::new(&binary)
            .env("FORTRESS_WORKERS", workers)
            .output()
            .expect("could not run the fill");
        assert_eq!(out.status.code(), Some(0));
        out.stdout
    };
    let serial = run_with("1");
    assert_eq!(serial.iter().filter(|b| **b == b'\n').count(), 1_000_000);
    assert_eq!(serial, run_with("4"), "4 workers disagreed with serial");
    assert_eq!(serial, run_with("8"), "8 workers disagreed with serial");
    let _ = std::fs::remove_file(&binary);
}

/// `seq(...)` is a promise about order, and 5000 is above the size at which
/// everything else runs inline -- so this really does test the flag rather
/// than the threshold.
#[test]
fn a_sequential_loop_runs_in_index_order() {
    let binary = compile_fixture("parallelseq.fss", "parallelseq");
    let out = run(&binary);
    let stdout = String::from_utf8_lossy(&out.stdout);
    for (index, line) in stdout.lines().enumerate() {
        assert_eq!(line, index.to_string(), "out of order at line {index}");
    }
    assert_eq!(stdout.lines().count(), 5000);
    let _ = std::fs::remove_file(&binary);
}

/// The body is OUTLINED into a real function taking an index and an
/// environment, and the environment is allocated ONCE -- outside the loop.
/// Allocation inside the parallel region is what makes an allocating loop
/// collect N times as often and run slower than the serial one.
#[test]
fn a_loop_body_is_outlined_and_its_environment_allocated_once() {
    let ir = emit_ir("parallelfill.fss");
    assert!(
        ir.contains(r#"define void @"$loop1"(i64 %0, ptr %1, i64 %2)"#),
        "the body was not outlined:\n{ir}"
    );
    let run_body = ir
        .split("define void @run()")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .unwrap_or_default();
    assert_eq!(
        run_body.matches("call ptr @fortress_env_alloc").count(),
        1,
        "the environment must be allocated exactly once:\n{run_body}"
    );
    assert!(
        run_body.contains("call void @fortress_parallel_for"),
        "the loop does not reach the runtime:\n{run_body}"
    );
}

/// The scope boundary, and it is the whole of M4's race freedom: a parallel
/// body may only assign to storage its own iteration owns.
#[test]
fn a_parallel_body_may_not_assign_outside_itself() {
    for (name, phrase) in [
        ("badparallelescape.fss", "is declared outside this loop"),
        ("badparallelindex.fss", "the element its own iteration owns"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
            .arg(fixture(name))
            .arg("--emit-obj")
            .arg("-o")
            .arg("/dev/null")
            .output()
            .expect("could not run fortressc");
        let message = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{name}: {message}");
        assert!(message.contains(phrase), "{name}: {message}");
    }
}

// ------------------------------------------------------------------------ M5

/// The headline. A reduction over a range well above the runtime's inline
/// threshold folds to the serial answer exactly, at every worker count --
/// ZZ64 addition is associative whatever the grouping, so this is an equality
/// and not a tolerance.
#[test]
fn a_reduction_folds_to_the_serial_answer_at_every_worker_count() {
    let binary = compile_fixture("reductionsum.fss", "reductionsum");
    for workers in ["1", "2", "8", "16"] {
        let out = Command::new(&binary)
            .env("FORTRESS_WORKERS", workers)
            .output()
            .expect("could not run the produced binary");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "499999500000\n",
            "the sum of 0..999999 on {workers} worker(s)"
        );
    }
    let _ = std::fs::remove_file(&binary);
}

/// `+=` and `-=` on two ZZ32 variables inside one `atomic`. The negative
/// answer is the point: `-=` accumulates `Identity - e` and the merge folds
/// with `+`, so a merge using the wrong operator comes back positive.
#[test]
fn two_zz32_reductions_in_one_atomic_block_are_exact() {
    let binary = compile_fixture("reductionzz32.fss", "reductionzz32");
    let out = Command::new(&binary)
        .env("FORTRESS_WORKERS", "8")
        .output()
        .expect("could not run the produced binary");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "100000\n-99000\n");
    let _ = std::fs::remove_file(&binary);
}

/// The exit-70 internal error M4 shipped latent, and its own diagnostic walked
/// the user into it: refuse the parallel form, recommend `seq(...)`, and the
/// seq form crashed. No corpus file writes this shape, which is why M4's
/// full-driver sweep found nothing to catch it with.
#[test]
fn a_seq_loop_may_assign_to_a_scalar_outside_it() {
    let binary = compile_fixture("seqouterassign.fss", "seqouterassign");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0), "this used to be exit 70");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "499500\n");
    let _ = std::fs::remove_file(&binary);
}

/// The lock path. A build that kept M4's by-value capture would be silently
/// wrong WITH THE LOCK HELD -- every worker incrementing its own loop-entry
/// copy, the update lost anyway.
#[test]
fn an_atomic_assignment_reaches_the_callers_storage() {
    let binary = compile_fixture("atomiclocked.fss", "atomiclocked");
    let out = Command::new(&binary)
        .env("FORTRESS_WORKERS", "8")
        .output()
        .expect("could not run the produced binary");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "200000\n");
    let _ = std::fs::remove_file(&binary);
}

/// `atomic` around a parallel loop. `fortress_atomic_enter` hands over the
/// runtime's in-parallel flag so the inner loop runs inline; without it the
/// workers block on the mutex the calling thread holds and the calling thread
/// parks at the join. A recursive mutex does not help -- recursion rescues
/// re-entry by the same thread, and the workers are different threads.
#[test]
fn an_atomic_around_a_parallel_loop_does_not_deadlock() {
    let binary = compile_fixture("atomicoutside.fss", "atomicoutside");
    let out = Command::new(&binary)
        .env("FORTRESS_WORKERS", "8")
        .output()
        .expect("could not run the produced binary");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1000000\n");
    let _ = std::fs::remove_file(&binary);
}

/// M4's boundary is LEXICAL, and an array travels by pointer, so a callee's
/// `a[j] := v` is checked against an empty loop context and refused by
/// nothing. M5 may not weaken the loop rules while that is open.
#[test]
fn a_shared_array_may_not_be_handed_to_a_call_in_a_parallel_body() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("badsharedarray.fss"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(
        message.contains("out of reach of the loop's own rules"),
        "{message}"
    );
}

/// reduction.tex:35's third condition. A name the body also READS is not a
/// reduction, and the verdict needs the finished body -- decide it at the
/// assignment and this file reads as a private accumulator AND a captured read
/// of the same storage.
#[test]
fn a_compound_assignment_to_a_name_the_body_reads_is_not_a_reduction() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("badreductionread.fss"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(
        message.contains("is declared outside this loop"),
        "{message}"
    );
}

/// The phase split, and it is a SILENT WRONG ANSWER without it: an inferred
/// return type used to be backpatched after the body was walked, so every call
/// site typed before that read the `Void` placeholder. `println(f())` printed
/// an empty line at exit 0, which no compile metric can see.
///
/// `chainTop` is the part that needs a FIXPOINT rather than one ordered sweep:
/// three inferred signatures in a chain, each written above the one it calls,
/// so a single round in declaration order resolves exactly one of them and
/// `chain` comes out as the empty line again.
#[test]
fn an_inferred_return_type_is_visible_to_a_caller_declared_above_it() {
    let binary = compile_fixture("inferredreturn.fss", "inferredreturn");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "greeting\nchain\n42\ngreeting\ntag\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The same defect, and for a method it did not need a source order to trigger:
/// every method body is checked after every function body, so a call from a
/// top-level function ALWAYS read the placeholder.
///
/// The first line is
/// `ProjectFortress/compiler_regressions/parent_method_override.fss`, which
/// compiled to exit 0 and printed an empty line where PASS belongs -- inside
/// the 280, and invisible to the number.
#[test]
fn an_inferred_method_return_type_is_resolved_before_any_function_body() {
    let binary = compile_fixture("inferredmethod.fss", "inferredmethod");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "Child.foo(Child) PASS\n42\nCounter\nChild.foo(Child) PASS\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The other half, and a `.fss` file is the only way to reach it:
/// `dispatch_target` memoises with `or_insert_with`, so the FIRST table
/// computed for a set is the one codegen emits. The signature pass computes
/// tables while the return types are still settling, and every one of them has
/// to be thrown away -- keep them and LLVM rejects the module with `ret void`
/// against an `i32` return.
#[test]
fn a_dispatch_table_built_while_signatures_were_settling_is_discarded() {
    let binary = compile_fixture("inferreddispatch.fss", "inferreddispatch");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n1\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

// ------------------------------------------------------- mutable fields

/// A mutable field is storage, and the store is DIRECT.
/// `Specification/basic/expressions/bindings.tex:60-61` says assigning a field
/// calls the corresponding setter; there is no setter machinery in this
/// compiler -- accessors are skipped at every member walk -- so calling one
/// would mean inventing it. The deviation is named on `AssignTarget::Field`.
///
/// Three spellings in one program, because they take three different paths:
/// `c.n := 5` is the dotted target, `c.n += 2` is the compound form that has to
/// evaluate the receiver ONCE for the load and the store alike, and `n := n + 1`
/// inside a method is the bare name that resolves to a field of `self`.
#[test]
fn a_mutable_field_is_written_by_all_three_spellings() {
    let binary = compile_fixture("mutablefield.fss", "mutablefield");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "8\n18\nrenamed\n0\n41\n51\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The collector, and this is the first storage in the language a WRITE can put
/// a pointer into after the block was allocated. An object goes through
/// `fortress_alloc_scanned`, so the field is scanned and what it names survives
/// unrelated allocation. Atomic memory is NOT scanned -- `runtime/tests/
/// array_trace.c` measured that -- so getting an object onto the atomic path
/// would free the string this field is still holding.
#[test]
fn a_pointer_stored_into_a_mutable_field_survives_a_collection() {
    let binary = compile_fixture("gcfield.fss", "gcfield");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "kept-across-collection-7\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// A field is not an assignment target unless it was declared `var`, the same
/// rule a local binding has.
#[test]
fn an_immutable_field_is_not_an_assignment_target() {
    let message = refusal("badimmutablefield.fss");
    assert!(message.contains("field `w` is immutable"), "{message}");
}

/// The M5 soundness argument was "`f(h)` where `h` is an object holding an
/// array would pass; no field store exists in the language yet, so nothing can
/// exploit it today". A field store exists now, so these three are what keeps
/// it true. An array has the `a[binder]` carve-out because an index can name
/// the slot one iteration owns; a field has no index and no carve-out.
#[test]
fn a_parallel_body_may_not_write_a_field_of_anything_it_shares() {
    let message = refusal("badparallelfield.fss");
    assert!(
        message.contains("is declared outside this loop"),
        "{message}"
    );
    assert!(message.contains("a field has no index"), "{message}");
}

/// The aliasing case, and it needs its own diagnostic: `c` is loop-LOCAL, so
/// the depth comparison that is the whole of M4's race freedom calls it
/// private. It was bound from something outside the loop, which is what makes
/// it shared. A message naming the wrong mechanism sends the reader to the
/// wrong fix -- the class of defect this project has paid for twice.
#[test]
fn a_loop_local_bound_from_shared_storage_is_still_shared() {
    let message = refusal("badaliasedfield.fss");
    assert!(
        message.contains("declared inside this loop but bound from storage outside it"),
        "{message}"
    );
}

/// The array refusal one indirection out. Reachability is computed over the
/// registry, so an object holding an object holding an array is refused too,
/// and the diagnostic names the path it found.
#[test]
fn an_object_that_reaches_mutable_storage_may_not_be_handed_to_a_call() {
    let message = refusal("badsharedobject.fss");
    assert!(
        message.contains("reaches mutable storage through `b.n`"),
        "{message}"
    );
}

/// What the three refusals leave available, and it is exact rather than
/// approximately right: `atomic` serialises the write, so the count is the same
/// at every worker count. Both halves matter -- the second loop writes the
/// field from a CALLEE, which is the path the reachability refusal covers.
#[test]
fn an_atomic_field_write_counts_exactly_at_every_worker_count() {
    let binary = compile_fixture("atomicfield.fss", "atomicfield");
    for workers in ["1", "2", "8", "16"] {
        let out = Command::new(&binary)
            .env("FORTRESS_WORKERS", workers)
            .output()
            .expect("could not run the produced binary");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "100000\n200000\n",
            "at FORTRESS_WORKERS={workers}"
        );
        assert_eq!(out.status.code(), Some(0));
    }
    let _ = std::fs::remove_file(&binary);
}

// -------------------------------------------------- control flow extras

/// SPIKE-CONTROL-FLOW-EXTRAS, all three in one program.
///
/// `case` desugars to an if/elif chain over `subject = guard`, so every rule
/// about which types `=` is defined on is the rule `infix` already enforces.
/// `typecase` is a switch on the 32-bit tag at offset 0 -- the same load
/// `dispatch_node` does, because a trait has no run-time representation and an
/// arm naming one is the set of concrete tags below it. `label`/`exit` is one
/// merge block with a phi over its incoming edges: a forward jump inside one
/// function, which is why none of this needs unwinding.
///
/// The `area` lines are the tag arithmetic: 2*2*3 = 12 for a Circle, 5*5 = 25
/// for a Square, and 0 from the `else` for the Dot no arm claims.
#[test]
fn case_typecase_and_label_all_produce_the_right_values() {
    let binary = compile_fixture("controlflow.fss", "controlflow");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "circle 3\nsquare 4\nsomething else\n12\n25\n0\none\ntwo\nmany\n11\n-1\n100\n-1\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The subject of a `case` is evaluated ONCE. Inlining it into every guard runs
/// its side effects once per arm, which is the defect M3f's chained comparison
/// already paid for -- three arms here, and exactly one `evaluated`.
#[test]
fn a_case_subject_is_evaluated_once_however_many_arms_it_has() {
    let binary = compile_fixture("caseonce.fss", "caseonce");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "evaluated\nmany\n1\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// THE ATOMIC-ROLLBACK OBLIGATION, and it is the deliverable rather than the
/// garnish. `atomic.tex:59-70` has two arms and this construct re-opens the
/// writes-RETAINED one. Until there is an answer, an `exit` crossing the
/// boundary is a diagnostic: the branch would skip `fortress_atomic_leave` and
/// leave one process-wide recursive mutex held for the rest of the process.
#[test]
fn an_exit_out_of_an_atomic_region_is_refused_by_name() {
    let message = refusal("badexitatomic.fss");
    assert!(message.contains("leaves an `atomic` region"), "{message}");
    assert!(message.contains("skip the unlock"), "{message}");
}

/// Every `for` body is OUTLINED into its own function -- `seq(...)` included,
/// because one lowering serves both -- so an `exit` out of one is a jump
/// between functions. That is exactly the unwinding this construct was chosen
/// for not needing, so it is refused instead of lowered.
#[test]
fn an_exit_out_of_a_loop_body_is_refused_by_name() {
    let message = refusal("badexitloop.fss");
    assert!(message.contains("leaves a `for` body"), "{message}");
}

/// Arms are matched in order, so a trait arm above an object arm claims every
/// tag the object has. The later arm can never run.
#[test]
fn a_typecase_arm_an_earlier_arm_already_claims_is_refused() {
    let message = refusal("badtypecasedead.fss");
    assert!(message.contains("can never run"), "{message}");
}

/// A label whose exits carry a value and whose body can also run off the bottom
/// has no value on that edge. Inventing a zero for it would be the silent
/// wrong answer this compiler refuses to produce.
#[test]
fn a_label_that_exits_with_a_value_may_not_also_fall_through() {
    let message = refusal("badlabelfall.fss");
    assert!(message.contains("run off the bottom"), "{message}");
}

/// 1.0 throws `MatchFailure` when no arm matches; this subset has no
/// exceptions, so the `else` arm is what supplies the value instead. In
/// statement position a `case` needs none, which is why the rule is about the
/// value being used rather than about the arms.
#[test]
fn a_case_whose_value_is_used_needs_an_else_arm() {
    let message = refusal("badcaseelse.fss");
    assert!(message.contains("needs an `else => ...` arm"), "{message}");
}

/// The `case` fallthrough. 1.0 throws MatchFailure when no arm matches; this
/// subset has no exceptions, so a `case` used for its effect HALTS with a
/// diagnostic and exit 1 -- the answer `assert` and the dispatch tree already
/// give. Doing nothing instead would have been the silent-wrong-behaviour class
/// this compiler refuses to join, and it is what made
/// `ProjectFortress/tests/XXXcaseTest.fss` -- a must-FAIL test -- run to exit 0.
#[test]
fn a_case_that_matches_nothing_halts_rather_than_falling_through() {
    let binary = compile_fixture("caseunmatched.fss", "caseunmatched");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "before\n");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no case arm matched"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1));
    let _ = std::fs::remove_file(&binary);
}

/// The hole the first draft of the reachability pass left: it guarded the
/// arguments of a FUNCTIONAL call and nothing else. A dotted call routes
/// through `method_call`, and its receiver reaches the receiver's storage by
/// construction -- that is what a receiver is. Before field mutation no method
/// could write anything it owned, so receivers were safe by construction and
/// the guard was not missing, it was unnecessary. It is necessary now.
#[test]
fn a_shared_receiver_may_not_take_a_method_call_in_a_parallel_body() {
    let message = refusal("badsharedmethod.fss");
    assert!(
        message.contains("reaches mutable storage through `b.n`"),
        "{message}"
    );
}

/// And the argument half of the same path: `u.hit(b)` never reached the
/// argument guard either, because that guard sits on the functional branch.
#[test]
fn a_shared_object_may_not_be_a_method_argument_in_a_parallel_body() {
    let message = refusal("badsharedmethodarg.fss");
    assert!(
        message.contains("reaches mutable storage through `b.n`"),
        "{message}"
    );
}

// ------------------------------------------------ closure representation

/// SPIKE-CLOSURE-REPRESENTATION, branch (b): a named function used as a value
/// is lowered to a generated object with an `apply` method, and the call on it
/// is a dotted method call -- so it enters M3c's whole-program dispatch instead
/// of needing a representation of its own. Branch (a), a fat pointer, would
/// cost `Type` its `Copy` and touch every pass; it is only worth pricing if
/// this fails.
///
/// Seven lines, and each is a different way an arrow value travels: through a
/// parameter, through a parameter handed straight on, out of a RETURN type,
/// into a local binding, and through a second arrow type in the same program.
#[test]
fn a_named_function_travels_as_a_value_through_every_shape() {
    let binary = compile_fixture("closure.fss", "closure");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "7\n20\n8\n8\n14\n18\nhi!\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// THE BRANCH ANSWER, and it needs both halves or it answers nothing. With TWO
/// implementors of one arrow the registry builds a real table and codegen emits
/// a `switch` on the tag; with ONE it collapses to a direct call and memoises
/// no table at all. A spike measured only on the one-implementor shape would
/// report success without ever building the thing under test -- this project's
/// own `inferreddispatch` fixture exists for the same reason.
#[test]
fn two_closures_of_one_arrow_build_a_real_dispatch_table_and_one_does_not() {
    let two = emitted_ir("closure.fss");
    assert!(
        two.contains("switch i32"),
        "two implementors must reach a tag switch"
    );
    assert!(
        two.contains("apply$dispatch$Arrow$ZZ32$ZZ32"),
        "the dispatch function is named after the arrow trait"
    );
    let one = emitted_ir("closureone.fss");
    assert!(
        !one.contains("switch i32"),
        "one implementor collapses to a direct call"
    );
    assert!(
        one.contains("inc$fn$Arrow$ZZ32$ZZ32$m$apply"),
        "and it still goes through the generated object's method"
    );
}

/// The signature is checked where the object is minted, because nothing
/// downstream will: after the pass the generated object is an ordinary
/// implementor and its `apply` body is an ordinary call to the function.
#[test]
fn a_function_value_whose_signature_is_not_the_arrow_is_refused() {
    let message = refusal("badclosuresig.fss");
    assert!(
        message.contains("is used as a value of type `ZZ32 -> ZZ32`"),
        "{message}"
    );
}

// ------------------------------------------------------------ `fn`

/// `fn` on the closure representation: a generated object whose CONSTRUCTOR
/// PARAMETERS are what the body captures. That is why the body needs no
/// rewriting at all -- a dotted method reads its receiver's fields by their own
/// spelling, so a captured `k` resolves to the field `k` exactly as it resolved
/// to the enclosing local, with no environment struct and no fat pointer.
///
/// Eight lines, and the last two are the ones that matter: `adder(100)` is a
/// closure that OUTLIVES the call that made it, carrying its capture in a
/// scanned field, and `nested(7)` is a lambda whose body builds another one.
#[test]
fn a_lambda_captures_its_enclosing_bindings_and_outlives_them() {
    let binary = compile_fixture("lambda.fss", "lambda");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "6\n18\n42\n15\n115\nhi-tagged\n105\n12\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// A capture becomes a constructor parameter, and a constructor parameter needs
/// a written type. There is no inference here and no guess: `k = 10` is refused
/// by name.
#[test]
fn a_lambda_may_not_capture_a_name_with_no_written_type() {
    let message = refusal("badlambdacapture.fss");
    assert!(message.contains("has no written type"), "{message}");
}

/// The generated object binds `self` to the CLOSURE, so a captured `self` would
/// be silently shadowed by it. Refused rather than shadowed.
#[test]
fn a_lambda_may_not_capture_self() {
    let message = refusal("badlambdaself.fss");
    assert!(message.contains("may not close over `self`"), "{message}");
}

// -------------------------------------------------- BIG over ranges

/// SPIKE-BIG-OVER-RANGES. `reductions.tex:60-77` desugars `SUM[v <- g] e` into
/// `do var r = identity; for v <- g do r += e end; r end`, which is EXACTLY the
/// shape M5's recogniser already turns into a per-worker private accumulator.
/// No `Reduction` trait, no generator protocol, no closure.
///
/// EXACT AT EVERY WORKER COUNT, which is the whole assertion: a reduction that
/// is right at one worker and wrong at sixteen is the signature this project
/// has measured twice, and the PROD line is the one that had it -- with the
/// zero identity and the `+` merge it printed 1.
#[test]
fn a_big_reduction_over_a_range_is_exact_at_every_worker_count() {
    let binary = compile_fixture("bigreduction.fss", "bigreduction");
    for workers in ["1", "2", "8", "16"] {
        let out = Command::new(&binary)
            .env("FORTRESS_WORKERS", workers)
            .output()
            .expect("could not run the produced binary");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "55\n30\n55\n100\n120\n120\n10\n499999500000\n",
            "at FORTRESS_WORKERS={workers}"
        );
        assert_eq!(out.status.code(), Some(0));
    }
    let _ = std::fs::remove_file(&binary);
}

/// MAX and MIN are recognised so that they are refused BY NAME rather than read
/// as a subscript. Their identity is the type's own extremum rather than a zero
/// bit pattern, and the accumulator carries no operator that could fold them --
/// guessing zero would make a MAX over negative numbers quietly wrong.
#[test]
fn a_big_max_is_refused_by_name_rather_than_read_as_a_subscript() {
    let message = refusal("badbigmax.fss");
    assert!(message.contains("identity element"), "{message}");
}

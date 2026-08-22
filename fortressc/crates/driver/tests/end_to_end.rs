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
    // `OL` and `OR` are OPERATOR WORDS now -- `OR` is the disjunction operator --
    // so the fixture's objects were renamed and this assertion follows them.
    assert!(
        message.contains("is ambiguous for (OLeft, ORight)"),
        "{message}"
    );
    // The two declarations are secondary spans now, placed by the driver's
    // renderer as `note:` lines, because a `Display` with no source and no path
    // cannot turn a byte offset into a position.
    assert!(
        message.contains("note: one declaration is here")
            && message.contains("note: and the other is here"),
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
        // `256.0` and not `256`: the last line is an RR64, and an RR64 shows that
        // it is one. It read `256` until `rr64_needs_point` landed, which is the
        // `%g` defect Compiled7.Print17.fss asserts against.
        "1024\n64\n18\n18\n5\n0.00390625\n256\n0.00390625\n256.0\n"
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

/// An `sdiv` traps on x86-64 for a zero divisor, and SIGFPE is a core dump with
/// no diagnostic. 1.0 throws `DivideByZero`; this subset has no exceptions, so
/// division halts the way a bad subscript does.
#[test]
fn integer_division_by_zero_halts_with_a_diagnostic_rather_than_faulting() {
    let binary = compile_fixture("divzero.fss", "divzero");
    let out = run(&binary);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a trapping division must be a clean halt, not a signal:\n{stderr}"
    );
    assert!(stderr.contains("integer division by zero"), "{stderr}");
    // The halt path flushes. Without it the program loses the line it had
    // already printed, and a lost buffer looks exactly like never getting there.
    assert!(
        String::from_utf8_lossy(&out.stdout).contains('7'),
        "output produced before the halt must survive it"
    );
    let _ = std::fs::remove_file(&binary);
}

/// The other trapping operand pair. Its quotient is not representable, and
/// delegating the 32 bit width to the 64 bit one would return a truncated
/// `INT_MIN` instead of halting -- which is the silently wrong answer.
#[test]
fn the_minimum_over_minus_one_halts_at_both_widths() {
    for (name, tag) in [
        ("divoverflow32.fss", "divoverflow32"),
        ("divoverflow64.fss", "divoverflow64"),
    ] {
        let binary = compile_fixture(name, tag);
        let out = run(&binary);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{name}:\n{stderr}");
        assert!(
            stderr.contains("integer division overflows"),
            "{name}: {stderr}"
        );
        let _ = std::fs::remove_file(&binary);
    }
}

/// The one divisor the run-time guard can never see: LLVM's constant folder
/// turns the division into `poison` while the module is being built, so the
/// program prints a value nothing computed.
#[test]
fn a_literal_zero_divisor_is_refused_before_llvm_can_fold_it() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("baddivzeroliteral.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(message.contains("literal zero divisor"), "{message}");
}

/// RR64 is not routed through the guard: `1.0/0.0` is `inf` and that is right.
#[test]
fn floating_division_by_zero_is_still_infinity() {
    let binary = compile_fixture("divquotients.fss", "divquotients");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "3\n-3\n3000000000\n0.25\ninf\n"
    );
    let _ = std::fs::remove_file(&binary);
}

/// `b.x = 7` in statement position is an equality test whose value is thrown
/// away, so the field is printed unchanged. `blocks.tex:49-63` invalidates the
/// program twice over; the parser cannot see it, because statement position
/// exists only in the checker.
#[test]
fn a_field_assignment_in_statement_position_is_refused_rather_than_discarded() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("badfieldassign.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(
        message.contains("equality test whose result is discarded"),
        "{message}"
    );
    // The advice must not send the reader to `:=`, which dead-ends on
    // InvalidAssignTarget and then on MutableFieldUnsupported.
    assert!(!message.contains(":="), "{message}");
}

/// 1.0 reads `f(x = 2)` as a keyword argument and reserves the parenthesised
/// form for the test; the parser erases parentheses, so the compiler cannot
/// tell them apart and used to pass a Boolean silently.
#[test]
fn a_bare_name_equals_argument_is_refused_instead_of_passing_a_boolean() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("badkeywordargument.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(message.contains("keyword argument"), "{message}");
}

/// No builtin has a named parameter, so `assert(count = 1000)` is unambiguous
/// -- and it is legal, working Fortress that a blanket guard would regress.
#[test]
fn an_equality_test_as_a_builtin_argument_is_still_legal() {
    let binary = compile_fixture("kwbuiltin.fss", "kwbuiltin");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\ntrue\n");
    let _ = std::fs::remove_file(&binary);
}

/// `widen` used to hardcode ZZ32 -> ZZ64 at both ends while the advice that
/// recommends it recognised three widenings, so `x: RR64 = widen(n)` repeated
/// the same message one type up and no expression reached an RR64 from an
/// integer at all.
#[test]
fn widen_reaches_every_widening_the_advice_recommends() {
    let binary = compile_fixture("widenrr64.fss", "widenrr64");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout), // ZZ32 -> RR64, ZZ32 -> ZZ64, ZZ64 -> RR64. The two RR64 results print
        // `3.0` now; only the integer one prints `3`. Same `%g` fix.
        "3.0\n3\n3.0\n"
    );
    let _ = std::fs::remove_file(&binary);

    let ir = emit_ir_with("widenrr64.fss", &[]);
    let ir = String::from_utf8_lossy(&ir.stdout);
    assert!(
        ir.contains("sitofp"),
        "an integer to RR64 widening is an sitofp:\n{ir}"
    );
    assert!(
        ir.contains("sext"),
        "an integer widening is still a sext:\n{ir}"
    );
}

/// A file with two complete components compiled at exit 0 and the second one
/// was gone. Only UNLEXABLE trailing text was caught, and the lexer caught that.
#[test]
fn nothing_may_follow_the_components_closing_end() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("badtrailingcomponent.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(
        message.contains("end of file after the component"),
        "{message}"
    );
}

/// `aggregate.tex:120-121`: `RectSeparator ::= ';'+ | Whitespace`. A
/// whitespace-separated run was swallowed as one juxtaposition, so `[1 2 3]`
/// was ONE element holding 6. 128 corpus sites write the juxtaposed spelling.
#[test]
fn a_juxtaposed_array_literal_has_one_element_per_operand() {
    let binary = compile_fixture("arrayjuxtaposed.fss", "arrayjuxtaposed");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n1\n3\n3\n4\n8\n");
    let _ = std::fs::remove_file(&binary);
}

/// `"a " x` with a trait- or object-typed `x` reached codegen as
/// `to_string_Shape` and came out as `fortressc: internal error`, exit 70 --
/// a compiler bug raised by ordinary source. `println` had the guard all along
/// and said why; `concatenation` did not.
#[test]
fn a_concatenation_with_no_conversion_is_a_diagnostic_and_not_an_internal_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("badconcat.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "70 is a compiler bug, not a diagnostic:\n{message}"
    );
    assert!(message.contains("has no conversion"), "{message}");
    // It must NOT name `println`: the fixture has none on that line.
    assert!(!message.contains("println` does not accept"), "{message}");
}

/// Diagnostics carry `line:col` and a source excerpt, not byte offsets.
#[test]
fn a_diagnostic_names_a_line_and_column_and_shows_the_source() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("badconcat.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(message.contains("badconcat.fss:11:40:"), "{message}");
    assert!(message.contains("11 | describe(s: Shape)"), "{message}");
    assert!(message.contains('^'), "{message}");
}

/// `where { ... }` was brace-matched and thrown away, so nothing inside it was
/// parsed at all and a bound written there was a silent no-op -- while the
/// identical bound in the bracket list was enforced.
#[test]
fn a_where_clause_is_parsed_rather_than_skipped() {
    for (name, phrase) in [
        ("badwhereclause.fss", "only constrain a static parameter"),
        ("badwherevariable.fss", "introduces fresh static variables"),
        ("badwherebound.fss", "does not satisfy `A extends Top`"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
            .arg(fixture(name))
            .arg("--emit-ir")
            .output()
            .expect("could not run fortressc");
        let message = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{name}:\n{message}");
        assert!(message.contains(phrase), "{name}: {message}");
    }
}

/// The one form v1 implements needs no machinery of its own: the constraint is
/// appended to the named static parameter's bounds, so `record_bounds` and
/// `discharge_bounds` enforce it exactly as they do a bracket-list bound.
#[test]
fn a_satisfied_where_bound_compiles_and_an_empty_clause_is_legal() {
    let binary = compile_fixture("whereclause.fss", "whereclause");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n7\n");
    let _ = std::fs::remove_file(&binary);
}

/// A generic declaration nothing instantiates is DELETED by expansion, so no
/// name in its header is ever resolved by anything. The non-generic sibling
/// `trait R extends Nowhere end` was refused all along.
#[test]
fn a_generic_declaration_header_resolves_its_type_names() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("badgenericheader.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(message.contains("unknown type `Nowhere`"), "{message}");
}

/// Names only. A generic header whose names all resolve still compiles, and its
/// BODY is still unchecked at an opaque parameter -- deliberately, because the
/// encoding that would check it cannot represent `T extends ZZ32`.
#[test]
fn a_generic_header_whose_names_resolve_still_compiles() {
    let binary = compile_fixture("genericheader.fss", "genericheader");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
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
/// Thirteen lines. `adder(100)` is a closure that OUTLIVES the call that made
/// it, carrying its capture in a scanned field; `nested(7)` is a lambda whose
/// body builds another one; and the five 42s are the shapes the corpus actually
/// writes -- an unwritten parameter type, a bare binder with no parentheses,
/// ZERO parameters, a closure factory taking its arrow from a declaration's
/// RETURN type, and a lambda taking it from a binding's annotation.
///
/// The census is what makes those five worth having: of 1064 `fn` uses, 540
/// carry no annotation at all, 169 have no parameter, and 154 write the bare
/// binder. Refusing them would have refused the majority shape.
#[test]
fn a_lambda_captures_its_enclosing_bindings_and_outlives_them() {
    let binary = compile_fixture("lambda.fss", "lambda");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "6\n18\n42\n15\n115\nhi-tagged\n105\n12\n42\n42\n42\n42\n42\n"
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
///
/// The `-1` is MAX's: the maximum of ten negative numbers, which a slot seeded
/// with a zero bit pattern reports as 0. That is why the identity is a fact
/// about the operator AND the type, computed in codegen, rather than the
/// allocator's memset.
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
            "55\n30\n55\n100\n120\n120\n10\n10\n1\n-1\n1\n499999500000\n",
            "at FORTRESS_WORKERS={workers}"
        );
        assert_eq!(out.status.code(), Some(0));
    }
    let _ = std::fs::remove_file(&binary);
}

/// A BIG reduction over a COLLECTION rather than a range. All four operators
/// are lowered when the generator is a range; iterating a collection needs the
/// generator PROTOCOL, which needs a name to cross a file boundary first.
/// Refused by name rather than read as a subscript, which is what it was.
#[test]
fn a_big_reduction_over_a_collection_is_refused_by_name() {
    let message = refusal("badbigmax.fss");
    assert!(message.contains("over a collection"), "{message}");
}

// --------------------------------------------------------- `also do`

/// `do A also do B end`, serialised -- a deviation with a licence rather than a
/// shortcut. `also.tex:17-21` makes each block an implicit thread of one group,
/// `parallelism.tex:88-90` permits an implementation to serialise any group of
/// implicit threads, and `also.tex:24-27` requires every block and the group to
/// have type `()`. With no value to combine, running them in order is a legal
/// schedule.
///
/// The parallel lowering was measured and rejected: a two-iteration `for` never
/// distributes (the runtime runs any range below 4096 inline), and the loop
/// rules would then refuse nearly every real site, because an `also` block
/// assigns enclosing locals non-atomically as a matter of routine.
///
/// The third and fourth groups are the `atomic` rule: it binds to a DoFront and
/// NOT to the group, because the grammar puts it inside
/// `DoFront ::= [at Expr] [atomic] do [BlockElems]`.
#[test]
fn an_also_group_runs_every_block_and_atomic_binds_one_front() {
    let binary = compile_fixture("alsodo.fss", "alsodo");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1\n2\n3210\n2\n2\n2\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// The block-type rule, and the legacy implementation agrees on the verdict:
/// `ProjectFortress/compiler_tests/Compiled10.a.fss` is this file, and
/// `XXX10a.test` expects "do-also expression has type IntLiteral, but it must
/// have () type". Ours names the block rather than the literal, which is what
/// the rule is about.
#[test]
fn a_block_of_an_also_group_must_have_type_void() {
    let message = refusal("badalsovalue.fss");
    assert!(
        message.contains("every block of one must have type ()"),
        "{message}"
    );
}

/// The `atomic` rule, tested WITHOUT depending on a schedule.
///
/// `DoFront ::= [at Expr] [atomic] do [BlockElems]` puts the modifier inside a
/// front, so `atomic do A also do B end` makes only A atomic. Serialised
/// execution cannot tell that reading from whole-group-atomic apart -- both
/// print the same thing -- so the distinguisher is a COMPILE-TIME one: an
/// `exit` crossing an `atomic` boundary is refused, and B is not inside one.
///
/// The first draft wrapped the whole group. It parsed, it ran, it printed the
/// right numbers, and it was a different program; this pair is what says which.
#[test]
fn atomic_binds_to_one_also_front_and_not_to_the_group() {
    let binary = compile_fixture("alsoatomicfront.fss", "alsoatomicfront");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);

    let message = refusal("badalsoatomicexit.fss");
    assert!(message.contains("leaves an `atomic` region"), "{message}");
}

// -------------------------------------------------- array generators

/// `for x <- a do ... end` over an ARRAY, desugared onto the indexed loop that
/// already exists: `for $k <- 0 # length(a) do x = a[$k]; body end`.
///
/// NOTHING FROM THE CLOSURE REPRESENTATION IS INVOLVED -- zero minted traits,
/// zero `apply` methods. The generator PROTOCOL is the part that needs closures,
/// and the census says it is blocked on imports rather than on them: of 238
/// bare-identifier `for` sources in the corpus, five resolve to an Array and 134
/// to List/Map/Set/Generator.
///
/// The first two lines are the two properties that could have gone wrong: the
/// reduction in the body is still recognised, so the sum is exact at every
/// worker count, and THE SOURCE IS EVALUATED ONCE -- `built` is 1, not 6.
#[test]
fn a_for_loop_over_an_array_iterates_it_and_evaluates_it_once() {
    let binary = compile_fixture("arraygenerator.fss", "arraygenerator");
    for workers in ["1", "8"] {
        let out = Command::new(&binary)
            .env("FORTRESS_WORKERS", workers)
            .output()
            .expect("could not run the produced binary");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "21\n1\n720\nhello\n",
            "at FORTRESS_WORKERS={workers}"
        );
        assert_eq!(out.status.code(), Some(0));
    }
    let _ = std::fs::remove_file(&binary);
}

/// And a source that is not an array is refused by name rather than read as a
/// range with a missing bound.
#[test]
fn a_for_loop_over_something_that_is_not_an_array_is_refused() {
    let message = refusal("badforinsource.fss");
    assert!(message.contains("expected an array"), "{message}");
}

/// A COMPONENT IS NOT RESPONSIBLE FOR AN API'S INTERNAL WELL-FORMEDNESS.
/// `comprises` rule three -- an api may not declare a trait extending one of
/// its own open-comprises traits -- must read only declarations THIS FILE
/// WROTE, not the ones resolution merged in.
///
/// THE MUTATION TABLE IS WHY THIS FIXTURE EXISTS. Making
/// `Library/FortressLibrary.fsi` parse in full took
/// `SpecData/examples/advanced/Overloading.fss` from compiling to refused, with
/// its caret on the component header, for `trait AnyIntegral extends { QQ }`
/// written in a file it merely imports. Scoping the rule fixed it -- and then
/// that file started failing on MAX_INSTANTIATIONS instead, so the corpus
/// witness disappeared and the mutation ESCAPED. This is its replacement.
#[test]
fn a_component_is_not_punished_for_an_imported_apis_comprises_defect() {
    let binary = compile_fixture("importsopencomprises.fss", "importsopencomprises");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
    let _ = std::fs::remove_file(&binary);
}

/// AND THE API ITSELF IS STILL REFUSED, which is what makes the test above a
/// scoping assertion rather than the rule being switched off.
#[test]
fn the_api_with_that_defect_is_still_refused_on_its_own() {
    let message = refusal("openapicomprises.fsi");
    assert!(
        message.contains("an api may not declare a trait that extends"),
        "{message}"
    );
}

// ------------------------------------------------------------- getters
//
// A GETTER IS A NULLARY DOTTED METHOD UNDERNEATH and a FIELD READ on the
// surface, and both halves have to be true at once. It used to be neither:
// `AccessorUnsupported` refused every `o.g`.

/// Read like a field, dispatched like a method -- through the trait, on the
/// RUN-TIME type.
#[test]
fn a_getter_is_read_like_a_field_and_dispatched_like_a_method() {
    let binary = compile_fixture("getterread.fss", "getterread");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "circle\n8\ncircle\nsquare\n"
    );
    let _ = std::fs::remove_file(&binary);
}

/// AND NEVER CALLED. Without this the nullary method underneath answers `o.g()`
/// too, and `Compiled6.y.fss` -- which writes `O.z` and `O.z()` on consecutive
/// lines and expects only the second to fail -- became a new must-fail
/// ACCEPTANCE. The gate caught it; the fixture is what keeps it caught.
#[test]
fn a_getter_may_not_be_called_with_parentheses() {
    let message = refusal("badgettercall.fss");
    assert!(message.contains("is READ as `.z`, not called"), "{message}");
}

/// `Getter/setter declarations should not be overloaded with method
/// declarations` (Compiled6.l.fss). THE COLLISION IS THE SETTER AGAINST THE
/// METHOD and not the getter: `getter x()` is nullary and `x(y)` takes one, so
/// those two merely overload. The first version of this fixture omitted the
/// setter, compiled, and asserted nothing.
#[test]
fn a_setter_and_a_method_of_the_same_shape_collide() {
    let message = refusal("badgetteroverload.fss");
    // The message names BOTH declarations now, and the argument types, because
    // with overloading the NAME is shared by design and one span was not enough
    // to find the pair. Asserting the types too keeps this test about the
    // collision rather than about the word "twice".
    assert!(
        message.contains("`x` is declared twice on the same argument types (O, ZZ32)"),
        "{message}"
    );
}

// -------------------------------------------------------- `asString`, and `%g`
//
// `FortressLibrary.fsi` declares `asString` as a getter on every numeric trait.
// The scalars are BUILTINS in this compiler and do not come from the library,
// so nothing would ever declare it for them -- and the shim it needs is the
// same `Target::ToString` `println` and concatenation have used since M1.

/// TEN of the sixteen accessor-blocked oracle cases were this spelling, and
/// every one arrived through `"..." || x.asString`.
///
/// THE RR64 LINES ARE THE POINT OF THE FIXTURE. `%g` prints `17` for `17.0`,
/// which `compiler_tests/Compiled7.Print17.fss` asserts is wrong -- and all
/// THREE shims that reach a double had it: `println`, `print` and `to_string`.
/// A value printed one way and concatenated another is the same defect one
/// step later, so the fixture takes the same RR64 through two of them.
#[test]
fn as_string_on_every_scalar_and_a_float_that_shows_its_point() {
    let binary = compile_fixture("asstring.fss", "asstring");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "3\n9\ntrue\nhi\n17.0\n2.5\n17.0\nr = 17.0\n"
    );
    let _ = std::fs::remove_file(&binary);
}

// --------------------------------------------- a named import brings what it named
//
// `ImportItems` was on the declaration and read by NOTHING, so
// `import FortressLibrary.{println, String}` merged every trait and object the
// library declares. That is what put `Indexed` into the instantiation budget of
// a component that never named it, and MAX_INSTANTIATIONS is what that
// component died on.

/// What was named, PLUS its supertypes -- a trait's supertype is part of its
/// identity and subtyping cannot be decided without it.
#[test]
fn a_named_import_brings_what_it_named_and_its_supertypes() {
    let binary = compile_fixture("namedimport.fss", "namedimport");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
    let _ = std::fs::remove_file(&binary);
}

/// AND NOTHING ELSE. Without this the whole api arrived and `Unwanted`
/// compiled; that is the defect, stated as a test.
#[test]
fn a_named_import_does_not_bring_what_it_did_not_name() {
    let message = refusal("badnamedimport.fss");
    assert!(message.contains("unknown type `Unwanted`"), "{message}");
}

/// ON DEMAND STILL BRINGS EVERYTHING. `intro.tex:38-63`, and 841 of the
/// corpus's 983 brace imports are this form -- narrowing it would be the real
/// regression, so the pair is the assertion and neither half alone is.
#[test]
fn an_on_demand_import_still_brings_the_whole_api() {
    let binary = compile_fixture("ondemandimport.fss", "ondemandimport");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
    let _ = std::fs::remove_file(&binary);
}

// ------------------------------------------ operator declarations (SPIKE-OPEXPR)
//
// THE BOOTSTRAP ROOT'S TWO WALLS. `Library/FortressLibrary.fsi` died at byte
// 44522 on a subscripted assignment and then at 79037 on a postfix declaration;
// with both of these it parses in full.

/// `subscripting.tex:44-54`. Every spacing the corpus writes, multiple indices,
/// an `_` index name and the `abstract` prefix -- and the GET and the SET
/// coexisting, which is what the `:=` in the member NAME is for.
#[test]
fn every_subscripted_assignment_shape_the_corpus_writes_parses() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("oprsubscriptdecl.fsi"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A postfix DECLARATION has no trailing parameter list.
#[test]
fn a_postfix_operator_declaration_parses() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("oprpostfixdecl.fsi"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// AND THE ROOT ITSELF, by path. This is the one assertion that says the
/// milestone happened: the file the whole bootstrap depends on is READ.
/// It does not CHECK, and the wall has MOVED TWICE, which is why this asserts
/// the parse and names the current wall rather than pinning a line number:
///   :406  the library's own open-`comprises` defect  -- patched in source
///   :758  `__cond[\E,R\]` vs `__cond[\E\]`, a GENUINE 1.0 uniformity
///         violation, the same class `Library/QuickSort.fsi` is refused for and
///         which DEV-6 records as enforced and permanent
/// `resolve()` needs an api to PARSE, not to check, so the phase-3 value does
/// not depend on either.
///
/// THE :758 WALL IS PAID and the wall behind it has a different owner. The
/// legacy library's own uniformity violation is exempted BY CONTENT -- DEV-15,
/// a pair of bodiless declarations -- see `mono::is_signature_only`.
///
/// AND :1117 IS PAID TOO. That was `MAX_INSTANTIATIONS`, and it was not a
/// budget that wanted raising: `trait Indexed[\E,I\]` at :1138 declares
/// `getter indexValuePairs(): Indexed[\(I,E),I\]`, which demands its own
/// declaration at a strictly larger type, forever. Measured rather than read --
/// a trace of the instantiation queue put 793 of the 4096 on that single
/// `Indexed -> Indexed` edge. Expansion now FILES such a member instead of
/// walking it, exactly as it already did for a generic method, so the chain
/// never starts. See `Component::cuts`.
///
/// THE WALL AT :654 HAS MOVED TWICE AND THIS TEST IS WHY EITHER MOVE WAS SEEN.
/// It was `unknown type Any` -- the root trait, unseeded, which is what merging
/// `semantics/phase2` answered. Merging it ALONE did not move this line: the
/// arrow-lifting pass kept a SECOND list of the names the compiler knows
/// without a declaration, and `Generator[\Any\]` at :1992 substitutes `Any`
/// into `filter`'s arrow here. One shared `BUILTIN_TYPE_NAMES` answered that.
///
/// THE WALL MOVED A THIRD TIME, to `RR64 is not a trait, so nothing can extend
/// it` at :376. `Char` is a real type now, so `trait String extends
/// ZeroIndexed[\Char\]` at :2286 resolves; :376 is `trait QQ extends { RR64,
/// StandardPartialOrder[\QQ\] }`, a trait extending a SCALAR. That is the
/// 102-file traits-objects class and it is a representation question, not a
/// plumbing one: a scalar carries no tag, so nothing below it can dispatch.
///
/// The line number goes DOWN because this checker is not order-sensitive and
/// reports the first error it finds, not the first one in the file.
#[test]
fn the_bootstrap_root_parses_in_full() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("Library/FortressLibrary.fsi"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        !message.contains("expected"),
        "FortressLibrary.fsi must not fail to PARSE: {message}"
    );
    assert!(
        !message.contains("4096 instantiations"),
        "the instantiation budget is no longer the blocker; \
         a growing member is filed, not walked: {message}"
    );
    assert!(
        !message.contains("unknown type `Any`") && !message.contains("unknown type `Object`"),
        "the root traits are seeded and reach every pass; neither may be the wall: {message}"
    );
    assert!(
        !message.contains("unknown type `Char`"),
        "the character type is built; it may not be the wall again: {message}"
    );
    assert!(
        !message.contains("is not a trait, so nothing can extend it"),
        "a trait extending a scalar was a DECLARED trait the resolver could not \
         reach; the shadowing fix retired this wall and it may not come back: \
         {message}"
    );
    assert!(
        !message.contains("a tuple type is not implemented"),
        "tuple TYPES resolve now; :1730 may not come back as the wall: {message}"
    );
    // REPINNED A SECOND TIME, DELIBERATELY, AND THE NEW WALL IS NOT A TUPLE
    // ONE. `Maybe[\(Reduction[\R\],Reduction[\R\])\]` at :1730 resolves,
    // and the file walks on to a collision in an IMPORTED api:
    // `Library/FlatString.fsi` declares `opr ||(self, b:FlatString)` and
    // `opr ||(a:FlatString, self)`, which are the same `(FlatString,
    // FlatString)` signature with `self` in the other operand position.
    //
    // Pinning it here is what stops the next reader assuming tuples are still
    // the blocker on this file. They are not.
    assert!(
        !message.contains("is declared twice on the same argument types"),
        "two BODILESS declarations of one signature are one declaration; that \
         wall may not come back: {message}"
    );
    // REPINNED A FIFTH TIME, AND THE FILE IS NOW PAST EVERY TOPOLOGICAL WALL.
    // The builtins are importable, so `RR32` resolves; the four `comprises`
    // contradictions with the builtin are CORRECTED IN THE LIBRARY SOURCE, as
    // are the eight Boolean operators it declared on top of the builtin's own
    // methods and the two `String` methods it declared twice. All fourteen are
    // marked `v1 SOURCE CORRECTION` at the site, and each was named by the
    // COMPILER rather than by reading -- one run per removal.
    //
    // WHAT IS LEFT IS NOT THE LIBRARY'S FAULT. `Library/String.fsi:43` writes
    // `var maxLeafSize: ZZ32`, which this parser cannot read yet -- the
    // `expected an expression, found KwVar` class, 58 first-blockers and the
    // largest single one in the corpus -- so the resolver skips that api as
    // unreadable and `StringStats` never arrives. That source is CORRECT
    // Fortress and is deliberately not "corrected".
    //
    // AND ONE MORE WALL IS MEASURED BEHIND IT, not guessed: neutralise that one
    // line and `String.fsi` checks clean (60 declarations) and this file walks
    // from :2423 to :878, `opr SQCAP(self, o: Maybe[\T\])` being ambiguous for
    // a pair of `Just` instantiations.
    assert!(
        !message.contains("unknown type `RR32`"),
        "the builtins are importable now; `RR32` may not come back as the wall: \
         {message}"
    );
    assert!(
        !message.contains("is listed in the `comprises` clause of"),
        "every topological contradiction is corrected in the source; none may \
         come back as the wall: {message}"
    );
    assert!(
        !message.contains("is declared twice on the same argument types"),
        "the fourteen duplicate declarations are corrected in the source: \
         {message}"
    );
    assert!(
        message.contains("unknown type `StringStats`"),
        "the remaining blocker should be an api this parser cannot read: \
         {message}"
    );
}

/// `(a, b) = (e1, e2)` BINDS, and this test asserts the VALUES because the
/// failure mode it guards is silent.
///
/// Without a binder node the parser falls through to an expression and this is
/// INFIX EQUALITY -- a discarded Boolean. `tupleTest1.fss` and `tupleTest2.fss`
/// have no asserts and no `.test`, so that reading compiles, exits 0, does
/// nothing, and counts as two files gained. Only the values can tell the two
/// apart, so exit codes are not enough here.
#[test]
fn a_tuple_binding_binds_its_names_and_does_not_compare_them() {
    let src = output_path("tuplebind").with_extension("fss");
    std::fs::write(
        &src,
        "component tuplebind\n         export Executable\n         run():()=do\n           (a,b) = (1,2)\n           println(a)\n           println(b)\n         end\n         end\n",
    )
    .expect("could not write fixture");
    let exe = output_path("tuplebind-out");
    let built = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("could not run fortressc");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&exe).output().expect("could not run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "1\n2\n");
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&exe);
}

/// THE ELEMENTS ARE CHECKED BEFORE ANY NAME IS DECLARED, so `(a2,b2) = (b,a)`
/// reads the OUTER bindings. Declaring as it went would make the second element
/// see the name this statement is introducing.
#[test]
fn a_tuple_binding_evaluates_its_elements_before_binding_any_name() {
    let src = output_path("tupleswap").with_extension("fss");
    std::fs::write(
        &src,
        "component tupleswap\n         export Executable\n         run():()=do\n           a = 1\n           b = 2\n           (a2,b2) = (b,a)\n           println(a2)\n           println(b2)\n         end\n         end\n",
    )
    .expect("could not write fixture");
    let exe = output_path("tupleswap-out");
    let built = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("could not run fortressc");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&exe).output().expect("could not run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "2\n1\n");
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&exe);
}

/// A REAL CORPUS PROGRAM, and it self-checks: `Compiled5.Binding.fss` computes
/// `fib 20` through two destructurings per recursion and prints 6765. A binder
/// that bound the wrong element, or bound nothing, would not print that.
#[test]
fn the_corpus_destructuring_program_computes_the_right_answer() {
    let exe = output_path("compiled5binding");
    let built = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus(
            "ProjectFortress/compiler_tests/Compiled5.Binding.fss",
        ))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("could not run fortressc");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&exe).output().expect("could not run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "6765\n");
    let _ = std::fs::remove_file(&exe);
}

/// A TUPLE IN STATEMENT POSITION IS ITS ELEMENTS, EVALUATED, and the corpus
/// file that wants it wants exactly that: `atomicExpr.fss:18` writes
/// `(atomic do x+=1; y+=1; end, atomic do z:=x+y end)` for the EFFECTS and
/// discards the value.
///
/// It self-checks, and it accepts BOTH orderings -- `z` may be 0 or 2 and it
/// refuses 1 -- so this pins that both elements run, not which ran first.
#[test]
fn a_tuple_in_statement_position_evaluates_both_elements() {
    let exe = output_path("atomicexpr");
    let built = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/tests/atomicExpr.fss"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("could not run fortressc");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&exe).output().expect("could not run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "PASS\n");
    let _ = std::fs::remove_file(&exe);
}

/// AND ONLY IN STATEMENT POSITION. A tuple whose value is USED needs a
/// representation and stays refused by name -- without this the test above
/// would pass just as well if tuples had quietly become values everywhere.
#[test]
fn a_tuple_whose_value_is_used_is_still_refused() {
    let src = output_path("tupleval").with_extension("fss");
    std::fs::write(
        &src,
        "component tupleval\n         export Executable\n         run():()=do\n           x = (1,2)\n           println(1)\n         end\n         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("tupleval-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("a tuple expression is not implemented in this subset"),
        "{message}"
    );
    let _ = std::fs::remove_file(&src);
}

/// NO OTHER SHADOWING IS PERMITTED, and this covers the three binders the
/// parameter rule did not: a local, a tuple binder and a LOOP binder over a
/// top-level value. `declarations.tex:476-533`.
///
/// MEASURED BEFORE IT WAS WRITTEN: swept over all 1956 corpus files, 449
/// compiling either way, zero gained and zero lost. It refuses only programs
/// nothing writes -- which is why each of the three is asserted here rather
/// than trusted to the sweep.
#[test]
fn no_binder_may_shadow_a_top_level_value() {
    for (tag, body) in [
        ("local", "  v = 2\n  println(v)\n"),
        ("tuplebinder", "  (v,w) = (2,3)\n  println(v)\n"),
        ("loopbinder", "  for v <- 0#3 do println(v) end\n"),
    ] {
        let src = output_path(&format!("shadow-{tag}")).with_extension("fss");
        std::fs::write(
            &src,
            format!(
                "component shadow{tag}\n\
                 export Executable\n\
                 v = 1\n\
                 run():()=do\n{body}end\n\
                 end\n"
            ),
        )
        .expect("could not write fixture");
        let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
            .arg(&src)
            .arg("-o")
            .arg(output_path(&format!("shadow-{tag}-out")))
            .output()
            .expect("could not run fortressc");
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(
            message.contains("`v` is already declared at the top level"),
            "{tag}: {message}"
        );
        let _ = std::fs::remove_file(&src);
    }
}

/// AND A NAME NOTHING DECLARES AT THE TOP LEVEL IS STILL FREE. Without this
/// the rule above would pass just as well if every binder were refused.
#[test]
fn a_binder_that_shadows_nothing_is_fine() {
    let src = output_path("shadow-free").with_extension("fss");
    std::fs::write(
        &src,
        "component shadowfree\n\
         export Executable\n\
         v = 1\n\
         run():()=do\n\
           w = 2\n\
           for k <- 0#2 do println(k) end\n\
           println(w)\n\
         end\n\
         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("shadow-free-out"))
        .output()
        .expect("could not run fortressc");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_file(&src);
}

/// `coerce(x: T)` PARSES. Fifteen of them were the ONLY parse blocker in
/// `ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi`, which is where the
/// standard library's `RR32`, `IntLiteral` and `FloatLiteral` live.
///
/// A VARIANT OF ITS OWN, NOT A METHOD NAMED `coerce`: parsed as a method it
/// would join an overload set and could win a dispatch, which is a silent
/// wrong answer in a feature that has no semantics yet.
#[test]
fn a_coercion_declaration_parses() {
    let src = output_path("coerceok").with_extension("fss");
    std::fs::write(
        &src,
        "component coerceok\n\
         export Executable\n\
         trait B end\n\
         trait C\n\
         coerce(x: B)\n\
         end\n\
         run(): () = ()\n\
         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("coerceok-out"))
        .output()
        .expect("could not run fortressc");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_file(&src);
}

/// A COERCION IS AN EDGE IN THE TYPE HIERARCHY, and this is what recording it
/// without reading it got wrong. `Compiled6.p.fss` writes `trait A extends B`
/// with `coerce(x:B)` inside it: one edge from `extends`, one from the
/// coercion, and 1.0 refuses it with "Cyclic type hierarchy: Type B
/// transitively extends/coerces to itself".
///
/// LANDING `coerce` ACCEPTED THAT PROGRAM and the must-fail ratchet is what
/// said so -- pass fell below its floor and a new acceptance appeared in the
/// same run. Nothing else in the suite could see it.
#[test]
fn a_coercion_closing_a_cycle_is_refused() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/compiler_tests/Compiled6.p.fss"))
        .arg("-o")
        .arg(output_path("c6p-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("extends itself, directly or through another trait"),
        "{message}"
    );
}

/// AND THE COERCION EDGE MUST NOT REACH SUBTYPING. A coercion says a `B` is
/// CONVERTIBLE to an `A`, not that it IS one -- putting it in the supertrait
/// closure would make `is_subtype` answer yes and every dispatch built on that
/// wrong. Here `C` coerces from `B`, so a `B` may not be passed where a `C` is
/// required.
#[test]
fn a_coercion_is_not_a_subtyping_edge() {
    let src = output_path("coercesub").with_extension("fss");
    std::fs::write(
        &src,
        "component coercesub\n\
         export Executable\n\
         trait B end\n\
         object Bee extends B end\n\
         trait C\n\
         coerce(x: B)\n\
         end\n\
         f(c: C): () = ()\n\
         run(): () = f(Bee)\n\
         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("coercesub-out"))
        .output()
        .expect("could not run fortressc");
    assert!(
        !out.status.success(),
        "a coercion is not subtyping: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_file(&src);
}

/// AND IT DOES NOT SHADOW A REAL METHOD OF THE SAME NAME./// AND IT DOES NOT SHADOW A REAL METHOD OF THE SAME NAME. A coercion is not in
/// the dotted namespace at all, so declaring one beside a method called
/// `coerce` must not collide -- which is exactly what parsing it as a method
/// would have caused.
#[test]
fn a_coercion_does_not_collide_with_a_method() {
    let src = output_path("coercemix").with_extension("fss");
    std::fs::write(
        &src,
        "component coercemix\n\
         export Executable\n\
         trait B end\n\
         object O extends B\n\
         coerce(x: B)\n\
         end\n\
         run(): () = ()\n\
         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("coercemix-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        !message.contains("declared twice") && !message.contains("no winner"),
        "{message}"
    );
    let _ = std::fs::remove_file(&src);
}

/// A MUTABLE TOP-LEVEL VALUE MUST WRITE ITS TYPE, and the GRAMMAR is the
/// authority: `variables.tex:22-27` gives the untyped form as
/// `VarImmutableMods? BindIdOrBindIdTuple = Expr` -- immutable modifiers and
/// `=` only -- while `:=` appears solely in the alternatives carrying a
/// `: Type`. 1.0 answers `Compiled5.k.fss` with "The type of x is required".
///
/// CAUGHT BY THE MUST-FAIL RATCHET, not by a test: landing component-level
/// values made this program compile, and the oracle gate went red on a NEW
/// acceptance.
#[test]
fn a_mutable_top_level_value_must_write_its_type() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/compiler_tests/Compiled5.k.fss"))
        .arg("-o")
        .arg(output_path("c5k-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(message.contains("the type of `x` is required"), "{message}");
}

/// AND A PARAMETER MAY NOT SHADOW A TOP-LEVEL VALUE.
/// `declarations.tex:476-533` lists every shadowing a Fortress program may
/// contain -- a field or dotted method, a KEYWORD parameter, `self`, `result`
/// -- and closes with "No other shadowing is permitted in a Fortress program".
/// An ordinary parameter is not on that list. `Compiled1.x.fss` writes `v = 1`
/// and then `f(v: ZZ32) = v`, and 1.0 answers "Variable v is already
/// declared". Caught by the same ratchet.
#[test]
fn a_parameter_may_not_shadow_a_top_level_value() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/compiler_tests/Compiled1.x.fss"))
        .arg("-o")
        .arg(output_path("c1x-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("`v` is already declared at the top level"),
        "{message}"
    );
}

/// A TUPLE-DOMAIN ARROW IS N PARAMETERS, verified BY VALUE.
/// `basic/overloading.tex:125` -- a functional has a single parameter WHICH MAY
/// BE A TUPLE -- so `(A,B) -> C` mints an `apply(x: A, y: B): C` and needs no
/// tuple value at all.
#[test]
fn a_tuple_domain_arrow_forwards_every_argument() {
    let src = output_path("arrowtuple").with_extension("fss");
    std::fs::write(
        &src,
        "component arrowtuple\n\
         export Executable\n\
         add(p: ZZ32, q: ZZ32): ZZ32 = p + q\n\
         comb(f: (ZZ32,ZZ32) -> ZZ32, x: ZZ32): ZZ32 = f(x,x)\n\
         run():()= println(comb(add, 3))\n\
         end\n",
    )
    .expect("could not write fixture");
    let exe = output_path("arrowtuple-out");
    let built = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("could not run fortressc");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&exe).output().expect("could not run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "6\n");
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&exe);
}

/// A NAMED FUNCTION PASSED TO A METHOD'S ARROW PARAMETER, which the lift could
/// not see: `arrow_parameters` consulted top-level functions alone, so a dotted
/// call left its arrow arguments unlifted and the checker said `unknown name`.
/// `Compiled17d.fss` is the corpus program, and it self-checks -- it prints the
/// concatenation `34`.
#[test]
fn a_method_arrow_parameter_lifts_a_named_function() {
    let exe = output_path("compiled17d");
    let built = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/compiler_tests/Compiled17d.fss"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("could not run fortressc");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&exe).output().expect("could not run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "34\n");
    let _ = std::fs::remove_file(&exe);
}

/// AND `XXXArrowType.fss` STAYS REFUSED. It is an XXX must-fail and it is the
/// first casualty of widening the codomain: `f: ZZ32 -> () -> ()` becomes
/// liftable the moment a `()` codomain does. That widening was tried, gained
/// ZERO files, and was reverted.
#[test]
fn a_curried_arrow_field_stays_refused() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/parser_tests/XXXArrowType.fss"))
        .arg("-o")
        .arg(output_path("xxxarrow-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("an arrow type is not implemented in this subset"),
        "{message}"
    );
}

/// THE SELF-POSITION PAIR IS ONE DECLARATION, and this is what unblocks the
/// standard library. `Library/FlatString.fsi` writes both
/// `opr ||(self, b:F)` and `opr ||(a:F, self)`; they differ only in which
/// operand is the receiver and are NOT distinguishable by type.
///
/// A SELF-POSITION RULE WOULD BE WRONG. `traits.tex:484-494` says a functional
/// method has `self` at an ARBITRARY position and that such declarations "can
/// be viewed as top-level function declarations"; the spec's own example
/// (`SpecData/examples/basic/Trait.Method.a.fss`) puts `f(self, t:T)` beside
/// `f(s:S, self)` and rewrites them as `f1(a:A, t:T)` and `f2(s:S, a:A)` --
/// distinct only because `T` and `S` are, and that file declares
/// `trait T excludes A`. Two declarations that flatten to ONE type vector are
/// one declaration.
///
/// What is true is narrower: the refusal exists because dispatch cannot choose
/// between two IMPLEMENTATIONS. With no bodies there is nothing to choose.
#[test]
fn two_bodiless_declarations_of_one_signature_are_one_declaration() {
    let src = output_path("selfpos").with_extension("fsi");
    std::fs::write(
        &src,
        "api selfpos\n\
         trait S end\n\
         object F extends { S }\n\
         opr ||(self, b:F): S\n\
         opr ||(a:F, self): S\n\
         end\n\
         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("selfpos-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && !message.contains("declared twice"),
        "{message}"
    );
    let _ = std::fs::remove_file(&src);
}

/// THE OTHER SIDE, and without it the rule reads as "self position makes them
/// different", which is exactly what it is not. Give either declaration a BODY
/// and the ambiguity is real again -- two implementations, one signature, no
/// way to choose -- so the refusal comes back.
#[test]
fn one_signature_with_two_bodies_still_collides_whatever_the_self_position() {
    let src = output_path("selfpos2").with_extension("fss");
    std::fs::write(
        &src,
        "component selfpos2\n\
         export Executable\n\
         trait S end\n\
         object F extends { S }\n\
         opr ||(self, b:F): S = self\n\
         opr ||(a:F, self): S = self\n\
         end\n\
         run(): () = ()\n\
         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("selfpos2-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("`||` is declared twice on the same argument types (F, F)"),
        "{message}"
    );
    let _ = std::fs::remove_file(&src);
}

/// AND A DISAGREEMENT ON THE RESULT IS STILL AN ERROR even with no bodies: two
/// declarations asserting DIFFERENT results for one signature do not say the
/// same thing, so they are not one declaration.
#[test]
fn two_bodiless_declarations_that_disagree_on_the_result_still_collide() {
    let src = output_path("selfpos3").with_extension("fsi");
    std::fs::write(
        &src,
        "api selfpos3\n\
         trait S end\n\
         object F extends { S }\n\
         opr ||(self, b:F): S\n\
         opr ||(a:F, self): F\n\
         end\n\
         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("selfpos3-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("`||` is declared twice on the same argument types (F, F)"),
        "{message}"
    );
    let _ = std::fs::remove_file(&src);
}

/// A DECLARED NAME WINS, and this is the fix's own subject. `trait RR64` is an
/// ordinary library trait in 1.0 -- `conversions-coercions.tex:850-866` writes
/// one and `Library/FortressLibrary.fsi:335` declares one -- so a component
/// that writes the declaration means it.
#[test]
fn a_declared_trait_shadows_the_builtin_scalar_of_the_same_name() {
    let src = output_path("shadow-yes").with_extension("fss");
    std::fs::write(
        &src,
        "component shadowyes\n         trait RR64 end\n         trait QQ extends { RR64 } end\n         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("shadow-yes-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        !message.contains("is not a trait"),
        "a DECLARED `trait RR64` must be reachable in supertype position: {message}"
    );
    let _ = std::fs::remove_file(&src);
}

/// THE OTHER SIDE OF THE ORDER, and without it the fix could have deleted the
/// rule outright while the test above still passed. No declaration, so `RR64`
/// is the builtin scalar and a scalar carries no tag for anything below it to
/// dispatch on.
#[test]
fn an_undeclared_scalar_may_still_not_be_extended() {
    let src = output_path("shadow-no").with_extension("fss");
    std::fs::write(
        &src,
        "component shadowno\n         trait QQ extends { RR64 } end\n         end\n",
    )
    .expect("could not write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(&src)
        .arg("-o")
        .arg(output_path("shadow-no-out"))
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("`RR64` is not a trait, so nothing can extend it"),
        "an UNDECLARED scalar keeps the refusal: {message}"
    );
    let _ = std::fs::remove_file(&src);
}

/// AND THE OBJECT SIDE, which the corpus already carries as a must-FAIL.
/// `XXXextendBoolean.fss` writes `object Mumble() extends { Boolean }` with NO
/// declaration of `Boolean`, and the XXX convention says 1.0 refuses it.
///
/// THE FIRST CUT OF THIS FIX ACCEPTED IT -- accept any scalar in supertype
/// position -- and that is how we learned acceptance was the wrong shape: swept
/// over all 1956 corpus files its entire measured gain was this ONE file, and
/// this one file is a program that must not compile.
#[test]
fn an_object_may_not_extend_an_undeclared_scalar() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/tests/XXXextendBoolean.fss"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("`Boolean` is not a trait, so nothing can extend it"),
        "XXXextendBoolean is a must-FAIL and stays refused: {message}"
    );
}

/// A SHADOWING COMPONENT MAY NOT ALSO REACH THE BUILTIN, and saying so with a
/// test is what stops the first hole in `resolve_name`'s note from being a
/// claim. `Compiled6.u.fss` declares `trait Boolean` and its methods take
/// `Boolean` parameters -- which now means the TRAIT, so the builtin operator
/// `NOT` no longer applies to them. That is a DIAGNOSTIC.
///
/// It used to be exit 70. The accept-any-scalar cut matched a supertype by
/// NAME, so an object under the user's `trait Boolean` also satisfied the
/// BUILTIN `Type::Boolean`, and codegen emitted `ret ptr %trueTest` into an
/// `i1` return -- malformed IR, not a wrong answer anyone would see as one.
#[test]
fn a_component_shadowing_boolean_gets_a_diagnostic_and_not_malformed_ir() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/compiler_tests/Compiled6.u.fss"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a shadowed builtin is a user diagnostic, exit 1 and never 70: {message}"
    );
    assert!(
        !message.contains("does not match operand type"),
        "malformed IR reaching the verifier is the failure this pins: {message}"
    );
}

/// THE CUT IS WHAT MOVED IT, and without this the test above passes whether the
/// cut is doing anything or not -- the file could have reached :654 for some
/// unrelated reason and nothing would say so. A minimal growing trait, and the
/// grown instantiation must be ABSENT from what expansion emits.
///
/// The failure mode this pins is not a wrong answer, it is an OOM: each round
/// DOUBLES the mangled spelling, so `growA`'s two-parameter shape is killed by
/// the allocator (exit 137) long before it reaches 4096 stamps.
#[test]
fn a_member_that_demands_its_owner_larger_is_filed_rather_than_walked() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("growingmember.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let ir = String::from_utf8_lossy(&out.stdout);
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "an ordinary member of a growing trait must still compile:\n{message}"
    );
    assert!(
        !ir.contains("$tuple$"),
        "no instantiation at a grown argument may be emitted:\n{ir}"
    );
}

/// And CALLING the cut member names the mechanism. `has no field` was what it
/// said before the cut list was carried, and that names the wrong thing on a
/// member the source plainly declares.
#[test]
fn calling_a_filed_growing_member_names_the_mechanism() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("growingmembercall.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "the call must refuse:\n{message}"
    );
    assert!(
        message.contains("properly contain its own") && message.contains("`pairs`"),
        "the diagnostic must name the member and the mechanism:\n{message}"
    );
    assert!(
        !message.contains("has no field"),
        "an absence is not the diagnostic:\n{message}"
    );
}

/// A BOUND WAS CHARGED TO THE WRONG MEMBER OF AN OVERLOAD SET, and it refused
/// legal, uniformity-conformant code. Expansion instantiates EVERY member of a
/// set at the same static arguments -- it has no types and cannot know which
/// member a call meant -- and every member's bound was a HARD obligation. So
/// `f[\R\](R)` against `f[\T extends Red\](x:T)` and
/// `f[\T extends Blue\](x:T,y:ZZ32)` was charged BLUE's bound and refused.
///
/// A member whose bound does not hold at these arguments is not a member the
/// call can have meant, so it is pruned -- the same answer `prune_stamp` gives
/// an over-approximated method stamp. THE PRUNE IS KEYED BY (MANGLED NAME,
/// SOURCE SPAN) AND BOTH HALVES ARE LOAD BEARING: the span alone identifies the
/// source declaration, which every instantiation shares, so pruning Blue
/// because it fails at `[\R\]` also pruned it at `[\B\]` where it is the
/// only valid target. This fixture calls BOTH, which is what caught that.
#[test]
fn a_bound_is_charged_to_its_own_member_of_an_overload_set() {
    let exe = std::env::temp_dir().join("fortress_overloadbound");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("overloadbound.fss"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("could not run fortressc");
    assert_eq!(
        out.status.code(),
        Some(0),
        "must compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ran = Command::new(&exe)
        .output()
        .expect("could not run the binary");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let _ = std::fs::remove_file(&exe);
    assert_eq!(stdout, "1\n2\n", "each call must reach its own member");
}

/// AND THE PRUNE ITSELF IS LOAD BEARING, which the test above does NOT reach --
/// the mutation table said so by SURVIVING when the prune was replaced with a
/// no-op. Not erasing the obligation is what fixes the wrong refusal; PRUNING
/// is what stops the member it belonged to from being dispatched to anyway.
/// Without it `f[\R\](R, 0)` compiles and prints 2, calling the member whose
/// bound `R` does not satisfy. A silent wrong answer.
#[test]
fn a_member_whose_bound_failed_is_not_a_dispatch_target() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("overloadboundprune.fss"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "the pruned member must not be callable:\n{message}"
    );
}

/// AND WHEN NO MEMBER'S BOUND HOLDS, THE CLEAN DIAGNOSTIC SURVIVES. Pruning
/// them all turns `Green does not satisfy T extends Red` into a dispatch
/// failure reading `takes 1 argument(s), found 1`, which is nonsense. A member
/// is pruned only when a SIBLING survives.
#[test]
fn every_member_failing_its_bound_is_still_a_bound_diagnostic() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("overloadboundnone.fss"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "must refuse:\n{message}");
    assert!(
        message.contains("does not satisfy"),
        "the bound must still be named:\n{message}"
    );
    assert!(
        !message.contains("argument(s), found"),
        "an arity message is not the diagnostic:\n{message}"
    );
}

/// A TRAIT OR OBJECT OVERLOAD SET WAS UNIFORMITY-CHECKED BY NOTHING.
/// `check_uniformity` walked `Decl::Function` alone, so `trait Holder[\T\]`
/// beside `trait Holder` compiled to EXIT 0 -- and expansion then met a set
/// whose members disagree on how many static arguments they take, which is the
/// one thing `expand_types` states it may assume.
///
/// Retroactive cost measured before landing, not asserted: 1956 corpus files
/// swept, 397 compiling either way, 0 gained, 0 lost, 0 IR bodies changed. No
/// corpus file writes such a set, which is why these two fixtures exist.
#[test]
fn a_trait_overload_set_is_uniformity_checked() {
    for name in ["traituniformity.fss", "objectuniformity.fss"] {
        let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
            .arg(fixture(name))
            .arg("--emit-obj")
            .arg("-o")
            .arg("/dev/null")
            .output()
            .expect("could not run fortressc");
        let message = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{name} must refuse:\n{message}");
        assert!(
            message.contains("differ in their static parameters"),
            "{name} must refuse by name:\n{message}"
        );
    }
}

/// A GETTER WHOSE RETURN TYPE IS OMITTED RETURNED NOTHING, SILENTLY. Not a
/// refusal and not a crash -- `getter label() = "1"` compiled, linked, ran and
/// printed an empty line at exit 0, which is the worst class this project
/// recognises. `inferred_bodies` skipped every accessor, so the fixpoint never
/// reached one and it kept its `Void` placeholder.
///
/// The filter's own comment claimed it mirrored `run`'s three loops exactly.
/// It had stopped: `run` lifts accessors and says so at the `ACCESSORS ARE
/// LIFTED HERE TOO` comment, and this side still skipped them.
///
/// Found by a full-driver sweep after the growing-member cut, NOT by the
/// compile count -- the count read 397 either way. `ProjectFortress/tests/
/// nestedInst.fss` is the corpus witness and its IR is the ONLY one of 397 that
/// moved.
#[test]
fn a_getter_with_an_omitted_return_type_returns_its_body() {
    let exe = std::env::temp_dir().join("fortress_inferredgetter");
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("inferredgetter.fss"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("could not run fortressc");
    assert_eq!(
        out.status.code(),
        Some(0),
        "must compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ran = Command::new(&exe)
        .output()
        .expect("could not run the binary");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let _ = std::fs::remove_file(&exe);
    assert_eq!(
        stdout, "1\n5\n",
        "a getter must return its body, not the Void placeholder"
    );
}

/// THE PRE-EXISTING DEFECT THE CUT EXPOSED, and it is not the cut's: a generic
/// method whose arrow parameter mentions its OWNER's static parameter was
/// refused as `unknown type E` on master, with no cut involved. It reached no
/// diagnostic before only because no file got far enough to be checked.
#[test]
fn a_generic_methods_arrow_may_name_its_owners_static_parameter() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("arrowstaticparam.fss"))
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "`body: E->R` inside `shove[\\R\\]` on `Gen[\\E\\]` is well formed:\n{message}"
    );
}

/// AND :758 IS PAID BY CONTENT, NOT BY PATH. DEV-14 suspended the uniformity
/// rule for anything under a `Library` directory and is RETIRED: DEV-15 pays
/// for `__cond[\E,R\]` beside `__cond[\E\]` because both are bodiless, and
/// it pays for them wherever the file sits.
///
/// THE ASSERTION IS ON THE LIBRARY ROOT AND ON A COPY OF IT SOMEWHERE ELSE,
/// because either alone passes with the scope wrong: the root alone passes if
/// a path exemption is still hiding somewhere, and `copiedcond.fsi` alone --
/// see `a_bodiless_overload_set_may_differ_in_its_static_parameters` -- does
/// not say the real file moved.
#[test]
fn the_bootstrap_root_pays_758_by_content_and_not_by_path() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("Library/FortressLibrary.fsi"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        !message.contains("differ in their static parameters"),
        ":758 must be paid by DEV-15, with no path exemption left: {message}"
    );
}

/// DEV-15, AUTHORIZED 2026-08-22. `copiedcond.fsi` is
/// `FortressLibrary.fsi:757-758`'s shape copied verbatim into a file OUTSIDE
/// `Library/`, and it is now ACCEPTED -- not because of where it sits but
/// because both declarations are bodiless. This test asserted the opposite
/// until the deviation landed, and it is kept rather than deleted because it is
/// the one that says the relaxation is CONTENT-based: a path exemption alone
/// would still refuse this file.
#[test]
fn a_bodiless_overload_set_may_differ_in_its_static_parameters() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("copiedcond.fsi"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    assert_eq!(
        out.status.code(),
        Some(0),
        "two bodiless `__cond` declarations are DEV-15's whole subject:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// AND THE OTHER SIDE OF IT, which is what keeps DEV-15 from being "the rule is
/// gone". A BODY is what takes the exemption away: `size(x: ZZ32): ZZ32 = 2`
/// beside `size[\T\](x: T): ZZ32` is refused, in an api, where the bodiless
/// pair one line up is accepted. Without this the deviation would be
/// indistinguishable from a blanket relaxation -- which was measured and costs
/// a must-fail acceptance, `ProjectFortress/compiler_tests/Compiled6.ak.fss`.
///
/// BOTH ORDERS, and the second file exists because the mutation table found
/// that one of them could not see the difference: a relaxation that asks only
/// about the declaration IN HAND rather than about the pair meets the bodied
/// declaration second in one order and first in the other.
///
/// THE ASSERTION IS ON THE MESSAGE AND THAT IS THE POINT. A body inside an api
/// is refused by a SECOND rule -- "an `api` is a set of declarations" -- either
/// way, so `exit 1` alone passes on both readings and says nothing about
/// whether the pair was ever compared.
#[test]
fn a_declaration_with_a_body_is_not_exempt_from_uniformity() {
    for name in ["mixedoverload.fsi", "mixedoverloadrev.fsi"] {
        let message = refusal(name);
        assert!(
            message.contains("differ in their static parameters"),
            "{name}: {message}"
        );
    }
}

/// AND A TRAIT IS NEVER A SIGNATURE. It writes no body because it cannot, not
/// because it is a promise somebody else keeps -- and its name is written in
/// TYPE position, which is demand expansion has to serve. `trait Holder[\T\]`
/// beside `trait Holder` stays refused INSIDE AN API, where every function
/// declaration around it is exempt.
#[test]
fn a_trait_in_an_api_is_not_exempt_from_uniformity() {
    let message = refusal("bodilesstrait.fsi");
    assert!(
        message.contains("differ in their static parameters"),
        "{message}"
    );
}

/// THE MUST-FAIL THE BLANKET RELAXATION WOULD HAVE COST, asserted directly.
/// `Compiled6.ak.fss` writes `f(x: ZZ32) = ()` beside `f[\T extends Any\](x:
/// T) = ()` -- two BODIES -- and its `.test` expects two errors. It is the
/// single file that separates DEV-15 from the general version, so it is named
/// here rather than left to the oracle ratchet alone.
#[test]
fn dev15_does_not_accept_the_bodied_must_fail() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/compiler_tests/Compiled6.ak.fss"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(
        message.contains("differ in their static parameters"),
        "{message}"
    );
}

/// DIMENSIONS AND UNITS, sub-phase 4d, rung one. Declarations parse, register
/// and are CHECKED; nothing above that is built and every part of it is
/// refused by name.
#[test]
fn dimension_and_unit_declarations_run() {
    let binary = compile_fixture("dimensions.fss", "dimensions");
    let out = run(&binary);
    // BOTH FEATURES IN ONE FILE, which is the assertion that answers "do
    // dimensions break array types": `dim Area = Length^2` and `a: ZZ32[5]`
    // share one suffix production, classified by the position it was written
    // in. The first three lines are the array half.
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "4\n2.5\n7\ndimensions declared\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// A DERIVATION OVER AN UNDECLARED NAME IS A CLAIM ABOUT A DIMENSION THAT DOES
/// NOT EXIST -- the same shape as an unresolved `comprises` name, which this
/// compiler already refuses. It refuses the shipped 1.0 library too, and that
/// is the rule working: `Fortress.SIUnits.fsi` writes
/// `dim ElectricPotential = Power / Current` with no `Current` declared (the
/// dimension is `ElectricCurrent`) and `dim AngularVelocity = Angle / Second`,
/// where `Second` is a UNIT of `Time`.
#[test]
fn an_undeclared_dimension_name_is_refused() {
    let message = refusal("baddimname.fss");
    assert!(
        message.contains("`Length` is not a declared dimension"),
        "{message}"
    );
}

/// THE SINGLE GATE. A dimension is not a type: `dimensions.tex:237-253` gives
/// a dimensioned value a representation this backend has no boxing for, and
/// `dimensions.tex:206-215` makes a unit mismatch a static error nothing here
/// can decide. Without this arm the name would report `unknown type`, sending
/// the reader to look for a declaration that IS there.
#[test]
fn a_dimension_used_as_a_type_says_which_it_is() {
    let message = refusal("baddimtype.fss");
    assert!(message.contains("is a dimension, not a type"), "{message}");
}

/// A LIVE WRONG ANSWER, FIXED. `in` was an ordinary identifier, so
/// `println(x in nm)` over three `RR64` bindings was a three-way juxtaposition
/// PRODUCT: it compiled, linked and printed `7.8`, at exit 0, with no
/// diagnostic. Seven unit operators are reserved now, and the retroactive cost
/// was measured before the reclassification -- ZERO of the 394 files that
/// compiled used any of them as a name.
#[test]
fn a_unit_operator_is_no_longer_an_identifier() {
    let message = refusal("badunitop.fss");
    assert!(message.contains("reserved word `in`"), "{message}");
}

#[test]
fn a_dimension_name_is_declared_once_and_in_one_namespace() {
    let message = refusal("baddimdup.fss");
    assert!(message.contains("is declared twice"), "{message}");
    let message = refusal("baddimcollide.fss");
    assert!(message.contains("separate namespaces"), "{message}");
}

/// A `unit`/`dim` STATIC PARAMETER PARSES AND CANNOT BE INSTANTIATED, and the
/// refusal is at the INSTANTIATION rather than at the declaration on purpose:
/// `ProjectFortress/tests/dimensionUnitDecl.fss` declares
/// `trait Float1[\unit U absorbs unit, nat e, nat s\]` and never instantiates
/// it, so refusing the declaration would cost a file for a capability nothing
/// in the corpus asks for.
#[test]
fn a_unit_static_parameter_cannot_be_instantiated() {
    let message = refusal("baddiminstance.fss");
    assert!(
        message.contains("instantiating one is not implemented"),
        "{message}"
    );
}

/// ARRAY TYPES. `traits.tex:97-108`, the one-dimensional bracket form.
///
/// `ZZ32[5]` IS `Array[\ZZ32\]` WITH A SIZE THE CHECKER CAN COMPARE, and the
/// fixture asserts both halves at once: it runs, and it runs at a binding, at
/// a parameter and behind a `nat` static parameter, because the extent has to
/// survive monomorphization's substitution to reach the last one.
#[test]
fn an_array_type_runs_at_every_position() {
    let binary = compile_fixture("arraytype.fss", "arraytype");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "4\n2.5\n9\n4\n5\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// THE SIZE IS NOT DECORATION. `ProjectFortress/not_passing_yet/XXXwrongArrayDim.fss`
/// is a must-FAIL filed from a user bug report and it is entirely about this
/// shape. Without the check, `length(a)` answers 6 for a declared `ZZ32[5]`
/// and `a[5]` passes the bounds check, because the runtime header is built
/// from the literal's own length.
#[test]
fn a_declared_extent_must_match_the_literal_that_fills_it() {
    let message = refusal("badarrayextent.fss");
    assert!(
        message.contains("declared with 5 element(s) and 6 are written"),
        "{message}"
    );
}

/// RANK TWO AND THREE, END TO END: a non-square fill through a nest of loops,
/// the compound form at rank three, and a rank-one array in the same program on
/// the code path it always had.
///
/// NOTHING SQUARE, deliberately. A wrong stride in the linearisation collides
/// two subscripts onto one slot, and a 3 by 3 hides that -- the mutation that
/// multiplies by `extents[0]` instead of `extents[d]` prints `0 1 10 10 11 12`
/// here and would print the right answer on a square one.
#[test]
fn a_multi_dimensional_array_is_filled_and_read_back() {
    let binary = compile_fixture("arraymulti.fss", "arraymulti");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "0\n1\n2\n10\n11\n12\n123\n7\n7\n"
    );
    let _ = std::fs::remove_file(&binary);
}

/// THE MATRIX AGGREGATE, and the assertion is a VALUE because only a value can
/// see a transposed linearisation. `aggregate.tex:143-150` is the oracle: for
/// `A: ZZ32[3,3] = [1 2 3; 4 5 6; 7 8 9]`, "then A(1,0) evaluates to 4". So `;`
/// steps dimension 0 and whitespace steps dimension 1.
///
/// The four spellings after it are the ones `aggregate.tex:192-196` calls
/// equivalent -- `Expr.Array.b` through `.e` -- and the rank-three cube encodes
/// each element's own coordinates, so a permuted order cannot agree by
/// accident.
#[test]
fn a_matrix_aggregate_places_its_elements_where_the_specification_says() {
    let binary = compile_fixture("arrayaggregate.fss", "arrayaggregate");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "4\n5\n5\n5\n5\n7\n");
    let _ = std::fs::remove_file(&binary);

    // Rank three and NON-square, kept in its own file: a transposed literal of
    // a non-square shape stops compiling against its declaration, so it never
    // reaches the value comparison above. 234 sits at `a[1,2,3]` -- the values
    // encode their own coordinates.
    let binary = compile_fixture("arraycube.fss", "arraycube");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "234\n");
    let _ = std::fs::remove_file(&binary);
}

/// A literal whose groups differ in size names no rectangle, and matrix pasting
/// -- an array as an ELEMENT of an array literal, `aggregate.tex:180-188` -- is
/// not built. Both refused by name rather than by an odometer quietly taking
/// each extent to be the largest index it reached.
#[test]
fn a_ragged_or_pasted_aggregate_is_refused_by_name() {
    let message = refusal("badraggedarray.fss");
    assert!(
        message.contains("this array literal is ragged"),
        "{message}"
    );
    let message = refusal("badpastedarray.fss");
    assert!(
        message.contains("expected ZZ32, found Array[\\ZZ32\\]"),
        "{message}"
    );
}

/// EVERY DIMENSION IS CHECKED ON ITS OWN. `a[0,4]` on a 2 by 3 linearises to
/// offset 4, which is inside the six slots the array holds -- so a check made
/// after the linearisation lets it through and hands back `a[1,1]`. Measured,
/// not argued: with the per-dimension check replaced by a total comparison this
/// program prints `0` at exit 0.
#[test]
fn a_subscript_is_bounds_checked_in_each_dimension() {
    let binary = compile_fixture("arrayoob.fss", "arrayoob");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("out of bounds in dimension 1"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_file(&binary);
}

/// The rank is a fact about the TYPE and the subscript count is a fact about
/// the SOURCE, so only the checker can compare them. Both directions, because
/// only one of them was ever a parse error.
#[test]
fn a_subscript_count_must_match_the_rank() {
    let message = refusal("badsubscriptfew.fss");
    assert!(
        message.contains("a rank 2 array takes 2 subscript(s), found 1"),
        "{message}"
    );
    let message = refusal("badsubscriptmany.fss");
    assert!(
        message.contains("a rank 1 array takes 1 subscript(s), found 2"),
        "{message}"
    );
}

/// `length` and iteration are rank-one operations and are refused BY NAME above
/// it -- 1.0 gives `Array2` a per-dimension extent and no single `length`, so a
/// total would be inventing a meaning and an extent would be picking a
/// dimension. `array(6)` for a `ZZ32[2,3]` is the constructor's half of the
/// same rule.
#[test]
fn a_rank_two_array_refuses_the_rank_one_operations() {
    let message = refusal("badarrayrank.fss");
    assert!(
        message.contains("`length` of a rank 2 array is not in this subset"),
        "{message}"
    );
    let message = refusal("badarraynewarity.fss");
    assert!(
        message.contains("`array` takes 2 argument(s), found 1"),
        "{message}"
    );
}

/// `traits.tex:106-108` gives an extent three spellings. Only the bare size
/// resolves: a lower bound other than zero has nowhere to live, because
/// `fortress_array_slot` indexes from zero and the header carries a length and
/// no origin.
#[test]
fn an_extent_range_is_refused_by_name() {
    let message = refusal("badextentrange.fss");
    assert!(message.contains("is an extent range"), "{message}");
    let message = refusal("badarraysize.fss");
    assert!(message.contains("writes no size"), "{message}");
}

/// `RR^3` and `ZZ32^(2 BY 4)` are 1.0's VECTOR and MATRIX types. They are not
/// `Array1` and do not share its trait, so resolving them to a one dimensional
/// array would be a wrong answer rather than a partial one. All 18 corpus
/// sites are shapes; not one is the dimension exponent that shares the
/// spelling. `BY` reaches the parser as `OpWord`, not `Ident` -- the fixture
/// would not even parse if the recogniser matched only the latter.
#[test]
fn a_vector_or_matrix_type_is_refused_by_name() {
    let message = refusal("badmatrixtype.fss");
    assert!(
        message.contains("a vector or matrix type is not implemented"),
        "{message}"
    );
}

/// AT MOST ONE SHAPE SUFFIX. 1.0 forbids stacking at three separate sites and
/// this compiler enforces it by returning after the first rather than looping,
/// so the second is reported as whatever the caller expected next instead of
/// being swallowed.
#[test]
fn a_shape_suffix_may_not_be_stacked() {
    let message = refusal("badstackedshape.fss");
    assert!(
        message.contains("expected `)`, found LBracket"),
        "{message}"
    );
}

/// `subscripting.tex:53-54` -- a result type may appear "but it must be ()".
/// WITHOUT THIS, Compiled5.az.fss became a new must-fail acceptance the moment
/// the form parsed. The gate caught it; review did not.
#[test]
fn a_subscripted_assignment_with_a_non_unit_result_is_refused() {
    let message = refusal("badsubscriptreturn.fss");
    assert!(
        message.contains("if a result type is given it must be `()`"),
        "{message}"
    );
}

/// Same section, :47-49 -- exactly one value parameter after `:=`.
#[test]
fn a_subscripted_assignment_with_two_values_is_refused() {
    let message = refusal("badsubscriptvalue.fss");
    assert!(
        message.contains("takes exactly one value parameter after `:=`"),
        "{message}"
    );
}

// ------------------------------------------------------------- `||`, and only `||`
//
// The one builtin a USER DECLARATION BEATS. Every other builtin shadows a
// declaration of its name; `||` is an ordinary library operator
// (`FortressLibrary.fss:4020`) and not a keyword, and a program declaring its
// own compiles and runs. Both directions are asserted because a naive builtin
// arm passes the first and silently breaks the second.

/// The fallback: PLAIN concatenation, per the 2026-08-21 juxtaposition ruling.
#[test]
fn an_undeclared_bar_bar_concatenates_strings() {
    let binary = compile_fixture("concatbar.fss", "concatbar");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ab\ncount = 7\n");
    let _ = std::fs::remove_file(&binary);
}

/// AND THE INVERSION.
#[test]
fn a_declared_bar_bar_wins_over_the_builtin() {
    let binary = compile_fixture("concatbarlocal.fss", "concatbarlocal");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
    let _ = std::fs::remove_file(&binary);
}

/// Neither side a String and nothing declared: the library declares `||` on
/// String and nowhere else, so this must not silently become "34".
#[test]
fn bar_bar_on_two_numbers_is_refused() {
    let message = refusal("badconcatbar.fss");
    assert!(message.contains("unknown name `||`"), "{message}");
}

// ------------------------------------------------------- `nat`/`int`/`bool`
//
// D7 §3.1. A value static parameter is substituted with a NUMBER, and the
// argument must be STATICALLY EVALUABLE -- "the rule is *statically evaluable*,
// not *literal*", because `Library/Generator22D.fss` writes
// `[\T, 0, s0 + s2, 0, s1 + s3\]` and a literals-only rule cannot compile the
// library's own array generators.

/// All three kinds, a static expression over an enclosing parameter, and
/// JUXTAPOSITION AS PRODUCT -- `[\2 3\]` is 6, which is 13 corpus sites'
/// spelling and the reason there is no `*` in the sublanguage.
#[test]
fn value_static_parameters_are_substituted_with_their_values() {
    let binary = compile_fixture("natparams.fss", "natparams");
    let out = run(&binary);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "4\n10\n10\nyes\n-9\n7\n12\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

/// `[\2 + 3\]` AND `[\5\]` ARE ONE STAMP. That is only true because the
/// VALUE is what gets mangled, and the value only exists after evaluation --
/// mangle the expression instead and MAX_INSTANTIATIONS counts the spelling.
#[test]
fn two_spellings_of_one_value_are_one_instantiation() {
    let ir = emitted_ir("natparams.fss");
    let stamps = ir.matches("define i64 @\"sized$$10$e\"").count()
        + ir.matches("define i64 @\"sized$$5$e\"").count();
    assert_eq!(stamps, 1, "one stamp for 2+3 and 5:\n{ir}");
}

/// A type where a value was declared.
#[test]
fn a_type_in_a_value_static_argument_is_refused() {
    let message = refusal("badnattype.fss");
    assert!(
        message.contains("must be known at compile time"),
        "{message}"
    );
}

/// And a value where a type was declared -- "unknown type `3`" would send the
/// reader looking for a declaration that was never meant to exist.
#[test]
fn a_value_in_a_type_static_argument_is_refused() {
    let message = refusal("badnatvalue.fss");
    assert!(
        message.contains("is a static VALUE and this parameter is declared as a type"),
        "{message}"
    );
}

/// D7 leaves the constraint solver out of v1, and its own census is the reason:
/// NOT ONE `where { k < n }` exists in 1956 files.
#[test]
fn a_bound_on_a_value_static_parameter_is_refused() {
    let message = refusal("badnatbound.fss");
    assert!(
        message.contains("there is no constraint solver"),
        "{message}"
    );
}

/// KIND IS PART OF THE SHAPE. An overload set mixing `f[\T\]` and
/// `f[\nat n\]` has one parameter each with no bounds either side, so the
/// count comparison alone accepts it and then one member wants a type and the
/// other a number at the same position.
#[test]
fn an_overload_set_mixing_a_type_and_a_value_parameter_is_refused() {
    let message = refusal("badnatkindmix.fss");
    assert!(
        message.contains("differ in their static parameters"),
        "{message}"
    );
}

/// D7 §3.2, a NAMED DEVIATION. `NatReflect.reflect` turns a RUN-TIME integer
/// into a static parameter, and a monomorphizing compiler cannot stamp a
/// specialisation for a value it does not know. The diagnostic has to name the
/// mechanism or the failure surfaces as an unrelated mismatch deep inside
/// `ChunkedSparseArray`.
#[test]
fn the_natreflect_runtime_path_is_refused_by_name() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/LibraryBuiltin/NatReflect.fsi"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(
        message.contains("`NatReflect.reflect` produces one at run time"),
        "{message}"
    );
}

// ------------------------------------------- the exported hierarchy, clause by clause
//
// `source-code.tex:290-299` makes a trait's exported hierarchy a fact about the
// api, and `conform.rs` compares all three topology clauses for it. TWO OF THE
// THREE RULES HAD NO INDEPENDENT WITNESS: Compiled5.y.fss trips both the list
// comparison and the open-marker one, so deleting either left the corpus
// unchanged and the mutation table read both as escapes. These two fixtures
// isolate them. The `excludes` rule has a corpus witness in Compiled3.g.fss.

/// Both clauses CLOSED and the lists differ: only the list comparison sees it.
#[test]
fn a_component_that_widens_the_apis_comprises_list_is_refused() {
    let message = refusal("badexportcomprises.fss");
    assert!(
        message.contains("to comprise exactly what the api declares"),
        "{message}"
    );
}

/// The SAME list, one OPEN and one closed: only the marker comparison sees it.
#[test]
fn a_component_that_closes_the_apis_open_comprises_clause_is_refused() {
    let message = refusal("badexportopen.fss");
    assert!(
        message.contains("the same OPEN (`...`) `comprises` clause"),
        "{message}"
    );
}

// ------------------------------------------------ overloading inside an api
//
// M3c's ambiguity check is driven by the tuples a CALL SITE can produce, and an
// api has no call sites, so an api's overload set was checked by nothing at all
// from the day api check mode landed.

/// `overloading.tex` -- `f(x:O,y:T)` and `f(x:T,y:O)` with `O extends T` are
/// ambiguous at `(O, O)`.
#[test]
fn an_ambiguous_overload_set_in_an_api_is_refused() {
    let message = refusal("badapioverload.fsi");
    assert!(message.contains("`f` is ambiguous for (O, O)"), "{message}");
}

/// AND THE NEGATIVE, because a rule that refused every overloaded api would
/// pass the test above and take the library with it.
#[test]
fn an_unambiguous_overload_set_in_an_api_is_accepted() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(fixture("apioverloadok.fsi"))
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ------------------------------------------------- `comprises` well-formedness
//
// The clause was PARSED AND DROPPED since M3c, so all four of these compiled
// and ran before this existed. Each cites the rule it is testing, because the
// three are separate sentences in `traits.tex` and collapsing them into one
// "invalid comprises clause" is how a diagnostic stops naming a mechanism.

/// `traits.tex:161-162` -- an api may write `comprises { ... }`; a component
/// may not. The marker used to be discarded by the parser, so an open set and
/// an unwritten one were the same empty list.
#[test]
fn an_open_comprises_clause_in_a_component_is_refused() {
    let message = refusal("badcomprisesopen.fss");
    // The phrase is the one only THIS rule prints. `is open (`...`)` appears in
    // the :236-241 diagnostic too, and asserting on that made the fixture pass
    // with this rule deleted -- caught by the mutation table, not by review.
    assert!(
        message.contains("an api may write and a component may not"),
        "{message}"
    );
}

/// `traits.tex:232-235` -- the traits a `comprises` clause lists "must
/// explicitly extend T".
#[test]
fn a_comprises_name_that_does_not_extend_the_trait_is_refused() {
    let message = refusal("badcomprisesnoextend.fss");
    assert!(
        message.contains("`P` is listed in the `comprises` clause of `T`"),
        "{message}"
    );
}

/// `traits.tex:236-241` -- a component exporting the api may extend an
/// open-comprises trait, but the api may not declare something that does.
#[test]
fn an_api_extending_its_own_open_comprises_trait_is_refused() {
    let message = refusal("badcomprisesextendsopen.fsi");
    assert!(
        message.contains("an api may not declare a trait that extends"),
        "{message}"
    );
}

/// AND THE POSITIVE CASE, which is the one that says the rule is not simply
/// refusing every `comprises` clause it sees -- 28 corpus files carry one and
/// a blanket refusal would have taken all of them.
#[test]
fn a_well_formed_comprises_clause_still_compiles_and_runs() {
    let binary = compile_fixture("comprisesok.fss", "comprisesok");
    let out = run(&binary);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_file(&binary);
}

// ------------------------------------------------------- api resolution

/// A corpus file, by path from the repository root. Resolution's whole subject
/// is finding OTHER files, so its fixtures cannot live in `tests/`.
fn corpus(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(rel)
}

/// Resolution is the default; the flag is passed anyway so these read as
/// resolution tests rather than as ordinary compiles.
fn resolve_output(rel: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus(rel))
        .arg("--resolve-imports")
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc")
}

/// The first name to cross a file boundary in this compiler.
/// `RecursiveApiTest3a.fss` imports `RecursiveApiTest3b`, which imports it
/// back -- so it is also the test that the walk terminates on a cycle.
#[test]
fn a_type_declared_in_an_imported_api_is_in_scope() {
    let rel = "ProjectFortress/compiler_tests/RecursiveApiTest3a.fss";
    let with = resolve_output(rel);
    assert_eq!(
        with.status.code(),
        Some(0),
        "should compile with resolution:\n{}",
        String::from_utf8_lossy(&with.stderr)
    );
    // Resolution is ON by default since phase 2, so the negative half of this
    // test is what `--no-resolve-imports` is for: without it the assertion
    // measures nothing.
    let without = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus(rel))
        .arg("--no-resolve-imports")
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc");
    assert_eq!(
        without.status.code(),
        Some(1),
        "and must NOT compile without it, or the test proves nothing"
    );
}

/// `default_repository/configuration:44`, verbatim. `System` exists in
/// `Library/`, `CompilerLibrary/` and `ProjectFortress/LibraryBuiltin/` and the
/// three are DIFFERENT libraries; the path order is the only thing that says
/// which one an import means, and `LibraryBuiltin` comes first.
#[test]
fn the_source_path_order_decides_which_of_a_duplicated_api_is_meant() {
    let out = resolve_output("ProjectFortress/LibraryBuiltin/System.fsi");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "the LibraryBuiltin api checks clean:\n{message}"
    );
    // `Library/System.fsi` names `ImmutableArray`, which nothing declares.
    let other = resolve_output("Library/System.fsi");
    assert!(
        String::from_utf8_lossy(&other.stderr).contains("ImmutableArray"),
        "and the Library one is a different file"
    );
}

/// An api no file on the source path provides is REPORTED and skipped. Of the
/// 68 top-level `.fsi` files in the library set, most do not yet parse; making
/// an unreadable api fatal would measure the parser rather than the resolver
/// and take every importing component down with it.
#[test]
fn an_unresolvable_api_is_reported_rather_than_fatal() {
    let out = resolve_output("ProjectFortress/compiler_tests/RecursiveApiTest3a.fss");
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(
        message.contains("resolved 2 api(s)"),
        "resolution reports what it loaded:\n{message}"
    );
    // `Library/GeneratorLibrary.fsi` imports `CompilerAlgebra` and four others;
    // whatever is not on the source path is NAMED rather than being fatal.
    let other = resolve_output("Library/Reader.fsi");
    let message = String::from_utf8_lossy(&other.stderr);
    assert!(
        !message.contains("internal error"),
        "an unresolvable api is never an internal error:\n{message}"
    );
}

// ------------------------------------------------- component satisfies api

fn conform_output(rel: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus(rel))
        .arg("--check-exports")
        .arg("--emit-obj")
        .arg("-o")
        .arg("/dev/null")
        .output()
        .expect("could not run fortressc")
}

/// `source-code.tex:313-320`: a component must satisfy every top-level
/// declaration in any api it EXPORTS. Until this existed `Component::exports`
/// had no readers at all -- `export Executable` was a token the parser stored
/// and nobody asked about.
///
/// `Compiled0.p.fss` exports `Executable` and declares `ran()`. One letter, and
/// nothing in the compiler had ever looked.
#[test]
fn a_component_that_does_not_implement_its_api_is_refused() {
    let out = conform_output("ProjectFortress/compiler_tests/Compiled0.p.fss");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(
        message.contains("`run`, which is not declared"),
        "{message}"
    );
}

/// An api may declare an OVERLOAD SET, and satisfying one member is not
/// satisfying the api. `test_library/Compiled2.a.fsi` declares `f(): ()` and
/// `f(s: String): ()`; the component declares only the first.
#[test]
fn every_member_of_an_exported_overload_set_must_be_declared() {
    let out = conform_output("ProjectFortress/compiler_tests/Compiled2.a.fss");
    let message = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{message}");
    assert!(message.contains("`f/1`"), "{message}");
}

/// `Function.rats:18`'s `FnSig`: an api may declare a function as a NAME OF
/// ARROW TYPE. `AbstractFunctionDecls.fsi` writes `foo: String -> ()` and the
/// component writes `foo(s: String): () = ()`, and they are the same
/// declaration. Fifteen corpus apis use the form, and the first cut of this
/// check reported every one of them as a violation.
#[test]
fn an_arrow_typed_api_signature_is_the_same_declaration_as_a_function() {
    let out = conform_output("ProjectFortress/compiler_tests/Compiled5.bc.fss");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The corpus's own `Executable` apis all declare `run(): ()` -- NOT the
/// specification's `run(args: String...)`. That is why turning this check on
/// costs two files instead of the 1526 that export `Executable`, and it is
/// worth a test because the spec would predict otherwise.
#[test]
fn the_executable_api_in_this_tree_takes_no_arguments() {
    let out = conform_output("ProjectFortress/tests/atomic2.fss");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `Object` and `Any` are 1.0's root traits, seeded in `Checker::new` because
/// nothing can import them yet. Measured on the merged tree: 334 -> 346 corpus
/// objects, zero lost, and no previously-compiling module's IR body moved.
#[test]
fn object_and_any_are_seeded_root_traits() {
    let binary = compile_fixture("objectany.fss", "objectany");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n");
    let _ = std::fs::remove_file(&binary);
}

/// The seed goes into the trait table and NOT into the duplicate-definition map.
/// `ProjectFortress/LibraryBuiltin/AnyType.fss` declares `trait Any end` itself
/// and compiles today; seeding into `declared` would cost that file.
#[test]
fn a_program_may_still_declare_any_itself() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .join("ProjectFortress/LibraryBuiltin/AnyType.fss"),
        )
        .arg("--emit-ir")
        .output()
        .expect("could not run fortressc");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// THE ROOT TRAITS IN ARROW POSITION, which is the one place they did not
/// resolve. `x: Object` was fine and `f: Object -> ZZ32` was `unknown type
/// Object`, because `closure.rs` kept its OWN list of the names the compiler
/// knows without a declaration -- six, where `mono.rs` had eight.
/// `Library/FortressLibrary.fsi:654` is the file that found it: `Generator[\Any\]`
/// is written at :1992, so expansion substitutes `Any` into `filter`'s arrow
/// and the arrow-lifting pass reported it as undeclared, at the member's span.
#[test]
fn a_root_trait_resolves_inside_an_arrow() {
    let binary = compile_fixture("arrowroot.fss", "arrowroot");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n9\n7\n7\n");
    let _ = std::fs::remove_file(&binary);
}

/// One list, asked from both sides. The two passes cannot share a lookup --
/// `mono` runs before `Checker::new` builds a registry and `closure` runs after
/// -- so what they share is the NAMES, and this is the assertion that they do.
#[test]
fn the_builtin_type_names_agree_across_the_passes() {
    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../types/src/types.rs"),
    )
    .expect("types.rs");
    assert!(
        src.contains("pub(crate) const BUILTIN_TYPE_NAMES: [&str; 9]"),
        "the shared list is what stops a fourth one being written"
    );
    for other in ["closure.rs", "mono.rs"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../types/src")
            .join(other);
        let body = std::fs::read_to_string(&path).expect("source");
        assert!(
            body.contains("BUILTIN_TYPE_NAMES"),
            "{other} must read the shared list, not keep its own"
        );
    }
}

// --------------------------------------------------------------- characters

/// EVERY SHAPE `lexical-structure.tex:862-877` ACCEPTS -- including a NON-ASCII
/// character, which is refused everywhere outside a comment, a string and, by
/// decision, a character literal (`literals.tex:41-46` writes them in the
/// specification's own prose) -- and the middle four
/// lines are CROSS-CHECKS rather than prints: `'0061' = 'a'` and
/// `'TAB' = '\t'` relate two decoding paths to one character, which is the only
/// assertion a decoder that got one path right and the other wrong cannot
/// satisfy.
#[test]
fn every_character_literal_shape_decodes_to_one_character() {
    let binary = compile_fixture("charliteral.fss", "charliteral");
    let out = run(&binary);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a\n'\n\u{01C7}\n\u{03B1}\n\u{1D11E}\ntrue\ntrue\ntrue\ntrue\nordered\nreflexive\nconcatenated: z\n"
    );
    let _ = std::fs::remove_file(&binary);
}

/// THE LEGACY'S OWN RECORDED OUTPUT, and it is what pins the representation: a
/// `Char` lowers to an `i32` and `Char.test` says `run_out_equals=a`, so it
/// must print as ITSELF and not as its code point.
#[test]
fn the_legacy_recorded_a_character_printing_as_itself() {
    let out = Command::new(env!("CARGO_BIN_EXE_fortressc"))
        .arg(corpus("ProjectFortress/other_compiler_tests/Char.fss"))
        .arg("-o")
        .arg(std::env::temp_dir().join("fortress-char-oracle"))
        .output()
        .expect("could not run fortressc");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ran = Command::new(std::env::temp_dir().join("fortress-char-oracle"))
        .output()
        .expect("could not run the binary");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "a\n");
    let _ = std::fs::remove_file(std::env::temp_dir().join("fortress-char-oracle"));
}

/// ORDERED AND NOT NUMERIC. Without the arithmetic refusal, `is_ordered` lets a
/// `Char` into the arithmetic path and `+` emits an integer add on two code
/// points -- a silent wrong answer rather than a missing feature. Naming a
/// character (`'PLUS-MINUS SIGN'`) is PREPROCESSING and refused by name too.
#[test]
fn a_character_is_ordered_and_not_numeric() {
    let message = refusal("badchararith.fss");
    assert!(
        message.contains("is not defined on Char; a character is ordered, not numeric"),
        "{message}"
    );
    let message = refusal("badcharname.fss");
    assert!(
        message.contains("naming a character inside a character literal"),
        "{message}"
    );
    let message = refusal("badforbiddenchar.fss");
    assert!(
        message.contains("a character literal holds one character"),
        "{message}"
    );
}

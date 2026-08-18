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

#[test]
fn allocation_goes_through_the_collector() {
    let binary = compile_fixture("skeleton.fss", "gcsym");
    let symbols = Command::new("nm")
        .arg("-u")
        .arg(&binary)
        .output()
        .expect("could not run nm");
    let undefined = String::from_utf8_lossy(&symbols.stdout);
    assert!(
        undefined.contains("GC_malloc_atomic"),
        "the runtime is still allocating with malloc:\n{undefined}"
    );

    let deps = Command::new("ldd")
        .arg(&binary)
        .output()
        .expect("could not run ldd");
    let deps = String::from_utf8_lossy(&deps.stdout);
    assert!(
        deps.contains("libgc.so"),
        "the collector is not linked:\n{deps}"
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

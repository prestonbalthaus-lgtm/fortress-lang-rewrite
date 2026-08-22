#!/usr/bin/env bash
#
# The M5 gate: `atomic`, and reduction variables.
#
# Six things cargo cannot check on its own.
#
#   * A reduction over a range big enough to reach the pool produces the SERIAL
#     answer, exactly, at every worker count. ZZ32 and ZZ64 only: two's
#     complement addition is associative whatever the grouping, so the merged
#     sum is bit-identical to the serial fold, overflow included. RR64 is
#     deterministic per worker count and NOT across worker counts -- inherent to
#     reassociation, permitted by reduction.tex:43-46 -- so the gate PINS
#     FORTRESS_WORKERS and prints the spread rather than asserting an equality
#     that is not true.
#   * `atomic` around a parallel loop does not deadlock. That is measured with
#     a timeout, because the failure is a hang and not a wrong answer.
#   * A halt while the lock is held still exits, with its diagnostic. Same
#     shape: the defect is a process that never returns.
#   * A loop body that assigns to an outer scalar reaches live storage rather
#     than an internal error -- the exit-70 crash M4 shipped latent.
#   * A block that writes nothing but reduction variables takes NO lock, and
#     one that writes anything else does.
#   * A program with no `atomic` and no reduction emits no call to either
#     runtime, so nothing about M4's generated code moved.
#
#   ./tools/atomic-gate.sh              run the gate
#   ./tools/atomic-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/atomic-gate.sh --mutate     break the compiler seven ways and prove
#                                       the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build/m5
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
fixtures=$repo/fortressc/tests
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# 1000000 and 100000 are both over FORTRESS_PARALLEL_MIN (4096), which is what
# makes these measurements about the pool rather than about the inline path.
SUM_TO_999999=499999500000
HANG_TIMEOUT=10

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# The serial fold, computed here rather than read back out of the thing being
# tested. A gate that asks the compiler what the sum is and then agrees with it
# is checking nothing.
serial_sum() {  # serial_sum N  ->  0 + 1 + ... + N-1
    echo $(( ($1 - 1) * $1 / 2 ))
}

selftest() {
    printf '== gate self test ==\n'

    local want got
    want=$SUM_TO_999999
    got=$(serial_sum 1000000)
    if [[ $got == "$want" ]]; then
        ok "the serial fold is computed independently ($got)"
    else
        bad 'the serial fold is computed independently' "got $got, want $want"
    fi
    if [[ $(serial_sum 1000) == 499500 ]]; then
        ok 'the serial fold agrees on the small case too'
    else
        bad 'the serial fold agrees on the small case too'
    fi
    # The comparison must be able to say no.
    if [[ $(serial_sum 1000) == 499501 ]]; then
        bad 'a wrong sum is rejected' 'the comparison accepted 499501'
    else
        ok 'a wrong sum is rejected'
    fi

    # The hang detector must fire on something that really hangs, or every
    # deadlock case below passes trivially.
    if timeout "$HANG_TIMEOUT" true; then ok 'the timeout lets a fast command through'
    else bad 'the timeout lets a fast command through'; fi
    timeout 1 sleep 5 >/dev/null 2>&1
    if [[ $? -eq 124 ]]; then ok 'the timeout reports 124 on something that hangs'
    else bad 'the timeout reports 124 on something that hangs'; fi
}

preflight() {
    printf '\n== preflight ==\n'
    if [[ -x $fortressc ]]; then ok 'the compiler is built'
    else bad 'the compiler is built' "no $fortressc"; return 1; fi
    rm -rf "$build"; mkdir -p "$build"

    # The stride codegen writes and the stride the runtime is handed have to be
    # the same number, and they are the same number BY CONSTRUCTION: codegen
    # passes it. This asserts the call is actually shaped that way, because a
    # constant that stopped travelling would be an out of bounds store from a
    # worker thread and nothing else would notice.
    if grep -q 'REDUCTION_ALLOC,' "$repo/fortressc/crates/codegen/src/lib.rs" &&
       grep -q 'const REDUCTION_STRIDE: u64 = 64;' "$repo/fortressc/crates/codegen/src/lib.rs" &&
       grep -q 'fortress_reduction_alloc(int64_t reductions, int64_t stride)' "$repo/fortressc/runtime/shims.c"; then
        ok 'the accumulator stride travels from codegen to the allocator'
    else
        bad 'the accumulator stride travels from codegen to the allocator'
    fi
}

compile_one() {  # compile_one NAME
    "$fortressc" "$fixtures/$1.fss" -o "$build/$1" >"$build/$1.err" 2>&1
}

# ------------------------------------------------------------------ the sums

reductions() {
    printf '\n== reductions reach the pool and still fold to the serial answer ==\n'
    local name w got want

    for name in reductionsum reductionzz32; do
        if compile_one "$name"; then ok "$name compiles"
        else bad "$name compiles" "$(head -2 "$build/$name.err")"; continue; fi
    done

    want=$(serial_sum 1000000)
    for w in 1 2 4 8 16; do
        got=$(FORTRESS_WORKERS=$w "$build/reductionsum" 2>&1)
        if [[ $got == "$want" ]]; then
            ok "ZZ64 reduction over 1000000 is exact on $w worker(s) ($got)"
        else
            bad "ZZ64 reduction over 1000000 is exact on $w worker(s)" "got $got, want $want"
        fi
    done

    # `+=` and `-=` on two ZZ32 variables in ONE atomic block: 100000 up from 0,
    # and 100000 down from 1000. The negative answer is the point -- `-=`
    # accumulates Identity - e and merges with `+`, so a merge that used the
    # wrong operator would come back positive.
    for w in 1 8; do
        got=$(FORTRESS_WORKERS=$w "$build/reductionzz32" 2>&1 | tr '\n' ',')
        if [[ $got == '100000,-99000,' ]]; then
            ok "two ZZ32 reductions, += and -=, are exact on $w worker(s)"
        else
            bad "two ZZ32 reductions, += and -=, are exact on $w worker(s)" "got $got"
        fi
    done
}

# ------------------------------------------------------ the two ways it hangs

deadlocks() {
    printf '\n== the two ways one global lock hangs ==\n'
    local status out

    # (i) `atomic` OUTSIDE the loop. The inner `for` really distributes, the
    # workers block on the mutex the calling thread holds, and the calling
    # thread parks at the join. A recursive mutex does not help: recursion
    # rescues re-entry by the SAME thread and the workers are different
    # threads. fortress_atomic_enter handing over fortress_in_parallel is what
    # makes the inner loop run inline instead.
    if compile_one atomicoutside; then
        out=$(FORTRESS_WORKERS=8 timeout "$HANG_TIMEOUT" "$build/atomicoutside" 2>&1)
        status=$?
        if [[ $status -eq 0 && $out == '1000000' ]]; then
            ok "atomic around a parallel loop completes ($out)"
        else
            bad 'atomic around a parallel loop completes' "status $status, output: $out"
        fi
    else
        bad 'atomicoutside compiles' "$(head -2 "$build/atomicoutside.err")"
    fi

    # (ii) A halt with the lock held. fortress_pool_stop is an atexit handler
    # that joins the pool, and a worker parked in fortress_atomic_enter can
    # never be joined -- the diagnostic prints and the process hangs forever,
    # which under srun is a job burning its whole allocation.
    if compile_one atomichalt; then
        out=$(FORTRESS_WORKERS=8 timeout "$HANG_TIMEOUT" "$build/atomichalt" 2>&1)
        status=$?
        if [[ $status -eq 1 && $out == *'out of bounds'* ]]; then
            ok 'a halt inside atomic exits 1 with its diagnostic'
        else
            bad 'a halt inside atomic exits 1 with its diagnostic' "status $status, output: $out"
        fi
    else
        bad 'atomichalt compiles' "$(head -2 "$build/atomichalt.err")"
    fi
}

# ------------------------------------------------------- capture by reference

by_reference() {
    printf '\n== a loop body assigns to storage the caller owns ==\n'
    local got status

    # THE EXIT-70 CASE. M4's own diagnostic walked the user into it: refuse the
    # parallel form, recommend `seq(...)`, and the seq form was an internal
    # error. It went latent because no corpus file writes this shape.
    if compile_one seqouterassign; then
        got=$("$build/seqouterassign" 2>&1)
        if [[ $got == 499500 ]]; then
            ok "a seq loop assigning to an outer scalar runs ($got)"
        else
            bad 'a seq loop assigning to an outer scalar runs' "got $got"
        fi
    else
        status=$?
        bad 'a seq loop assigning to an outer scalar compiles' \
            "exit $status: $(head -2 "$build/seqouterassign.err")"
    fi

    # The lock path, and the half that is easy to miss: a naive build that kept
    # M4's by-VALUE capture would be silently wrong WITH THE LOCK HELD, every
    # worker incrementing its own loop-entry copy.
    if compile_one atomiclocked; then
        got=$(FORTRESS_WORKERS=8 timeout "$HANG_TIMEOUT" "$build/atomiclocked" 2>&1)
        if [[ $got == 200000 ]]; then
            ok "an atomic := through a captured scalar is exact ($got)"
        else
            bad 'an atomic := through a captured scalar is exact' "got $got, want 200000"
        fi
    else
        bad 'atomiclocked compiles' "$(head -2 "$build/atomiclocked.err")"
    fi
}

# ------------------------------------------------------------------ the shape

shapes() {
    printf '\n== what reaches the generated code ==\n'
    local ir

    # A block that writes nothing but reduction variables takes NO lock:
    # reduction.tex:40-42 gives up atomic's visibility guarantee for exactly
    # that name. Without this the one corpus file that reaches the pool takes a
    # process-wide mutex 30000 times, measured at 13.7x SLOWER than serial.
    ir=$("$fortressc" "$fixtures/reductionzz32.fss" --emit-ir -o /dev/stdout 2>/dev/null)
    if [[ $ir == *fortress_reduction_alloc* && $ir != *'call void @fortress_atomic_enter()'* ]]; then
        ok 'an atomic block of nothing but reductions takes no lock'
    else
        bad 'an atomic block of nothing but reductions takes no lock'
    fi

    # And one that writes anything else does take it.
    ir=$("$fortressc" "$fixtures/atomiclocked.fss" --emit-ir -o /dev/stdout 2>/dev/null)
    if [[ $ir == *'call void @fortress_atomic_enter()'* && $ir == *'call void @fortress_atomic_leave()'* ]]; then
        ok 'an atomic block that is not a reduction does take the lock'
    else
        bad 'an atomic block that is not a reduction does take the lock'
    fi

    # M5 changed the loop-body ABI, so "byte identical to M4" is not available
    # and claiming it would be a lie. What IS true, and is what the rule was
    # protecting: a program with neither construct calls neither runtime.
    ir=$("$fortressc" "$fixtures/parallelfill.fss" --emit-ir -o /dev/stdout 2>/dev/null)
    if [[ $ir != *'call void @fortress_atomic_enter()'* && $ir != *'call ptr @fortress_reduction_alloc'* ]]; then
        ok 'a program with no atomic and no reduction calls neither runtime'
    else
        bad 'a program with no atomic and no reduction calls neither runtime'
    fi

    # The worker index is the LAST parameter. Putting it second would renumber
    # `env` from get_nth_param(1) to (2), and get_nth_param returns an Option --
    # a wrong index is a run-time internal error, not a compile error.
    if [[ $ir == *'define void @"$loop1"(i64 %0, ptr %1, i64 %2)'* ]]; then
        ok 'the outlined body is (index, env, chunk), in that order'
    else
        bad 'the outlined body is (index, env, chunk), in that order'
    fi
}

# ---------------------------------------------------------------- the refusals

refusals() {
    printf '\n== what M5 refuses ==\n'
    # `entry` and `label` are LOCAL because this function is called from inside
    # the mutation loop, which iterates a variable of each name. Without it a
    # SURVIVED line names a CHECK instead of the mutation that survived -- which
    # is what generics-gate did, and it cost a session's worth of misreading.
    local entry name phrase label err status
    while IFS='|' read -r name phrase label; do
        err=$("$fortressc" "$fixtures/$name.fss" --emit-obj -o /dev/null 2>&1)
        status=$?
        if [[ $status -eq 1 && $err == *"$phrase"* ]]; then
            ok "$label (exit $status)"
        else
            bad "$label" "status $status: $err"
        fi
    done <<'CASES'
badsharedarray|out of reach of the loop's own rules|a shared array handed to a call inside a parallel body is refused
badparallelescape|is declared outside this loop|assigning to an outer binding is still refused
badparallelindex|the element its own iteration owns|assigning to a slot this iteration does not own is still refused
badreductionread|is declared outside this loop|a compound assignment to a name the body also READS is refused
CASES
}

# ----------------------------------------------------------------- mutations
#
# file|from|to|label. No `|` in any field: the table is split on IFS='|'.

MUTATIONS=(
  'runtime/shims.c|        fortress_in_parallel = 1;|        fortress_in_parallel = 0;|stop atomic from demoting a loop reached inside it'
  'runtime/shims.c|    _exit(1);|    exit(1);|run the atexit handlers on an abnormal halt'
  'runtime/shims.c|        task->body(i, task->env, w);|        task->body(i, task->env, 0);|give every worker the same accumulator row'
  'runtime/shims.c|    pthread_mutexattr_settype(&attr, PTHREAD_MUTEX_RECURSIVE);|    pthread_mutexattr_settype(&attr, PTHREAD_MUTEX_NORMAL);|make the atomic mutex non-recursive'
  'crates/codegen/src/lib.rs|        Ok(Slot::Cell { pointer, ty })|        Ok(Slot::Value(self.builder.build_load(ty, pointer, "by.value").map_err(CodegenError::from_builder)?))|capture an assigned scalar by value again'
  'crates/types/src/lib.rs|                && !ctx.captures.contains_key(&name)|                && true|drop reduction.tex condition 3, so a name the body reads is still private'
  'crates/types/src/lib.rs|            if !sequential && !record.all_atomic {|            if false {|let a compound assignment that is not a reduction through'
)

# FORTRESSC AND --mutate DO NOT MIX, and the failure is silent. Every mutation
# below rebuilds fortressc/target/debug; if FORTRESSC points anywhere else the
# gate keeps reading the pinned binary, the mutation has no effect, the
# assertion holds, and the table reports a clean escape. Refuse instead.
mutate_needs_the_built_compiler() {
    local built=$repo/fortressc/target/debug/fortressc
    if [[ $fortressc != "$built" ]]; then
        printf 'refusing --mutate: FORTRESSC is %s\n' "$fortressc" >&2
        printf 'but every mutation rebuilds %s.\n' "$built" >&2
        printf 'A pinned binary makes each mutation a silent no-op. Unset FORTRESSC.\n' >&2
        exit 2
    fi
}

mutate() {
    mutate_needs_the_built_compiler
    if ! git -C "$repo" diff --quiet HEAD -- fortressc/crates fortressc/runtime; then
        printf 'refusing to mutate: the tree differs from HEAD\n' >&2
        exit 2
    fi

    local entry file from to label hits status
    local broken=0 survived=0
    for entry in "${MUTATIONS[@]}"; do
        IFS='|' read -r file from to label <<<"$entry"
        printf '\n== mutation: %s ==\n' "$label"

        hits=$(grep -F -c -- "$from" "$repo/fortressc/$file")
        if [[ $hits -ne 1 ]]; then
            printf 'FAIL  the mutation pattern is not unique (%s hits in %s)\n' "$hits" "$file"
            broken=$((broken + 1)); continue
        fi

        python3 - "$repo/fortressc/$file" "$from" "$to" <<'PY'
import sys, pathlib
path, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
p = pathlib.Path(path)
p.write_text(p.read_text().replace(old, new, 1))
PY
        ( cd "$repo/fortressc" && cargo build --workspace >/dev/null 2>&1 )
        status=$?
        if [[ $status -ne 0 ]]; then
            printf 'REFUSED  the mutated compiler does not build, which is a refusal too\n'
        else
            passed=0; failed=0
            preflight >/dev/null 2>&1
            reductions; deadlocks; by_reference; shapes; refusals
            if [[ $failed -gt 0 ]]; then
                printf 'REFUSED  %d check(s) failed, which is the point\n' "$failed"
            else
                printf 'SURVIVED %s -- the gate did not notice\n' "$label"
                survived=$((survived + 1))
            fi
        fi
        git -C "$repo" checkout HEAD -- "fortressc/$file"
    done
    ( cd "$repo/fortressc" && cargo build --workspace >/dev/null 2>&1 )

    printf '\nmutations: %d run, %d survived, %d could not be applied\n' \
        "${#MUTATIONS[@]}" "$survived" "$broken"
    [[ $survived -eq 0 && $broken -eq 0 ]]
}

# ----------------------------------------------------------------- main

case "${1:-}" in
    --selftest) selftest ;;
    --mutate)   selftest; preflight; mutate; exit $? ;;
    *)
        selftest
        preflight || exit 1
        reductions
        deadlocks
        by_reference
        shapes
        refusals
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

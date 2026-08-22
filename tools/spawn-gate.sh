#!/usr/bin/env bash
#
# The M6 gate: `spawn`, `Thread[\T\]` and the runtime's spawn queue.
#
# EVERY FAILURE IN THIS MILESTONE IS A HANG, not a wrong answer, so every case
# here runs under a timeout and the gate self-tests the timeout first. A gate
# for concurrency that cannot tell "correct" from "never returned" is checking
# nothing.
#
# What cargo cannot check on its own:
#
#   * Spawn1/2/3 RUN, at every worker count from 1 to 8. The corpus files are
#     self-checking -- they `assert` -- and none has a `.test`, so the oracle
#     ratchet cannot see them and this gate is the only thing that does.
#   * THE PARENT'S SPIN TERMINATES WITH NO JOIN (Spawn2). This is the case that
#     forbids running a spawned body only at a join point, and it is why the
#     runner thread exists and ignores FORTRESS_WORKERS.
#   * THE CHILD IS NOT RUN AT THE SPAWN SITE (Spawn3). The child spins on a
#     value the parent stores afterwards; run to completion at the spawn site
#     it never returns.
#   * FORTRESS_WORKERS=1 IS THE CASE THAT MATTERS, because tools/oracle-gate.sh
#     exports it. At one worker the LOOP pool spawns zero threads, so a queue
#     served only by pool workers would have no runner at all.
#   * A MUTABLE IS CAPTURED BY REFERENCE EVEN WHEN THE BODY ONLY READS IT. The
#     defect this pins hung Spawn3 and is invisible statically.
#   * The two refusals are refusals BY NAME and the gate asserts the MESSAGE:
#     `spawn` inside `atomic`, and `val()` on a non-scalar.
#   * A program with no `spawn` emits no call to the spawn runtime, so nothing
#     about M4's or M5's generated code moved.
#
#   ./tools/spawn-gate.sh              run the gate
#   ./tools/spawn-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/spawn-gate.sh --mutate     break the compiler and prove it refuses
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build/m6
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# Generous, because a correct Spawn3 busy-waits on two threads and a loaded
# machine is slow. Short enough that a real hang is not an afternoon.
HANG_TIMEOUT=25
WORKER_COUNTS="1 2 4 8"
SPAWN_FILES="Spawn1 Spawn2 Spawn3"

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

selftest() {
    printf '== gate self test ==\n'

    # THE TIMEOUT IS THE INSTRUMENT. If it cannot fire, every liveness case
    # below passes trivially and the gate is decorative.
    if timeout "$HANG_TIMEOUT" true; then ok 'the timeout lets a fast command through'
    else bad 'the timeout lets a fast command through'; fi
    timeout 1 sleep 5 >/dev/null 2>&1
    if [[ $? -eq 124 ]]; then ok 'the timeout reports 124 on something that really hangs'
    else bad 'the timeout reports 124 on something that really hangs'; fi

    # The message matcher must be able to say no, or every refusal below passes
    # against any diagnostic at all.
    if printf 'x\n' | grep -q 'x'; then ok 'the message matcher accepts a match'
    else bad 'the message matcher accepts a match'; fi
    if printf 'x\n' | grep -q 'not-in-there'; then
        bad 'the message matcher rejects a non-match'
    else ok 'the message matcher rejects a non-match'; fi
}

preflight() {
    printf '\n== preflight ==\n'
    if [[ -x $fortressc ]]; then ok 'the compiler is built'
    else bad 'the compiler is built' "no $fortressc"; return 1; fi
    rm -rf "$build"; mkdir -p "$build"

    # The runner must not be conditional on the loop pool's worker count. This
    # is the design's central claim and it is one grep: if FORTRESS_WORKERS ever
    # reaches the spawn path, Spawn2 hangs under oracle-gate and nowhere else.
    # `getenv(` and NOT the name of the variable. The M6 section EXPLAINS at
    # length why it ignores FORTRESS_WORKERS, so a textual grep for the name
    # matches the argument for ignoring it -- which is how this check first
    # failed against a correct runtime. What matters is that the section reads
    # no environment at all.
    if grep -q 'fortress_runner_start' "$repo/fortressc/runtime/shims.c" &&
       ! sed -n '/M6$/,$p' "$repo/fortressc/runtime/shims.c" | grep -q 'getenv('; then
        ok 'the spawn runner reads no environment variable'
    else
        bad 'the spawn runner reads no environment variable'
    fi
}

compile_corpus() {  # compile_corpus NAME
    "$fortressc" "$repo/ProjectFortress/tests/$1.fss" -o "$build/$1" \
        >"$build/$1.err" 2>&1
}

# ------------------------------------------------- the corpus files, running

liveness() {
    printf '\n== Spawn1/2/3 compile and RUN at every worker count ==\n'
    local name w rc out

    for name in $SPAWN_FILES; do
        if compile_corpus "$name"; then ok "$name compiles"
        else bad "$name compiles" "$(head -1 "$build/$name.err")"; continue; fi

        for w in $WORKER_COUNTS; do
            out=$(FORTRESS_WORKERS=$w timeout "$HANG_TIMEOUT" "$build/$name" 2>&1)
            rc=$?
            if [[ $rc -eq 124 ]]; then
                bad "$name at $w worker(s)" 'HUNG -- no exit within the timeout'
            elif [[ $rc -ne 0 ]]; then
                bad "$name at $w worker(s)" "exit $rc: $out"
            else
                ok "$name at $w worker(s)"
            fi
        done
    done
}

# ------------------------------------------------------- the shape harness

shapes() {
    printf '\n== the runtime shapes, at the shim level ==\n'
    local out rc
    if cc -I "$repo/fortressc/runtime" \
          "$repo/fortressc/runtime/tests/spawn_shapes.c" \
          "$repo/fortressc/runtime/shims.c" \
          -lgc -lm -pthread -o "$build/spawn-shapes" 2>"$build/shapes.err"; then
        ok 'the shape harness builds'
    else
        bad 'the shape harness builds' "$(head -3 "$build/shapes.err")"; return
    fi
    out=$(timeout 120 "$build/spawn-shapes" 2>&1); rc=$?
    if [[ $rc -eq 0 ]]; then ok 'every shape holds (spawn1/2/3/5/6, steal, join)'
    else bad 'every shape holds' "$(printf '%s' "$out" | grep -E 'FAIL|HUNG' | head -2)"; fi
}

# ------------------------------------------------------------- the refusals

refusals() {
    printf '\n== the two refusals, asserted by MESSAGE and not by exit code ==\n'
    local out

    # spawn.tex:28-31, and Compiled1.am.fss:15 carries the prohibition as a
    # source comment. The spawned child would block on the process-wide atomic
    # mutex its parent holds.
    out=$("$fortressc" "$repo/ProjectFortress/compiler_tests/Compiled1.am.fss" \
          -o "$build/am" 2>&1)
    if printf '%s' "$out" | grep -q 'may not appear inside an `atomic` region'; then
        ok '`spawn` inside `atomic` is refused by name'
    else
        bad '`spawn` inside `atomic` is refused by name' "$(printf '%s' "$out" | head -1)"
    fi

    # `val()` needs a scalar. `Any` is a trait, a trait-typed value is a pointer
    # to a tagged object, and a `()` body produces no tag.
    cat > "$build/valany.fss" <<'FSS'
component valany
export Executable
run():()=do
   var x: ZZ32 = 0
   pt: Thread[\Any\] = spawn do x:=1 end
   println(pt.val())
end
end
FSS
    out=$("$fortressc" "$build/valany.fss" -o "$build/valany" 2>&1)
    if printf '%s' "$out" | grep -q 'a thread.s value has to be a scalar'; then
        ok '`val()` on a non-scalar result is refused by name'
    else
        bad '`val()` on a non-scalar result is refused by name' "$(printf '%s' "$out" | head -1)"
    fi
}

# ------------------------------------------------- nothing else moved

untouched() {
    printf '\n== a program with no `spawn` calls no spawn runtime ==\n'
    cat > "$build/plain.fss" <<'FSS'
component plain
export Executable
run():()=do
   for i<-0#8 do
      println(i)
   end
end
end
FSS
    if "$fortressc" "$build/plain.fss" --emit-ir 2>/dev/null \
        | grep -v '^declare ' | grep -q 'fortress_spawn\|fortress_thread_'; then
        bad 'no spawn call is emitted for a program without one'
    else
        ok 'no spawn call is emitted for a program without one'
    fi
}

# --------------------------------------------------------------- mutations

MUTATIONS=(
  'crates/types/src/lib.rs|                by_ref: self.lookup(&name).is_some_and(|l| l.mutable),|                by_ref: false,|capture a mutable the body only READS by value again'
  'crates/types/src/registry.rs|            return matches!(*want, Type::Trait("Any"));|            return false;|drop the covariance to `Any`, so no Spawn file types'
  'runtime/shims.c|    if (!fortress_runner_started) {|    if (0) {|never start the runner, so nothing runs without a join'
  'runtime/shims.c|        pthread_cond_broadcast(&fortress_spawn_done);|        (void)0;|drop the completion broadcast, so every join blocks for ever'
  'crates/types/src/lib.rs|            return Err(TypeError::SpawnInsideAtomic { span });|            {}|let `spawn` inside `atomic` through'
)

# FORTRESSC AND --mutate DO NOT MIX, and the failure is silent: every mutation
# rebuilds fortressc/target/debug, so a pinned binary makes each one a no-op and
# the table reports a clean escape.
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
            liveness; shapes; refusals; untouched
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
        liveness
        shapes
        refusals
        untouched
        printf '\n%d/%d\n' "$passed" "$failed"
        [[ $failed -eq 0 ]]
        ;;
esac

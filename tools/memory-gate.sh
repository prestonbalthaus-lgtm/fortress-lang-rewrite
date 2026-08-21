#!/usr/bin/env bash
#
# The M3a gate: proof that the collector plugs the leak.
#
# One Fortress program, three builds, one measurement. gcsoak.fss performs a
# million string concatenations; gcsoak_lite.fss performs ten thousand of them.
# If memory is collected, doing a hundred times the work costs the same
# resident set. If it is not, it costs a hundred times as much.
#
#   ./tools/memory-gate.sh              run the gate
#   ./tools/memory-gate.sh --selftest   only prove the assertions can refuse
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
rss=$repo/tools/peak-rss.py
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# The workloads, as written in the fixtures. A hundred to one.
LITE_ITERATIONS=10000
FULL_ITERATIONS=1000000
# Flat means flat: a hundred times the allocations inside twice the memory.
FLAT_TENTHS=20
# What the control has to show for the measurement to be worth anything.
LEAK_TENTHS=50

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# Ratios in tenths, so 20 reads as 2.0x, with no floating point in sight.
at_most_ratio()  { [[ $(($1 * 10)) -le $(($2 * $3)) ]]; }
at_least_ratio() { [[ $(($1 * 10)) -ge $(($2 * $3)) ]]; }

selftest() {
    printf '== gate self test ==\n'

    # Real numbers from a passing run, so the thresholds are checked against
    # the shape of the thing they are meant to judge.
    if at_most_ratio 5764 5760 "$FLAT_TENTHS"; then
        ok 'flatness accepts a curve that does not grow'
    else
        bad 'flatness accepts a curve that does not grow'
    fi

    if at_most_ratio 64080 5760 "$FLAT_TENTHS"; then
        bad 'flatness refuses a curve that grows elevenfold' 'it called a leak flat'
    else
        ok 'flatness refuses a curve that grows elevenfold'
    fi

    if at_least_ratio 64080 5764 "$LEAK_TENTHS"; then
        ok 'the control check sees a leaking build'
    else
        bad 'the control check sees a leaking build'
    fi

    if at_least_ratio 5764 5764 "$LEAK_TENTHS"; then
        bad 'the control check refuses a control that did not leak' \
            'a negative control that measures the same as the subject proves nothing'
    else
        ok 'the control check refuses a control that did not leak'
    fi
}

# ------------------------------------------------------------------ the gate

preflight() {
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    # Wiped, so a failed build cannot be measured as a passing one.
    rm -rf "$build"
    mkdir -p "$build"
}

builds() {
    printf '== builds ==\n'

    if "$fortressc" "$repo/fortressc/tests/gcsoak.fss" -o "$build/soak-gc" 2>"$build/gc.err"; then
        ok 'the million iteration soak compiles and links against the collector'
    else
        bad 'the million iteration soak compiles and links against the collector' "$(cat "$build/gc.err")"
    fi

    if "$fortressc" "$repo/fortressc/tests/gcsoak_lite.fss" -o "$build/soak-gc-lite" 2>"$build/lite.err"; then
        ok 'the ten thousand iteration soak compiles'
    else
        bad 'the ten thousand iteration soak compiles' "$(cat "$build/lite.err")"
    fi

    # The negative control: the same object, linked against the allocator M1
    # shipped. Same program, same work, no collector.
    if "$fortressc" "$repo/fortressc/tests/gcsoak.fss" --emit-obj -o "$build/soak.o" 2>"$build/obj.err" &&
        cc "$build/soak.o" "$repo/fortressc/runtime/shims.c" -DFORTRESS_NO_GC -lm -o "$build/soak-malloc" 2>"$build/ctl.err"; then
        ok 'the leaking control links from the same object'
    else
        bad 'the leaking control links from the same object' "$(cat "$build/obj.err" "$build/ctl.err")"
    fi
}

have() {
    if [[ -f $1 ]]; then
        return 0
    fi
    bad "$2" "no artifact at $1"
    return 1
}

symbols() {
    printf '== symbols ==\n'
    have "$build/soak-gc" 'allocation goes through the collector' || return
    have "$build/soak-malloc" 'the control allocates with malloc and nothing else' || return

    # DEFINED, not undefined. M4 made the collector a static archive, so its
    # symbols are inside the binary rather than left for the loader -- `nm -u`
    # finds nothing and would have reported a collected build as uncollected.
    # COUNTED, not `grep -q`. Under `set -o pipefail` a `grep -q` exits on the
    # first match, `nm` takes SIGPIPE, and the pipeline reports failure even
    # though the symbol was found. The old form survived only because `nm -u`
    # produced few enough lines to fit in the pipe buffer; a static collector's
    # symbol table does not.
    if [[ $(nm "$build/soak-gc" | grep -cE '^[0-9a-f]+ [TtWw] GC_malloc_atomic') -gt 0 ]]; then
        ok 'allocation goes through the collector'
    else
        bad 'allocation goes through the collector' 'no GC_malloc_atomic defined in the binary'
    fi

    if [[ $(nm "$build/soak-malloc" 2>/dev/null | grep -cE ' [TtWwU] GC_') -gt 0 ]]; then
        bad 'the control allocates with malloc and nothing else' 'the control is collected too'
    else
        ok 'the control allocates with malloc and nothing else'
    fi

    # A statically linked collector is the point, not an accident: a Fortress
    # binary carries its collector and needs no LD_LIBRARY_PATH to run, which is
    # what makes it launchable under srun on a compute node.
    if [[ $(ldd "$build/soak-gc" | grep -c 'libgc') -gt 0 ]]; then
        bad 'the collector is linked statically' "$(ldd "$build/soak-gc" | tr '\n' ' ')"
    else
        ok 'the collector is linked statically, so the binary needs no library path'
    fi
}

# All three builds must do the same work, or the memory numbers are comparing
# two different programs.
correctness() {
    printf '== the three builds do the same work ==\n'
    local name out
    for name in soak-gc soak-gc-lite soak-malloc; do
        have "$build/$name" "$name runs and prints its result" || continue
        out=$("$build/$name")
        if [[ $out == done ]]; then
            ok "$name runs and prints its result"
        else
            bad "$name runs and prints its result" "$out"
        fi
    done
}

# Both configurations, because the control is compiled as often as the real one
# and a warning in it would be a warning in the thing being measured against.
warnings() {
    printf '== the runtime compiles clean ==\n'
    local flags out label
    for flags in "" "-DFORTRESS_NO_GC"; do
        label=${flags:-collected}
        out=$(cc -c -Wall -Wextra -Wpedantic -std=c11 $flags -o /dev/null \
            "$repo/fortressc/runtime/shims.c" 2>&1)
        if [[ -z $out ]]; then
            ok "shims.c is clean under -Wall -Wextra -Wpedantic ($label)"
        else
            bad "shims.c is clean under -Wall -Wextra -Wpedantic ($label)" "$out"
        fi
    done
}

memory() {
    printf '== memory ==\n'
    have "$build/soak-gc" 'the measurement runs at all' || return

    local lite full leak
    lite=$("$rss" "$build/soak-gc-lite")   || { bad 'measuring the lite build' "$lite"; return; }
    full=$("$rss" "$build/soak-gc")        || { bad 'measuring the full build' "$full"; return; }
    leak=$("$rss" "$build/soak-malloc")    || { bad 'measuring the control' "$leak"; return; }

    printf '      %s iterations, collected : %s KB\n' "$LITE_ITERATIONS" "$lite"
    printf '      %s iterations, collected : %s KB\n' "$FULL_ITERATIONS" "$full"
    printf '      %s iterations, leaking   : %s KB\n' "$FULL_ITERATIONS" "$leak"

    # The claim, and it is scale invariant rather than a threshold someone
    # picked: a hundred times the allocations, the same resident set.
    if at_most_ratio "$full" "$lite" "$FLAT_TENTHS"; then
        ok "a hundredfold workload costs under ${FLAT_TENTHS}/10 of the memory"
    else
        bad "a hundredfold workload costs under ${FLAT_TENTHS}/10 of the memory" \
            "$lite KB -> $full KB"
    fi

    # Without this the flatness number means nothing: it would also pass on a
    # workload too small to leak visibly.
    if at_least_ratio "$leak" "$full" "$LEAK_TENTHS"; then
        ok "the same program leaking costs over ${LEAK_TENTHS}/10 as much"
    else
        bad "the same program leaking costs over ${LEAK_TENTHS}/10 as much" \
            "collected $full KB, leaking $leak KB: the control did not leak enough to measure"
    fi
}

# ----------------------------------------------------------------- main

if [[ ${1:-} == --selftest ]]; then
    selftest
else
    selftest
    preflight
    builds
    symbols
    correctness
    warnings
    memory
fi

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

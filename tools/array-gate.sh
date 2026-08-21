#!/usr/bin/env bash
#
# The M3b gate: arrays and the loop that fills them.
#
# Four things cargo cannot check on its own: that the sum is the sum, that an
# out of bounds subscript halts instead of faulting, that a mutable declared in
# a loop body costs one stack slot rather than a million, and that the
# collector can see what an array is holding.
#
#   ./tools/array-gate.sh              run the gate
#   ./tools/array-gate.sh --selftest   only prove the assertions can refuse
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# The workload in tests/arraysum.fss.
ELEMENTS=100

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# The sum of the first n squares, computed here rather than copied from the
# program's output. The gate has to know the answer independently or it is only
# checking that the program is consistent with itself.
sum_of_squares() { printf '%d' $(((($1 - 1) * $1 * (2 * $1 - 1)) / 6)); }

# A clean halt: a diagnostic and a nonzero status. 139 is SIGSEGV and 134 is
# SIGABRT, and both mean the bounds check did not happen.
halted_cleanly() { [[ $1 -ne 0 && $1 -ne 139 && $1 -ne 134 ]]; }

selftest() {
    printf '== gate self test ==\n'

    if [[ $(sum_of_squares 100) == 328350 ]]; then
        ok 'the closed form agrees with the known value for 100'
    else
        bad 'the closed form agrees with the known value for 100' "$(sum_of_squares 100)"
    fi

    if [[ $(sum_of_squares 10) == 285 ]]; then
        ok 'the closed form agrees with the known value for 10'
    else
        bad 'the closed form agrees with the known value for 10' "$(sum_of_squares 10)"
    fi

    if halted_cleanly 1; then
        ok 'a clean halt is accepted'
    else
        bad 'a clean halt is accepted'
    fi

    for signal in 139 134 0; do
        if halted_cleanly "$signal"; then
            bad "status $signal is refused as a clean halt" 'a fault is not a diagnostic'
        else
            ok "status $signal is refused as a clean halt"
        fi
    done
}

# ------------------------------------------------------------------ the gate

preflight() {
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    rm -rf "$build"
    mkdir -p "$build"
}

compile() {
    printf '== compile ==\n'
    local name
    for name in arraysum oob loopalloca gcarray; do
        if "$fortressc" "$repo/fortressc/tests/$name.fss" -o "$build/$name" 2>"$build/$name.err"; then
            ok "$name.fss compiles and links"
        else
            bad "$name.fss compiles and links" "$(cat "$build/$name.err")"
        fi
    done
}

have() {
    if [[ -f $1 ]]; then
        return 0
    fi
    bad "$2" "no artifact at $1"
    return 1
}

arithmetic() {
    printf '== the sum ==\n'
    have "$build/arraysum" 'the array program computes the sum' || return

    local out want
    out=$("$build/arraysum")
    want=$(printf 'length = %d\nsum = %d\n' "$ELEMENTS" "$(sum_of_squares "$ELEMENTS")")
    if [[ $out == "$want" ]]; then
        ok "an array of $ELEMENTS is filled by a loop and sums to $(sum_of_squares "$ELEMENTS")"
    else
        bad "an array of $ELEMENTS is filled by a loop and sums to $(sum_of_squares "$ELEMENTS")" \
            "got: $out"
    fi
}

bounds() {
    printf '== bounds ==\n'
    have "$build/oob" 'an out of bounds subscript halts cleanly' || return

    local err status
    err=$("$build/oob" 2>&1 >/dev/null)
    status=$?
    if halted_cleanly "$status" && [[ $err == *"out of bounds"* ]]; then
        ok "an out of bounds subscript halts cleanly (status $status)"
    else
        bad 'an out of bounds subscript halts cleanly' "status $status: $err"
    fi
}

stack() {
    printf '== the stack ==\n'
    have "$build/loopalloca" 'a mutable in a loop body costs one slot' || return

    local out status
    out=$("$build/loopalloca" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == 1000000 ]]; then
        ok 'a million iterations declaring four mutables each do not grow the stack'
    else
        bad 'a million iterations declaring four mutables each do not grow the stack' \
            "status $status: $out"
    fi
}

strings() {
    printf '== an array of strings ==\n'
    have "$build/gcarray" 'an Array[\String\] survives a million allocations' || return

    local out want
    out=$("$build/gcarray")
    want=$(printf 'item 0\nitem 63\nvisits = 4096\n')
    if [[ $out == "$want" ]]; then
        ok 'an Array[\String\] survives a million allocations'
    else
        bad 'an Array[\String\] survives a million allocations' "got: $out"
    fi
}

# The one property no Fortress program can observe about itself: whether the
# collector can see what an array is holding.
tracing() {
    printf '== the collector sees the elements ==\n'
    if ! cc -Wall -Wextra -std=c11 "$repo/fortressc/runtime/tests/array_trace.c" \
        "$repo/fortressc/runtime/shims.c" -lgc -lm -o "$build/array-trace" 2>"$build/trace.err"; then
        bad 'the tracing harness builds' "$(cat "$build/trace.err")"
        return
    fi
    ok 'the tracing harness builds'

    local out
    if out=$("$build/array-trace" 2>&1); then
        ok "an array's elements survive a forced collection"
        printf '      %s\n' "$(printf '%s' "$out" | tr '\n' ' ')"
    else
        bad "an array's elements survive a forced collection" "$(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

# ----------------------------------------------------------------- main

if [[ ${1:-} == --selftest ]]; then
    selftest
else
    selftest
    preflight
    compile
    arithmetic
    bounds
    stack
    strings
    tracing
fi

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

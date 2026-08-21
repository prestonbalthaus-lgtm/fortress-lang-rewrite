#!/usr/bin/env bash
#
# The arithmetic gate: integer division halts instead of faulting.
#
# Nothing in tools/ gated arithmetic at all before this, and the reason it
# survived is measurable: NOT ONE of the 291 corpus files that compile performs
# an integer division. Exactly one performs a floating one. So the corpus can
# never exercise this and only a gate can.
#
# What faults, and why there are two rules rather than one: an `sdiv` traps on
# x86-64 for a zero divisor AND for MIN/-1, whose quotient is not representable.
# Both raise SIGFPE, which is a core dump with no diagnostic -- and it takes
# stdio's buffer with it, so the program loses output it had already produced.
# That last part is asserted here on purpose: every halt case checks that the
# line printed BEFORE the division still reaches the terminal.
#
# 1.0 throws DivisionByZero (opr-overview.tex:164-170, declared at
# Library/FortressLibrary.fss:1459 as an UncheckedException). This subset has no
# exceptions, so division halts the way a bad subscript does -- exit 1 with a
# diagnostic naming the mechanism. That is a named deviation, not an oversight.
#
# RR64 is NOT routed through the guard: 1.0/0.0 is `inf` and that is correct.
#
# The `--target-cpu` refusal rides along because it is the other thing the
# driver refuses that no gate had ever asked it to refuse.
#
#   ./tools/arith-gate.sh              run the gate
#   ./tools/arith-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/arith-gate.sh --mutate     break the compiler seven ways and prove
#                                      the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# A diagnostic is exit 1 and nothing else. 136 is SIGFPE, which is the whole
# point of this gate; 139 is SIGSEGV; 70 is a compiler bug; 0 means the program
# ran to completion when it should have stopped.
halted_cleanly() { [[ $1 -eq 1 ]]; }

# The property the fflush in fortress_abnormal_exit exists for. A halt that
# loses buffered stdout is indistinguishable, from the terminal, from a halt
# before the program ever got that far.
#
# THE MARKER MUST NOT APPEAR IN THE DIAGNOSTIC. The first version of this gate
# used the quotient itself, which `fortress: ... (7, 0)` also prints, so three
# of the four cases asserted nothing -- and only the mutation table showed it.
kept_output() { [[ $1 == *"$2"* ]]; }

selftest() {
    printf '== gate self test ==\n'

    if halted_cleanly 1; then
        ok 'exit 1 is a clean halt'
    else
        bad 'exit 1 is a clean halt'
    fi
    for status in 0 70 101 136 139; do
        if halted_cleanly "$status"; then
            bad "status $status is rejected as a clean halt" \
                'only exit 1 is a diagnostic; 136 is the SIGFPE this gate exists for'
        else
            ok "status $status is rejected as a clean halt"
        fi
    done

    if kept_output $'7\n' '7'; then
        ok 'output before a halt is seen when present'
    else
        bad 'output before a halt is seen when present'
    fi
    if kept_output '' '7'; then
        bad 'empty output is rejected' 'the flush assertion cannot see a lost buffer'
    else
        ok 'empty output is rejected'
    fi
}

# ------------------------------------------------------------------ the gate

preflight() {
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    rm -rf "$build"
    mkdir -p "$build"
}

# Ordinary arithmetic is unchanged. Truncation toward zero for the negative
# case is asserted because a shim is free to get rounding wrong in a way an
# `sdiv` cannot.
quotients() {
    printf '== quotients are unchanged ==\n'
    if ! "$fortressc" "$repo/fortressc/tests/divquotients.fss" -o "$build/divquotients" \
        2>"$build/divquotients.err"; then
        bad 'divquotients.fss compiles' "$(cat "$build/divquotients.err")"
        return
    fi
    local out status want
    out=$("$build/divquotients" 2>&1)
    status=$?
    want=$'3\n-3\n3000000000\n0.25\ninf'
    if [[ $status -eq 0 && $out == "$want" ]]; then
        ok 'ZZ32, negative ZZ32, ZZ64 past 2^31, RR64, and RR64 by zero'
    else
        bad 'quotients are unchanged' "status $status: $out"
    fi
}

# Four ways to reach a trapping `sdiv`, each of which must instead halt, name
# its mechanism, and keep the output the program had already produced.
halts() {
    printf '== a trapping division halts with a diagnostic ==\n'
    local name phrase before out status err
    while IFS='|' read -r name phrase before; do
        [[ -z $name ]] && continue
        if ! "$fortressc" "$repo/fortressc/tests/$name.fss" -o "$build/$name" \
            2>"$build/$name.err"; then
            bad "$name.fss compiles" "$(cat "$build/$name.err")"
            continue
        fi
        out=$("$build/$name" 2>&1)
        status=$?
        if ! halted_cleanly "$status"; then
            bad "$name halts cleanly" "status $status: $out"
            continue
        fi
        if [[ $out != *"$phrase"* ]]; then
            bad "$name names its mechanism" "wanted \`$phrase\`, got: $out"
            continue
        fi
        if kept_output "$out" "$before"; then
            ok "$name halts, names \`$phrase\`, and keeps \`$before\`"
        else
            bad "$name keeps the output it had already produced" \
                "wanted \`$before\` in: $out"
        fi
        if [[ $out == *unreachable* ]]; then
            bad "$name stops at the division" 'it ran past the halt'
        fi
    done <<'CASES'
divzero|integer division by zero|stdout survived the halt
divzz64|integer division by zero|stdout survived the halt
divoverflow32|integer division overflows|stdout survived the halt
divoverflow64|integer division overflows|stdout survived the halt
CASES
}

# The one divisor the run-time guard can never see, because LLVM's own constant
# folder turns the division into `poison` while the module is being built and
# the program then prints whatever that lowers to.
literal_zero() {
    printf '== a literal zero divisor is refused at compile time ==\n'
    local err status
    err=$("$fortressc" "$repo/fortressc/tests/baddivzeroliteral.fss" \
        --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if halted_cleanly "$status" && [[ $err == *"literal zero divisor"* ]]; then
        ok "baddivzeroliteral.fss is refused (exit $status)"
    else
        bad 'baddivzeroliteral.fss is refused' "status $status: $err"
    fi
}

# Two shape assertions on the IR, because "it halts" is compatible with a guard
# that also swallowed the floating path or that fires in programs that never
# divide.
ir_shape() {
    printf '== the shape of what is emitted ==\n'
    local ir
    ir=$("$fortressc" "$repo/fortressc/tests/divquotients.fss" --emit-ir 2>/dev/null)
    if grep -q ' sdiv ' <<<"$ir"; then
        bad 'no bare sdiv survives' 'a trapping instruction is still emitted'
    else
        ok 'no bare sdiv survives'
    fi
    if grep -q 'call i32 @fortress_div_zz32' <<<"$ir" &&
        grep -q 'call i64 @fortress_div_zz64' <<<"$ir"; then
        ok 'both widths route through the guard'
    else
        bad 'both widths route through the guard' 'a division did not reach a shim'
    fi
    if grep -q ' fdiv ' <<<"$ir"; then
        ok 'RR64 division is still an fdiv'
    else
        bad 'RR64 division is still an fdiv' 'the float path was routed through the guard'
    fi

    ir=$("$fortressc" "$repo/Documentation/Specification/Code/HelloWorld.fss" \
        --emit-ir 2>/dev/null)
    if grep -q 'call .*@fortress_div' <<<"$ir"; then
        bad 'a program that does not divide calls no guard' 'it does'
    else
        ok 'a program that does not divide calls no guard'
    fi
}

# The other thing the driver refuses that nothing had ever asked it to refuse.
# An unrecognised CPU must be a diagnostic, because LLVM's own behaviour is to
# warn and then quietly build for the baseline.
target_cpu() {
    printf '== --target-cpu ==\n'
    local err status
    err=$("$fortressc" "$repo/fortressc/tests/divquotients.fss" --emit-obj -o /dev/null \
        --target-cpu definitely-not-a-cpu 2>&1 >/dev/null)
    status=$?
    if halted_cleanly "$status" && [[ $err == *"unknown target CPU"* && $err == *"x86-64-v3"* ]]; then
        ok 'an unknown --target-cpu is refused and the accepted list is named'
    else
        bad 'an unknown --target-cpu is refused' "status $status: $err"
    fi

    if "$fortressc" "$repo/fortressc/tests/divquotients.fss" --emit-obj -o /dev/null \
        --target-cpu skylake-avx512 2>/dev/null; then
        ok 'a supported --target-cpu is accepted'
    else
        bad 'a supported --target-cpu is accepted' 'skylake-avx512 was refused'
    fi
}

# ----------------------------------------------------------------- mutations
#
# Each entry is file|from|to|label. Every `from` matches exactly once, and no
# `from` or `to` contains a `|`, because the table is split on IFS='|'.

MUTATIONS=(
  'runtime/shims.c|if (b == 0) {|if (0) {|drop the zero-divisor guard'
  'runtime/shims.c|if (a == LLONG_MIN && b == -1) {|if (0) {|drop the ZZ64 overflow guard'
  'runtime/shims.c|if (a == INT_MIN && b == -1) {|if (0) {|drop the ZZ32 overflow guard'
  'runtime/shims.c|    fflush(NULL);|    (void)0;|halt without flushing what the program already printed'
  'crates/types/src/lib.rs|if left.ty.is_integer() && right.kind == TypedExprKind::IntConst(0) {|if false {|let a literal zero divisor through to LLVM'
  'crates/codegen/src/lib.rs|  "fortress_div_zz64"|  "fortress_div_zz32"|send ZZ64 division to the 32 bit shim'
  'crates/driver/src/main.rs|if !fortress_codegen::SUPPORTED_CPUS.contains(&options.cpu.as_str()) {|if false {|accept any --target-cpu'
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
    # Against HEAD, not against the index: a restore from the index will
    # faithfully put a defect back if anything staged mid-run.
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
            broken=$((broken + 1))
            continue
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
            printf 'FAIL  the mutated compiler does not build\n'
            broken=$((broken + 1))
        else
            rm -rf "$build"; mkdir -p "$build"
            passed=0; failed=0
            quotients; halts; literal_zero; ir_shape; target_cpu
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
    --selftest)
        selftest
        ;;
    --mutate)
        selftest
        preflight
        mutate
        exit $?
        ;;
    *)
        selftest
        preflight
        quotients
        halts
        literal_zero
        ir_shape
        target_cpu
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

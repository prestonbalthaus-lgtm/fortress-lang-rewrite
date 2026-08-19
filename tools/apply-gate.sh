#!/usr/bin/env bash
#
# The M3f gate: juxtaposition as function application, and chained comparison.
#
# Six things cargo cannot check on its own: that `println "Hello"` becomes a
# real ELF that prints the right bytes, that a parameter shadowing a function
# name is NOT application, that a singleton object is a value and not a
# constructor, that a three-element juxtaposition halts with exit 1 rather than
# 70, that a chain evaluates its middle operand exactly once, and that a chain
# mixing two ordering senses is refused by name.
#
# It also carries this milestone's headline number. The parser corpus test stops
# at the parser and cannot see the compile metric at all, so the gate sweeps all
# 1956 corpus files with the real driver and fails if the count drops or if any
# file exits anything but 0 or 1.
#
#   ./tools/apply-gate.sh              run the gate
#   ./tools/apply-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/apply-gate.sh --mutate     break the compiler four ways and prove
#                                      the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build
fortressc=$repo/fortressc/target/debug/fortressc
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# Measured 2026-08-19 at the end of M3h, not taken from the design document.
# M3f left this at 187. M3h's bundle parses 138 more files and 18 of them go all
# the way through, with the component-level value bindings among them refused
# rather than counted -- see the M3h design note.
COMPILE_FLOOR=205

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# A diagnostic is exit 1 and nothing else. 70 is EXIT_INTERNAL_ERROR, 101 is a
# Rust panic, 139 is SIGSEGV: all three mean the compiler broke rather than
# reported. 0 means it accepted a program it should have refused.
refused_cleanly() { [[ $1 -eq 1 ]]; }

# The middle operand of a chain must run exactly once. Counting is the whole
# assertion, so it is its own function and it is self tested.
occurrences() { grep -c -F -- "$2" <<<"$1"; }

selftest() {
    printf '== gate self test ==\n'

    if refused_cleanly 1; then
        ok 'exit 1 is a clean refusal'
    else
        bad 'exit 1 is a clean refusal'
    fi
    for status in 0 70 101 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" \
                'only exit 1 is a diagnostic; the rest are compiler bugs'
        else
            ok "status $status is refused as a clean refusal"
        fi
    done

    local sample
    sample=$'MID\nYES'
    if [[ $(occurrences "$sample" MID) -eq 1 ]]; then
        ok 'one MID counts as one'
    else
        bad 'one MID counts as one'
    fi
    sample=$'MID\nMID\nYES'
    if [[ $(occurrences "$sample" MID) -eq 2 ]]; then
        ok 'two MIDs count as two'
    else
        bad 'two MIDs count as two' 'the counter cannot see a duplicated operand'
    fi
}

# ------------------------------------------------------------------ the gate

preflight() {
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    rm -rf "$build"
    mkdir -p "$build"
}

runs_and_prints() {
    printf '== programs that run ==\n'
    local name want label out status
    while IFS='|' read -r name want label; do
        [[ -z $name ]] && continue
        if ! "$fortressc" "$repo/fortressc/tests/$name.fss" -o "$build/$name" \
            2>"$build/$name.err"; then
            bad "$label" "$(cat "$build/$name.err")"
            continue
        fi
        out=$("$build/$name" 2>&1)
        status=$?
        if [[ $status -eq 0 && $out == "$(printf '%b' "$want")" ]]; then
            ok "$label"
        else
            bad "$label" "status $status: $out"
        fi
    done <<'CASES'
juxtapply|Hello\n42|`println "Hello"` and `double 21` are applications
juxtshadow|12|a parameter shadowing a function name stays multiplication
juxtnullary|42|`answer ()` is the zero-argument call
chainmixed|YES|a chain mixes equivalence with one ordering sense
rr64literal|1.75|an integer literal in RR64 position is a float constant
CASES
}

evaluated_once() {
    printf '== a chain evaluates its middle operand once ==\n'
    if ! "$fortressc" "$repo/fortressc/tests/chainonce.fss" -o "$build/chainonce" \
        2>"$build/chainonce.err"; then
        bad 'chainonce.fss compiles' "$(cat "$build/chainonce.err")"
        return
    fi
    local out count
    out=$("$build/chainonce" 2>&1)
    count=$(occurrences "$out" MID)
    if [[ $count -eq 1 ]]; then
        ok 'the middle operand ran once'
    else
        bad 'the middle operand ran once' "it ran $count times: $out"
    fi
    if [[ $out == *YES* ]]; then
        ok 'the chain is true'
    else
        bad 'the chain is true' "$out"
    fi
}

# Four refusals, and the PHRASE is the assertion rather than the exit code:
# every one of these is exit 1 with and without the code under test, and only
# the message distinguishes them.
refusals() {
    printf '== the refusals ==\n'
    local name phrase err status
    while IFS='|' read -r name phrase; do
        [[ -z $name ]] && continue
        err=$("$fortressc" "$repo/fortressc/tests/$name.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
        status=$?
        if refused_cleanly "$status" && [[ $err == *"$phrase"* ]]; then
            ok "$name.fss is refused (exit $status)"
        else
            bad "$name.fss is refused" "status $status: $err"
        fi
    done <<'CASES'
juxtnary|a juxtaposition of 3 elements led by a function is not implemented
juxtsingleton|neither multiplication nor concatenation
localfn|a local function declaration is not implemented
badchainsense|chained ordering operators must have the same sense
badvaluebinding|a component-level value declaration is parsed but not implemented
CASES
}

# The milestone's headline number, and the first time it has been guarded. The
# parser corpus test stops at the parser and cannot see this at all.
compile_metric() {
    printf '== the compile metric ==\n'
    local report compiled broken
    report=$(cd "$repo" && python3 - <<'PY'
import os, subprocess, collections
files = []
for d, ds, fs in os.walk('.'):
    ds[:] = [x for x in ds if x not in ('.git', 'target', 'fortressc')]
    files += [os.path.join(d, f) for f in fs if f.endswith(('.fss', '.fsi'))]
files.sort()
c = collections.Counter()
for p in files:
    r = subprocess.run(['fortressc/target/debug/fortressc', p, '--emit-obj', '-o', '/dev/null'],
                       capture_output=True, text=True)
    c[r.returncode] += 1
print(c[0], sum(n for code, n in c.items() if code not in (0, 1)))
PY
)
    read -r compiled broken <<<"$report"
    if [[ ${compiled:-0} -ge $COMPILE_FLOOR ]]; then
        ok "$compiled corpus files compile end to end (floor $COMPILE_FLOOR)"
    else
        bad "${compiled:-0} corpus files compile end to end" "floor is $COMPILE_FLOOR"
    fi
    if [[ ${broken:-1} -eq 0 ]]; then
        ok 'no corpus file makes the compiler crash or report an internal error'
    else
        bad 'no corpus file makes the compiler crash' "$broken did"
    fi
}

# ----------------------------------------------------------------- mutations
#
# Each entry is file|from|to|label. Every `from` must match exactly once in its
# file, and the tree has to be clean first. Restored either way.

MUTATIONS=(
  'crates/types/src/lib.rs|if self.lookup(name).is_some() {|if false {|drop the shadowing guard on a function element'
  'crates/parser/src/lib.rs|if is_literal(operand) {|if true {|duplicate every chain operand instead of binding it'
  'crates/parser/src/lib.rs|Some((seen, earlier)) if seen != this => {|Some((seen, earlier)) if false => {|drop the chain sense check'
  'crates/parser/src/lib.rs|&& self.glued_left(self.pos + 1)|&& false|drop the local function declaration guard'
  'crates/types/src/lib.rs|if f.value_binding {|if false {|carry a component-level value binding as a nullary function'
)

mutate() {
    if ! git -C "$repo" diff --quiet -- fortressc/crates; then
        printf 'refusing to mutate: fortressc/crates has unstaged changes\n' >&2
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
            runs_and_prints; evaluated_once; refusals
            if [[ $failed -gt 0 ]]; then
                printf 'REFUSED  %d check(s) failed, which is the point\n' "$failed"
            else
                printf 'SURVIVED %s -- the gate did not notice\n' "$label"
                survived=$((survived + 1))
            fi
        fi
        git -C "$repo" checkout -- "fortressc/$file"
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
        runs_and_prints
        evaluated_once
        refusals
        compile_metric
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

#!/usr/bin/env bash
#
# The tuple gate. READ THIS BEFORE ASSUMING IT GATES TUPLES.
#
# TUPLES ARE NOT IMPLEMENTED. `Type` (crates/types/src/types.rs) is still the
# nine-variant `Copy` enum with no composite case, and `Expr::Tuple` is refused
# by name. So what this gate pins is the REFUSAL: that a tuple is refused at
# every position it can appear in, cleanly, with a diagnostic that says which
# construct it is -- and not silently accepted, mis-parsed, or crashed on.
#
# THAT IS WORTH GATING ON ITS OWN. `04-state`'s own record has a construct that
# parsed and was silently accepted (`[1 2 3]` yields ONE element holding 6), and
# section 5 of the gap analysis is a list of forms that "parse, type-check clean,
# and mean nothing". A refusal is a contract like any other, and this one is
# load bearing: `overloading.tex:124-126` makes a functional's parameter a value
# that MAY BE A TUPLE, so M3c's dispatch is already written against a world
# where tuples do not exist.
#
# THIS GATE IS DESIGNED TO GO RED WHEN TUPLES LAND. That is not a bug. When
# SPIKE-COMPOSITE-TYPE ships and `Type` stops being `Copy`, every assertion here
# fails at once, and the person who landed it converts this file into a real
# tuple gate -- positive fixtures, an IR shape, a mutation table -- instead of
# discovering months later that nothing ever checked tuples. The failure message
# says so.
#
#   ./tools/tuple-gate.sh              run the gate
#   ./tools/tuple-gate.sh --selftest   only prove the assertions can refuse
#
# There is no --mutate. A mutation table breaks the compiler and proves the gate
# notices; here the ONLY thing to break is the refusal itself, which is what
# `--selftest`'s negative cases already cover and what landing tuples will do
# for real. Adding a table that deletes a diagnostic to watch a refusal stop
# would assert nothing this file does not already assert.
#
# FORTRESSC pins the binary. KEEP THE PINNED COPY OUTSIDE fortressc/build/.
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build/tuple
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# Measured over the whole corpus with tools/triage.sh at the M6 merge. The
# number is here so that when it MOVES, someone asks why -- it is the size of
# what tuples unlock and the reason SPIKE-COMPOSITE-TYPE is sequenced where it
# is (D6 makes it sub-phase 4a's exit condition).
TUPLE_FIRST_BLOCKERS=35

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

refused_cleanly() { [[ $1 -eq 1 ]]; }
names_mechanism() { grep -q -F -- "$2" <<<"$1"; }

# <label> <expected substring> <body>
probe() {
    mkdir -p "$build"
    local label=$1 want=$2 body=$3 err rc
    printf 'component t\nexport Executable\n%s\nend\n' "$body" > "$build/t.fss"
    err=$("$fortressc" "$build/t.fss" --emit-obj -o /dev/null 2>&1 >/dev/null); rc=$?
    if refused_cleanly $rc; then
        ok "$label is refused cleanly"
    else
        bad "$label is refused cleanly" \
            "exit $rc -- if tuples have LANDED, this gate has done its job: rewrite it as a real tuple gate"
    fi
    if names_mechanism "$err" "$want"; then
        ok "$label -- the diagnostic names the construct"
    else
        bad "$label -- the diagnostic names the construct" "got: $(head -1 <<<"$err")"
    fi
}

selftest() {
    printf '== gate self test ==\n'
    if refused_cleanly 1; then ok 'exit 1 is a clean refusal'
    else bad 'exit 1 is a clean refusal'; fi
    for status in 0 70 101 139; do
        if refused_cleanly "$status"; then
            bad "status $status reads as a clean refusal" \
                'accepting a tuple, or crashing on one, must both be red'
        else
            ok "status $status is not a clean refusal"
        fi
    done
    if names_mechanism 'a tuple type is not implemented' 'a tuple type'; then
        ok 'names_mechanism finds its substring'
    else bad 'names_mechanism finds its substring'; fi
    if names_mechanism 'a tuple type is not implemented' 'a tuple expression'; then
        bad 'names_mechanism confuses a type with an expression' \
            'the two diagnostics are different and the gate distinguishes them'
    else ok 'a type diagnostic does not satisfy an expression assertion'; fi
    if names_mechanism 'expected `)`, found Comma' 'a tuple'; then
        bad 'a generic parse error reads as a tuple diagnostic'
    else ok 'a generic parse error does not read as a tuple diagnostic'; fi
    printf '\n%d passed, %d failed\n' "$passed" "$failed"
    [[ $failed -eq 0 ]]
}

run_gate() {
    printf '== tuples: the refusal, at every position one can appear ==\n'
    printf '   TUPLES ARE NOT IMPLEMENTED. This gate pins the refusal, and it is\n'
    printf '   MEANT to go red the day they land. See the header.\n\n'

    probe 'a tuple TYPE in a parameter' 'a tuple type' \
'f(p: (ZZ32, ZZ32)): ZZ32 = 1
run(): () = println(1)'

    probe 'a tuple TYPE as a return type' 'a tuple type' \
'f(x: ZZ32): (ZZ32, ZZ32) = x
run(): () = println(1)'

    probe 'a tuple TYPE as a static argument' 'a tuple type' \
'f[\T\](x: T): ZZ32 = 1
run(): () = println(f[\(ZZ32, ZZ32)\](1))'

    probe 'a tuple EXPRESSION' 'a tuple expression' \
'run(): () = do
  t = (1, 2)
  println(1)
end'

    probe 'a tuple BINDER' 'a tuple expression' \
'run(): () = do
  (a, b) = (1, 2)
  println(a)
end'

    # A PARENTHESISED SINGLE EXPRESSION IS NOT A TUPLE, and confusing the two
    # would make the refusal above fire on ordinary code. This is the assertion
    # that keeps the others honest.
    mkdir -p "$build"
    printf 'component t\nexport Executable\nrun(): () = println((1 + 2))\nend\n' \
        > "$build/paren.fss"
    if "$fortressc" "$build/paren.fss" -o "$build/paren" >/dev/null 2>&1; then
        out=$("$build/paren" 2>&1)
        if [[ $out == 3 ]]; then ok 'a parenthesised expression is NOT a tuple and still runs'
        else bad 'a parenthesised expression is NOT a tuple and still runs' "printed $out"; fi
    else
        bad 'a parenthesised expression compiles' \
            'the tuple refusal is firing on ordinary parentheses'
    fi

    # The `Type` representation is the reason all of the above is true, so the
    # gate says so out loud rather than leaving it implicit.
    if grep -q 'pub enum Type {' "$repo/fortressc/crates/types/src/types.rs" &&
       ! grep -q 'Tuple' "$repo/fortressc/crates/types/src/types.rs"; then
        ok '`Type` still has no tuple variant -- the refusal is structural'
    else
        bad '`Type` still has no tuple variant' \
            'if a Tuple variant exists, tuples are landing: rewrite this gate'
    fi

    printf '\n%d passed, %d failed\n' "$passed" "$failed"
    printf 'tuple first-blockers in the corpus at the last measurement: %s\n' \
        "$TUPLE_FIRST_BLOCKERS"
    [[ $failed -eq 0 ]]
}

case ${1:-} in
    --selftest) selftest ;;
    '')         if [[ ! -x $fortressc ]]; then
                    printf 'no compiler at %s -- cargo build first\n' "$fortressc" >&2
                    exit 2
                fi
                run_gate ;;
    *)          printf 'unknown argument %s\n' "$1" >&2; exit 2 ;;
esac

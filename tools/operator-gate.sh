#!/usr/bin/env bash
#
# The M3k gate: primitive operators and builtins.
#
# Five things cargo cannot check on its own: that AND and OR really SHORT
# CIRCUIT rather than merely returning the right answer, that the branch they
# short circuit on is in the emitted module, that `^` is left associative and
# binds above juxtaposition, that a negative integer exponent halts cleanly
# instead of inventing a number, and that a failed `assert` stops the program.
#
# The short-circuit assertion is the reason this gate exists. `true AND false`
# is false whichever way it is evaluated, so a truth table alone cannot tell a
# short-circuit AND from a strict one. The witness is a right operand that
# PRINTS: its output missing is the only observable difference.
#
#   ./tools/operator-gate.sh              run the gate
#   ./tools/operator-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/operator-gate.sh --mutate     break the compiler six ways and prove
#                                         the gate refuses each one
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

# A diagnostic is exit 1 and nothing else. 70 is EXIT_INTERNAL_ERROR, 101 is a
# Rust panic, 139 is SIGSEGV.
refused_cleanly() { [[ $1 -eq 1 ]]; }

# The whole short-circuit assertion is a count of zero, so counting is its own
# function and it is self tested.
occurrences() { grep -c -F -- "$2" <<<"$1"; }

# The truth table, COMPUTED here rather than read out of the program under
# test. A gate that takes its expected values from the thing it is testing is
# only checking self-consistency.
truth_table() {
    local a b
    for a in true false; do
        for b in true false; do
            if [[ $a == true && $b == true ]]; then printf 'true\n'; else printf 'false\n'; fi
        done
    done
}

selftest() {
    printf '== gate self test ==\n'

    if refused_cleanly 1; then
        ok 'exit 1 is a clean refusal'
    else
        bad 'exit 1 is a clean refusal'
    fi
    local status
    for status in 0 70 101 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" \
                'only exit 1 is a diagnostic; the rest are compiler bugs'
        else
            ok "status $status is refused as a clean refusal"
        fi
    done

    local sample
    sample=$'true\nfalse'
    if [[ $(occurrences "$sample" RHS) -eq 0 ]]; then
        ok 'no RHS counts as zero'
    else
        bad 'no RHS counts as zero'
    fi
    sample=$'RHS\ntrue'
    if [[ $(occurrences "$sample" RHS) -eq 1 ]]; then
        ok 'one RHS counts as one'
    else
        bad 'one RHS counts as one' 'the counter cannot see the right operand running'
    fi

    local computed
    computed=$(truth_table | tr '\n' ' ')
    if [[ $computed == 'true false false false ' ]]; then
        ok "the gate computes AND's truth table itself: $computed"
    else
        bad 'the gate computes the truth table' "got: $computed"
    fi
}

preflight() {
    printf '== preflight ==\n'
    if [[ -x $fortressc ]]; then
        ok 'the compiler is built'
    else
        bad 'the compiler is built' "no binary at $fortressc"
        exit 1
    fi
    rm -rf "$build"
    mkdir -p "$build"
}

compile() {
    printf '== compile ==\n'
    local name
    for name in logical exponent builtins; do
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

logic() {
    printf '== logical operators ==\n'
    have "$build/logical" 'AND, OR and NOT compute what they should' || return

    local out first four
    out=$("$build/logical" 2>&1)
    # Lines 1..4 are AND and OR over the four rows; the first four of them are
    # what the computed table has to match.
    first=$(printf '%s\n' "$out" | sed -n '1p;2p')
    four=$(truth_table | sed -n '1p;2p')
    if [[ $first == "$four" ]]; then
        ok "AND agrees with the table the gate computed: $(printf '%s' "$first" | tr '\n' ' ')"
    else
        bad 'AND agrees with the computed truth table' "want $four, got $first"
    fi

    local want
    want=$(printf 'true\nfalse\ntrue\nfalse\nfalse\ntrue\nfalse\ntrue\nfalse\ntrue\ntrue\n')
    if [[ $out == "$want" ]]; then
        ok 'every logical line is what it should be'
    else
        bad 'every logical line is what it should be' \
            "want: $(printf '%s' "$want" | tr '\n' ' ') | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi

    # THE assertion. `false AND loud()` and `true OR loud()` must never run
    # their right operand, and `loud` prints when it runs.
    if [[ $(occurrences "$out" RHS) -eq 0 ]]; then
        ok 'AND and OR never evaluate a right operand they do not need'
    else
        bad 'AND and OR short circuit' \
            "the right operand ran $(occurrences "$out" RHS) time(s)"
    fi

    # And the shape it comes from: a conditional branch and a phi, not a select.
    local ir
    ir=$("$fortressc" "$repo/fortressc/tests/logical.fss" --emit-ir 2>/dev/null)
    if [[ $ir == *"br i1"* && $ir == *"phi i1"* ]]; then
        ok 'the short circuit is a conditional branch and a phi in the module'
    else
        bad 'the short circuit is a branch and a phi' 'no br/phi over i1 emitted'
    fi
    if [[ $ir != *"select i1"* ]]; then
        ok 'no select: a select would evaluate both operands'
    else
        bad 'no select is emitted' 'a select evaluates both sides'
    fi
}

exponent() {
    printf '== exponentiation ==\n'
    have "$build/exponent" 'the exponent operator computes what it should' || return

    local out want
    out=$("$build/exponent" 2>&1)
    # 2^3^2 is 64 under LEFT association and 512 under right. Stating both is
    # the point: the number is what distinguishes them.
    # THE LAST ONE IS `256.0` AND NOT `256`: it is an RR64, and an RR64 shows
    # that it is one. It read `256` until `rr64_needs_point` landed in
    # runtime/shims.c -- C's "%g" drops a trailing ".0", which
    # compiler_tests/Compiled7.Print17.fss asserts is wrong. Two Rust tests
    # pinned the same old answer and this gate was the third.
    want=$(printf '1024\n64\n18\n18\n5\n0.00390625\n256\n0.00390625\n256.0\n')
    if [[ $out == "$want" ]]; then
        ok "left associative, above juxtaposition, all four pairs: $(printf '%s' "$out" | tr '\n' ' ')"
    else
        bad 'the exponent operator computes what it should' \
            "want: $(printf '%s' "$want" | tr '\n' ' ') | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
    if [[ $(printf '%s\n' "$out" | sed -n '2p') == 64 ]]; then
        ok '2^3^2 is 64, so the group is left associative and not right'
    else
        bad '2^3^2 is 64' 'right association would give 512'
    fi
}

builtins() {
    printf '== the builtins ==\n'
    have "$build/builtins" 'print, ignore and assert do what they should' || return

    local out want
    out=$("$build/builtins" 2>&1)
    want=$(printf 'ab1true\nSIDE\ndone\n')
    if [[ $out == "$want" ]]; then
        ok 'print writes no newline, ignore still evaluates, asserts pass'
    else
        bad 'print, ignore and assert' \
            "want: $(printf '%s' "$want" | tr '\n' ' ') | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

halts() {
    printf '== the halts ==\n'
    local binary out status
    for entry in \
        "negexponent|negative exponent has no integer result|a negative integer exponent" \
        "assertfail|assertion failed|a failed assert"; do
        IFS='|' read -r name phrase label <<<"$entry"
        if ! "$fortressc" "$repo/fortressc/tests/$name.fss" -o "$build/$name" 2>/dev/null; then
            bad "$label halts cleanly" "$name.fss does not compile"
            continue
        fi
        out=$("$build/$name" 2>&1)
        status=$?
        if refused_cleanly "$status" && [[ $out == *"$phrase"* ]]; then
            ok "$label halts with a diagnostic and exit 1"
        else
            bad "$label halts cleanly" "status $status: $out"
        fi
    done
}

refusals() {
    printf '== the refusals ==\n'
    local name phrase label err status
    while IFS='|' read -r name phrase label; do
        err=$("$fortressc" "$repo/fortressc/tests/$name.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
        status=$?
        if refused_cleanly "$status" && [[ $err == *"$phrase"* ]]; then
            ok "$label (exit $status)"
        else
            bad "$label" "status $status: $err"
        fi
    done <<'CASES'
badlogical|`AND` takes Boolean operands|a non-Boolean operand is refused against the operator
badnot|`NOT` takes Boolean operands|NOT binds tighter than `<`, and the refusal says so
badassert|which is not defined on String|assert is only as strong as `=` is
CASES
}

# ----------------------------------------------------------------- mutations
#
# Each entry is file|from|to|label. Every `from` must match exactly once in its
# file, the tree has to match HEAD first, and it is restored from HEAD either
# way. No `from` or `to` may contain a `|`: the table is split on IFS='|'.

MUTATIONS=(
  'crates/types/src/lib.rs|(right, constant(false))|(constant(false), right)|swap the arms of the AND desugaring'
  'crates/types/src/lib.rs|(constant(true), right)|(right, constant(true))|swap the arms of the OR desugaring'
  'crates/parser/src/lib.rs|let exponent = self.primary()?;|let exponent = self.postfix()?;|make the exponent right associative'
  # RE-TARGETED at the three-lane merge. This row used to disable
  # `word_operator_here`, and that mutation SURVIVED -- not because the gate went
  # blind but because the property moved. `AND` and `OR` were `Ident` to the lexer
  # when the row was written; the frontend lane's operator-word rule lexes them as
  # `OpWord`, which `starts_juxt_operand`'s arm list does not carry, so the run
  # stops at one whether that guard fires or not. What models the defect NOW is
  # letting an `OpWord` start an operand again. The row cannot say so in the arm
  # list itself: those arms are separated by `|` and the table is split on it.
  'crates/parser/src/lib.rs|fn starts_juxt_operand(&self) -> bool {|fn starts_juxt_operand(&self) -> bool { if matches!(self.peek_kind(), Some(Kind::OpWord(_))) { return true; }|let a word operator be swallowed by juxtaposition again'
  'runtime/shims.c|if (b < 0) {|if (0) {|let a negative exponent return a number'
  'crates/types/src/lib.rs|target: Target::AssertFailed,|target: Target::Println { ty: Type::String },|let a failed assert print and carry on'
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
    # Against HEAD, not against the index, and the restore below matches. A
    # gate that rewinds to the index will faithfully put a DEFECT back if
    # anything staged during the run.
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
            compile >/dev/null 2>&1
            logic; exponent; builtins; halts; refusals
            if [[ $failed -gt 0 ]]; then
                printf 'REFUSED  %d check(s) failed, which is the point\n' "$failed"
            else
                printf 'SURVIVED  the gate did not notice\n'
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
        compile
        logic
        exponent
        builtins
        halts
        refusals
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

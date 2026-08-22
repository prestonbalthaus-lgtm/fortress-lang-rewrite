#!/usr/bin/env bash
#
# The dimension gate: sub-phase 4d, rung one.
#
# `dim` and `unit` declarations PARSE, REGISTER and are CHECKED. Nothing above
# that rung is built, and the gate's job is to prove that what is not built is
# REFUSED BY NAME rather than silently accepted -- because
# `dimensions.tex:206-215` makes a unit mismatch a STATIC error, so a
# parse-and-erase implementation would compile `meter + second` at exit 0.
# That is the wrong-acceptance class this project already hunts.
#
# The sharpest assertion here is the seventh: `in` was an ordinary identifier,
# so `println(x in nm)` over three RR64 bindings was a three-way juxtaposition
# PRODUCT and printed 7.8, at exit 0, with no diagnostic anywhere. Reserving
# the seven unit operators fixed a live wrong answer, and mutation 6 brings it
# back on demand.
#
#   ./tools/dimension-gate.sh              run the gate
#   ./tools/dimension-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/dimension-gate.sh --mutate     break the compiler six ways and
#                                          prove the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build/dimension-gate
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# The seven words `dimensions.tex:32` and `:49-54` make operators. Listed here
# rather than read from the lexer, so the gate and the lexer are two sources
# that have to agree.
UNIT_OPERATORS=(in per square squared cubic cubed inverse)

refused_cleanly() { [[ $1 -eq 1 ]]; }

selftest() {
    printf '== gate self test ==\n'
    if [[ ${#UNIT_OPERATORS[@]} -eq 7 ]]; then
        ok 'seven unit operators are named'
    else
        bad 'seven unit operators are named' "${#UNIT_OPERATORS[@]}"
    fi
    if refused_cleanly 1; then
        ok 'exit 1 is a clean refusal'
    else
        bad 'exit 1 is a clean refusal'
    fi
    local status
    for status in 0 70 124 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" \
                '0 compiled it, 70 is a compiler bug, 124 hung, 139 faulted'
        else
            ok "status $status is refused as a clean refusal"
        fi
    done
}

preflight() {
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    rm -rf "$build"
    mkdir -p "$build"
}

# Every declaration form on one file: base and derived dimensions, the bundled
# `dim ... SI_unit ...`, an alias list, a `: Dim` annotation, a definition, the
# `per` and `square` sugar, and a `unit` static parameter with `absorbs unit`.
#
# AND THE TWO FEATURES TOGETHER, which is the assertion that answers "do
# dimensions break array types". `dim Area = Length^2` and `a: ZZ32[5]` share
# the caret and bracket suffix production, so the fixture writes both and RUNS.
declaring() {
    printf '== every declaration form parses, registers and runs ==\n'
    if ! timeout 300 "$fortressc" "$repo/fortressc/tests/dimensions.fss" \
        -o "$build/dimensions" >"$build/build.log" 2>&1; then
        bad 'dimensions.fss compiles and links' "$(cat "$build/build.log")"
        return
    fi
    ok 'dimensions.fss compiles and links'

    local out status
    out=$("$build/dimensions" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == $'4\n2.5\n7\ndimensions declared' ]]; then
        ok 'it runs, exits 0, and its array types work alongside its dimensions'
    else
        bad 'it runs and exits 0' "status $status: $out"
    fi
}

# A DIMENSION IS ERASED, AND THE PROOF IS THAT IT WAS NEVER THERE. Rung one
# admits no dimensioned value at all, so a component that declares dimensions
# must emit exactly what the same component without them emits.
erasure() {
    printf '== declarations reach the emitted module in no form ==\n'
    cat >"$build/withdims.fss" <<'EOF'
component withdims
export Executable
dim Length SI_unit meter meters m_
unit inch inches: Length
run() = println("x")
end
EOF
    cat >"$build/without.fss" <<'EOF'
component without
export Executable
run() = println("x")
end
EOF
    local a b
    a=$(timeout 300 "$fortressc" "$build/withdims.fss" --emit-ir -o /dev/null 2>/dev/null | grep -v '^; ModuleID' | grep -v 'source_filename')
    b=$(timeout 300 "$fortressc" "$build/without.fss" --emit-ir -o /dev/null 2>/dev/null | grep -v '^; ModuleID' | grep -v 'source_filename')
    if [[ -n $a && $a == "$b" ]]; then
        ok 'the IR is identical with and without the declarations'
    else
        bad 'the IR is identical with and without the declarations' \
            "$(diff <(printf '%s' "$a") <(printf '%s' "$b") | head -5 | tr '\n' ' ')"
    fi
}

# Each of these is a program the compiler must REFUSE, and by a diagnostic that
# NAMES THE MECHANISM. Asserting the exit code alone would pass on `unknown
# type` or on a parse error, which is what every one of these reported before
# this rung and which sends the reader to the wrong place.
refusals() {
    printf '== the refusals ==\n'
    local name pattern label err status
    for entry in \
        "baddimname|is not a declared dimension|a derivation over an undeclared name is refused" \
        "baddimtype|is a dimension, not a type|a dimension used as a type says which it is" \
        "baddimdup|is declared twice|a dimension is declared once" \
        "baddimcollide|separate namespaces|a name is a dimension or a type, never both" \
        "baddiminstance|instantiating one is not implemented|a unit static parameter cannot be instantiated" \
        "badunitop|reserved word \`in\`|a unit operator is not an identifier"; do
        IFS='|' read -r name pattern label <<<"$entry"
        err=$(timeout 300 "$fortressc" "$repo/fortressc/tests/$name.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
        status=$?
        if refused_cleanly "$status" && [[ $err == *"$pattern"* ]]; then
            ok "$label (exit $status)"
        else
            bad "$label" "status $status: $err"
        fi
    done
}

# ALL SEVEN, not just the one with a fixture. Six of them cost nothing today
# and would each be a silent product tomorrow: the moment a `unit` declaration
# binds `m_`, `1.3 m_` starts multiplying instead of tagging a unit.
operators() {
    printf '== all seven unit operators are out of the identifier namespace ==\n'
    local word err status
    for word in "${UNIT_OPERATORS[@]}"; do
        cat >"$build/op.fss" <<EOF
component op
export Executable
run() = do
  $word: ZZ32 = 1
  println($word)
end
end
EOF
        err=$(timeout 300 "$fortressc" "$build/op.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
        status=$?
        if refused_cleanly "$status" && [[ $err == *"reserved word \`$word\`"* ]]; then
            ok "\`$word\` is reserved"
        else
            bad "\`$word\` is reserved" "status $status: $err"
        fi
    done
}

# --------------------------------------------------------------- mutations

MUTATIONS=(
  'crates/types/src/dimensions.rs|            resolve_names(derivation, &known.dims, "dimension")?;|            let _ = derivation;|stop resolving the names in a dimension derivation'
  'crates/types/src/registry.rs|if let Some(kind) = self.dimensions.describes(other) {|if let Some(kind) = None::<&str> {|let a dimension name reach the unknown-type diagnostic'
  'crates/types/src/dimensions.rs|if seen.insert(name, span).is_some() {|if false {|stop refusing a dimension declared twice'
  'crates/types/src/dimensions.rs|if declared.contains_key(dim.name.as_str()) {|if false {|let a dimension and a type share one name'
  'crates/types/src/mono.rs|if param.kind.is_dimensional() {|if false {|let a unit static parameter be instantiated'
  'crates/lexer/src/token.rs|_ if RESERVED.binary_search(&word).is_ok() => Kind::Reserved(word),|_ if word != "in" && RESERVED.binary_search(&word).is_ok() => Kind::Reserved(word),|make `in` an identifier again'
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
    # Against HEAD, not against the index, and the restore below matches.
    if ! git -C "$repo" diff --quiet HEAD -- fortressc/crates; then
        printf 'refusing to mutate: fortressc/crates differs from HEAD\n' >&2
        exit 2
    fi

    local entry file from to label hits status
    local survived=0 broken=0
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
            declaring >/dev/null 2>&1
            erasure >/dev/null 2>&1
            refusals
            operators >/dev/null 2>&1
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
        declaring
        erasure
        refusals
        operators
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

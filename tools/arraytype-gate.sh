#!/usr/bin/env bash
#
# The array-type gate: `T[n]`, `traits.tex:97-108`.
#
# Six things cargo cannot check on its own: that `a: ZZ32[5] = [0 1 2 3 4]`
# becomes a real ELF that prints the right elements, that a declared size and a
# literal's length are COMPARED rather than the size being dropped, that the
# five forms this subset does not build are each refused BY NAME rather than by
# a parse error naming the wrong mechanism, that `ZZ32[3]` and `Array[\ZZ32\]`
# lower to the SAME type, that an extent survives monomorphization's
# substitution so `g[\nat n\](a: ZZ32[n])` works, and that a SPACED bracket is
# still not an array size.
#
# The refusals are the point. `Type::Array` holds one `Elem` and `Elem` is a
# separate five-variant enum, so a second dimension is UNREPRESENTABLE rather
# than merely rejected -- one refusal in `Registry::resolve` keeps it that way,
# and this gate is what proves that refusal is still there.
#
#   ./tools/arraytype-gate.sh              run the gate
#   ./tools/arraytype-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/arraytype-gate.sh --mutate     break the compiler five ways and
#                                          prove the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build/arraytype-gate
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# What `arraytype.fss` must print, derived here from the fixture rather than
# copied out of a previous run: the last element of a five-element array, the
# second of a two-element one, the third element of the literal handed to `f`,
# the first of the one handed to `g`, and the length.
expected_output() { printf '4\n2.5\n9\n4\n5\n'; }

# A clean refusal: a diagnostic and exit 1. Anything else -- 0, or a signal --
# means the compiler did not decide, it fell over.
refused_cleanly() { [[ $1 -eq 1 ]]; }

selftest() {
    printf '== gate self test ==\n'
    if [[ $(expected_output) == $'4\n2.5\n9\n4\n5' ]]; then
        ok 'the expected output is the five values the fixture computes'
    else
        bad 'the expected output is the five values the fixture computes'
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

# The whole feature, end to end: a binding, a parameter, a `nat` extent behind
# monomorphization, an `RR64` element, and `length`.
running() {
    printf '== an array type runs at every position ==\n'
    if ! timeout 300 "$fortressc" "$repo/fortressc/tests/arraytype.fss" \
        -o "$build/arraytype" >"$build/build.log" 2>&1; then
        bad 'arraytype.fss compiles and links' "$(cat "$build/build.log")"
        return
    fi
    ok 'arraytype.fss compiles and links'

    local out status
    out=$("$build/arraytype" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == "$(expected_output)" ]]; then
        ok 'it prints the five values and exits 0'
    else
        bad 'it prints the five values and exits 0' "status $status: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

# TWO SPELLINGS OF ONE TYPE. If `ZZ32[3]` produced anything other than what
# `Array[\ZZ32\]` produces, every array builtin would have to learn about it.
one_type() {
    printf '== the two spellings lower to one type ==\n'
    cat >"$build/spellings.fss" <<'EOF'
component spellings
export Executable
f(a: ZZ32[3]): ZZ32 = a[0]
g(a: Array[\ZZ32\]): ZZ32 = a[0]
run() = ()
end
EOF
    local ir f g
    ir=$(timeout 300 "$fortressc" "$build/spellings.fss" --emit-ir -o /dev/null 2>/dev/null)
    f=$(printf '%s' "$ir" | grep -E '^define .*@f\(' | sed 's/@f/@X/')
    g=$(printf '%s' "$ir" | grep -E '^define .*@g\(' | sed 's/@g/@X/')
    if [[ -n $f && $f == "$g" ]]; then
        ok "ZZ32[3] and Array[\\ZZ32\\] give one signature: $f"
    else
        bad 'ZZ32[3] and Array[\ZZ32\] give one signature' "f=[$f] g=[$g]"
    fi
}

# Each of these is a program the compiler must REFUSE, and each must be refused
# by a diagnostic that NAMES THE MECHANISM. A gate asserting only the exit code
# would pass on `expected `)`, found LBracket`, which is what all five of these
# reported before this feature and which sends the reader to the wrong place.
refusals() {
    printf '== the refusals ==\n'
    local name pattern label err status
    for entry in \
        "badarrayextent|declared with 5 element(s) and 6 are written|a declared size must match the literal that fills it" \
        "badarraydims|has 2 dimensions; an array in this subset is one dimensional|a second dimension is refused by name" \
        "badextentrange|is an extent range|a hash extent range is refused by name" \
        "badarraysize|writes no size|an array type with no size is refused by name" \
        "badmatrixtype|a vector or matrix type is not implemented|a caret shape is refused by name" \
        "badstackedshape|found LBracket|a shape suffix may not be stacked"; do
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

# A size that is not a number by the time the checker sees it resolved to
# nothing. Saying so beats `unknown type `q``, which sends the reader looking
# for a declaration that was never meant to exist.
unresolved_size() {
    printf '== a size that resolved to nothing says so ==\n'
    cat >"$build/badsize.fss" <<'EOF'
component badsize
export Executable
run() = do
  a: ZZ32[q] = [1 2 3]
  println(a[0])
end
end
EOF
    local err status
    err=$(timeout 300 "$fortressc" "$build/badsize.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if refused_cleanly "$status" && [[ $err == *'is not a number, so it cannot be an array size'* ]]; then
        ok 'an unresolvable size names itself and not a missing type'
    else
        bad 'an unresolvable size names itself and not a missing type' "status $status: $err"
    fi
}

# THE GLUE RULE. A spaced bracket must not become an array size, or
# `x : ZZ32 [1,2,3]` silently changes what it means. Measured cost of requiring
# glue: zero, because all 62 corpus sites are glued.
glue() {
    printf '== a spaced bracket is not an array size ==\n'
    cat >"$build/spaced.fss" <<'EOF'
component spaced
export Executable
f(a: ZZ32 [3]): ZZ32 = a[0]
run() = ()
end
EOF
    local err status
    err=$(timeout 300 "$fortressc" "$build/spaced.fss" --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if refused_cleanly "$status"; then
        ok 'a spaced bracket does not parse as a size'
    else
        bad 'a spaced bracket does not parse as a size' "status $status: $err"
    fi
}

# --------------------------------------------------------------- mutations

MUTATIONS=(
  'crates/types/src/lib.rs|if declared_len == items.len() {|if true {|stop comparing a declared size with the literal that fills it'
  'crates/types/src/registry.rs|if spelling == ShapeSpelling::Caret {|if false {|let a caret shape resolve as an array'
  'crates/types/src/registry.rs|let [extent] = extents else {|let [extent, ..] = extents else {|let a multi dimensional array resolve as its first dimension'
  'crates/types/src/registry.rs|if !matches!(size, TypeRef::Static { .. }) {|if false {|let a size that resolved to nothing through'
  'crates/parser/src/lib.rs|let glued_bracket = self.at(&Kind::LBracket) && self.glued_left(self.pos);|let glued_bracket = self.at(&Kind::LBracket);|let a spaced bracket be an array size'
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
            running >/dev/null 2>&1
            one_type >/dev/null 2>&1
            refusals
            unresolved_size
            glue
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
        running
        one_type
        refusals
        unresolved_size
        glue
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

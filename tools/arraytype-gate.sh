#!/usr/bin/env bash
#
# The array-type gate: `T[n]` and `T[m,n]`, `traits.tex:97-108`.
#
# Nine things cargo cannot check on its own: that `a: ZZ32[5] = [0 1 2 3 4]`
# becomes a real ELF that prints the right elements, that a declared size and a
# literal's length are COMPARED rather than the size being dropped, that the
# forms this subset does not build are each refused BY NAME rather than by a
# parse error naming the wrong mechanism, that `ZZ32[3]` and `Array[\ZZ32\]`
# lower to the SAME type, that an extent survives monomorphization's
# substitution so `g[\nat n\](a: ZZ32[n])` works, that a SPACED bracket is
# still not an array size, that a HIGHER RANK is filled and read back through
# the right slots, that a subscript is bounds checked in EVERY dimension, and
# that a MATRIX AGGREGATE puts its elements where the specification says.
#
# THE AGGREGATE CHECK ASSERTS A VALUE AND NOT A SHAPE, which is the only way to
# see a transposed linearisation: `aggregate.tex:143-150` says that for
# `A: ZZ32[3,3] = [1 2 3; 4 5 6; 7 8 9]`, "then A(1,0) evaluates to 4". A gate
# comparing extents would pass with rows and columns swapped.
#
# THE NON-SQUARE FIXTURE IS THE POINT OF THE RANK-TWO CHECK. A wrong stride in
# the linearisation collides two subscripts onto one slot, and a 3 by 3 hides
# it: the mutation that multiplies by `extents[0]` instead of `extents[d]`
# prints `0 1 10 10 11 12` on the 2 by 3 here and the RIGHT answer on a square
# one. And the per-dimension bound is a correctness requirement rather than a
# nicety -- `a[0,4]` on a 2 by 3 linearises to 4, which is inside six, so a
# check made after the linearisation returns `a[1,1]` at exit 0.
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
    # `entry` and `label` are LOCAL because this function is called from inside
    # the mutation loop, which iterates a variable of each name. Without it a
    # SURVIVED line names a CHECK instead of the mutation that survived -- which
    # is what generics-gate did, and it cost a session's worth of misreading.
    local entry name pattern label err status
    for entry in \
        "badarrayextent|declared with 5 element(s) and 6 are written|a declared size must match the literal that fills it" \
        "badsubscriptfew|a rank 2 array takes 2 subscript(s), found 1|too few subscripts is refused by name" \
        "badsubscriptmany|a rank 1 array takes 1 subscript(s), found 2|too many subscripts is refused by name" \
        "badarrayrank|\`length\` of a rank 2 array is not in this subset|a rank-one operation above rank one is refused by name" \
        "badarraynewarity|\`array\` takes 2 argument(s), found 1|one constructor argument per dimension" \
        "badextentrange|is an extent range|a hash extent range is refused by name" \
        "badarraysize|writes no size|an array type with no size is refused by name" \
        "badmatrixtype|a vector or matrix type is not implemented|a caret shape is refused by name" \
        "badstackedshape|found LBracket|a shape suffix may not be stacked" \
        "badraggedarray|this array literal is ragged|a ragged aggregate is refused by name" \
        "badpastedarray|expected ZZ32, found Array|a pasted array element is refused"; do
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

# THE MATRIX AGGREGATE. Seven values, and the first one is the specification's
# own: 4, then the four spellings it calls equivalent, then a rank-three literal
# whose values carry their own coordinates, then a rank-one literal on the path
# it always had.
aggregate() {
    printf '== a matrix aggregate puts its elements where the spec says ==\n'
    if ! timeout 300 "$fortressc" "$repo/fortressc/tests/arrayaggregate.fss" \
        -o "$build/arrayaggregate" >"$build/aggregate.log" 2>&1; then
        bad 'arrayaggregate.fss compiles and links' "$(cat "$build/aggregate.log")"
        return
    fi
    local out status
    out=$("$build/arrayaggregate" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == $'4\n5\n5\n5\n5\n234\n7' ]]; then
        ok "the spec's own A(1,0)=4, four equivalent spellings, a rank three cube: $(printf '%s' "$out" | tr '\n' ' ')"
    else
        bad 'a matrix aggregate puts its elements where the spec says' \
            "want: 4 5 5 5 5 234 7 | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

# RANK TWO AND RANK THREE, FILLED AND READ BACK. Nothing here is square, so a
# wrong stride collides two subscripts onto one slot and the read shows it.
multi() {
    printf '== a higher rank is filled and read back ==\n'
    if ! timeout 300 "$fortressc" "$repo/fortressc/tests/arraymulti.fss" \
        -o "$build/arraymulti" >"$build/multi.log" 2>&1; then
        bad 'arraymulti.fss compiles and links' "$(cat "$build/multi.log")"
        return
    fi
    local out status
    out=$("$build/arraymulti" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == $'0\n1\n2\n10\n11\n12\n123\n7\n7' ]]; then
        ok "a 2 by 3, a 2 by 3 by 4 and a rank one array in one program: $(printf '%s' "$out" | tr '\n' ' ')"
    else
        bad 'a higher rank is filled and read back' \
            "status $status: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

# EVERY DIMENSION ON ITS OWN. This is the check a linearise-first
# implementation passes while being wrong: offset 4 is inside the six slots a
# 2 by 3 holds.
bounds() {
    printf '== a subscript is bounds checked in every dimension ==\n'
    if ! timeout 300 "$fortressc" "$repo/fortressc/tests/arrayoob.fss" \
        -o "$build/arrayoob" >"$build/oob.log" 2>&1; then
        bad 'arrayoob.fss compiles and links' "$(cat "$build/oob.log")"
        return
    fi
    local out status
    out=$("$build/arrayoob" 2>&1)
    status=$?
    if refused_cleanly "$status" && [[ $out == *'out of bounds in dimension 1'* ]]; then
        ok 'a[0,4] on a 2 by 3 halts and names the dimension'
    else
        bad 'a[0,4] on a 2 by 3 halts and names the dimension' "status $status: $out"
    fi
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
  # RE-TARGETED at the matrix aggregate. The row named the single comparison
  # `check_declared_extent` used to make; it walks every dimension now, and the
  # row read `0 hits` and reported COULD NOT BE APPLIED -- which is the clean
  # escape this table's own trap 3 exists to turn into a hard error.
  'crates/types/src/lib.rs|if declared_len != *found {|if false {|stop comparing a declared size with the literal that fills it'
  'crates/types/src/registry.rs|if spelling == ShapeSpelling::Caret {|if false {|let a caret shape resolve as an array'
  # THE RANK, at each of the four places it can be wrong: the linearisation,
  # the per-dimension bound, the checker's arity comparison, and the mangle.
  'runtime/shims.c|offset = offset * a->extents[d] + index;|offset = offset * a->extents[0] + index;|linearise every dimension with the first extent'
  'runtime/shims.c|if (index >= extent) {|if (index >= a->total) {|check every dimension against the TOTAL instead of its own extent'
  'crates/types/src/lib.rs|if found == usize::from(rank) {|if true {|stop comparing the subscript count with the rank'
  'crates/types/src/registry.rs|let Ok(rank) = u8::try_from(extents.len()) else {|let Ok(rank) = u8::try_from(1usize) else {|resolve every shape suffix as rank one'
  'crates/types/src/registry.rs|if !matches!(size, TypeRef::Static { .. }) {|if false {|let a size that resolved to nothing through'
  'crates/parser/src/lib.rs|let glued_bracket = self.at(&Kind::LBracket) && self.glued_left(self.pos);|let glued_bracket = self.at(&Kind::LBracket);|let a spaced bracket be an array size'
  # THE AGGREGATE, at each of the three places its shape can be wrong: which
  # dimension a separator steps, whether a line break is one at all, and whether
  # the groups have to be the same size.
  'crates/parser/src/lib.rs|const SWAP_LOWEST_TWO: bool = true;|const SWAP_LOWEST_TWO: bool = false;|transpose the aggregate: whitespace steps dimension 0 and a semicolon dimension 1'
  'crates/parser/src/lib.rs|            usize::from(line_break)|            0|stop reading a bare line break as a row separator'
  'crates/parser/src/lib.rs|Some(first) if *first == sub => {}|Some(_) => {}|accept a ragged aggregate'
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
            multi
            aggregate
            bounds
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
        multi
        aggregate
        bounds
        one_type
        refusals
        unresolved_size
        glue
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

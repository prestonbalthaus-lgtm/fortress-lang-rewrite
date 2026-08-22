#!/usr/bin/env bash
#
# The literal gate: character literals, the `Char` type, and radix numerals.
#
# Seven things cargo cannot check on its own: that every shape the specification
# accepts LEXES TO THE SAME CHARACTER two different ways, that a `Char` prints
# as itself rather than as a code point, that it is ORDERED without being
# NUMERIC, that the shapes the specification refuses are refused BY NAME, and
# that the legacy's own recorded output for `Char.fss` is reproduced, that a
# radix numeral gives the letters the values `lexical-structure.tex:1121-1129`
# gives them, and that radix twelve's own alphabet rule is enforced.
#
# THE CROSS-CHECKS ARE THE POINT. `'0061'` and `'a'` must compare equal, and so
# must `'TAB'` and `'\t'`: a decoder that got the hex path right and the escape
# path wrong would still print plausible characters on its own, and only an
# assertion relating two paths can see it.
#
# EXIT 70 IS WHAT SEPARATES A DIAGNOSTIC FROM A COMPILER BUG here, the same way
# unit-gate uses it: `println` on a type with no shim used to reach codegen and
# come back as `no runtime symbol to_string_Char`, which is 70 and not 1.
#
# THE RADIX CROSS-CHECK IS THE SAME SHAPE. `1xe_12` and `1ab_12` are two
# spellings of one duodecimal number and both must be 275: `X` is TEN and `E`
# is ELEVEN AT RADIX TWELVE, which no standard integer parser knows, so a
# decoder built on `from_str_radix` reads the first as a malformed literal and
# the second as 275 and looks half right.
#
# A RADIX NUMERAL MUST BEGIN WITH A DIGIT HERE, and that is a NAMED DEVIATION.
# `Literal.rats:22-27` puts the leading-digit guard on the second alternative
# only, so `DE AD BE EF_16` is a radix numeral in 1.0 and an identifier here.
# Letting a numeral begin with a LETTER is a lexical reclassification that
# would take every `NAME_16`-shaped identifier out of the passing set, and the
# measured gain from radix numerals is ZERO corpus files -- so the reclassifi-
# cation is not made and `ProjectFortress/tests/NumeralTest.fss` stays refused,
# on a non-ASCII digit-group separator one line further down.
#
#   ./tools/literal-gate.sh              run the gate
#   ./tools/literal-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/literal-gate.sh --mutate     break the compiler six ways and prove
#                                        the gate refuses each one
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build/char-gate
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

# ---------------------------------------------------------------- assertions

# A diagnostic is exit 1 and nothing else. 70 is EXIT_INTERNAL_ERROR, 124 a
# timeout, 139 SIGSEGV. 0 means it accepted a program it should have refused.
refused_cleanly() { [[ $1 -eq 1 ]]; }

# The eleven lines charliteral.fss computes, written here rather than copied
# from a run: four characters, four cross-checks, two orderings and a
# concatenation.
expected_output() {
    printf 'a\n%s\nǇ\n\U0001D11E\ntrue\ntrue\ntrue\ntrue\nordered\nreflexive\nconcatenated: z\n' "'"
}

selftest() {
    printf '== gate self test ==\n'
    if refused_cleanly 1; then
        ok 'exit 1 is a clean refusal'
    else
        bad 'exit 1 is a clean refusal'
    fi
    local status
    for status in 0 70 124 139; do
        if refused_cleanly "$status"; then
            bad "status $status is refused as a clean refusal" \
                'only exit 1 is a diagnostic; 70 is a compiler bug'
        else
            ok "status $status is refused as a clean refusal"
        fi
    done
    if [[ $(expected_output | wc -l) -eq 11 ]]; then
        ok 'the expected output is eleven lines'
    else
        bad 'the expected output is eleven lines' "$(expected_output | wc -l)"
    fi
}

preflight() {
    ( cd "$repo/fortressc" && cargo build --workspace ) || exit 2
    rm -rf "$build"
    mkdir -p "$build"
}

# EVERY SHAPE, AND TWO PATHS TO THE SAME CHARACTER. `'0061' = 'a'` and
# `'TAB' = '\t'` are the assertions a one-path decoder cannot satisfy.
shapes() {
    printf '== every character literal shape, cross-checked ==\n'
    if ! timeout 300 "$fortressc" "$repo/fortressc/tests/charliteral.fss" \
        -o "$build/charliteral" >"$build/char.log" 2>&1; then
        bad 'charliteral.fss compiles and links' "$(cat "$build/char.log")"
        return
    fi
    local out status
    out=$("$build/charliteral" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == "$(expected_output)" ]]; then
        ok 'one character, an apostrophe, four and five hex digits, three names and an ordering'
    else
        bad 'every character literal shape' \
            "status $status: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

# THE LEGACY'S OWN RECORDED OUTPUT. `Char.test` says `run_out_equals=a\n`, so a
# character prints as ITSELF and not as its code point -- which is the one thing
# an `i32` representation makes easy to get wrong.
oracle() {
    printf '== the legacy recorded a character printing as itself ==\n'
    if ! timeout 300 "$fortressc" "$repo/ProjectFortress/other_compiler_tests/Char.fss" \
        -o "$build/Char" >"$build/oracle.log" 2>&1; then
        bad 'Char.fss compiles and links' "$(cat "$build/oracle.log")"
        return
    fi
    local out status
    out=$("$build/Char" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == 'a' ]]; then
        ok 'Char.fss prints `a`, which is what its .test records'
    else
        bad 'Char.fss prints `a`' "status $status: $out"
    fi
}

# THE LETTER VALUES ARE THE SPECIFICATION'S AND NOT THE ORDINARY ALPHABET'S.
# Two spellings of one duodecimal number, `E` as fourteen outside radix twelve,
# a named specifier, a digit-group separator, and a hex literal compared with
# its own decimal value.
radix() {
    printf '== a radix numeral uses the specification-s digit values ==\n'
    if ! timeout 300 "$fortressc" "$repo/fortressc/tests/radixnumeral.fss" \
        -o "$build/radixnumeral" >"$build/radix.log" 2>&1; then
        bad 'radixnumeral.fss compiles and links' "$(cat "$build/radix.log")"
        return
    fi
    local out status
    out=$("$build/radixnumeral" 2>&1)
    status=$?
    if [[ $status -eq 0 && $out == $'275\n275\n30\n4095\n4095\n8\n84\ntrue' ]]; then
        ok "1xe_12 and 1ab_12 are both 275, and 1e_16 is 30: $(printf '%s' "$out" | tr '\n' ' ')"
    else
        bad 'a radix numeral uses the specification-s digit values' \
            "want: 275 275 30 4095 4095 8 84 true | got: $(printf '%s' "$out" | tr '\n' ' ')"
    fi
}

# Each of these is a program the compiler must REFUSE, and by a diagnostic that
# NAMES THE MECHANISM. Asserting the exit code alone would pass on `character
# literals are not in the M1 subset`, which is what all three reported before
# this milestone and which sends the reader to the wrong place.
refusals() {
    printf '== the refusals ==\n'
    # `entry` and `label` are LOCAL because this function is called from inside
    # the mutation loop, which iterates a variable of each name.
    local entry name pattern label err status
    for entry in \
        "badcharname|naming a character inside a character literal|a Unicode name is refused by name" \
        "badchararith|is not defined on Char|arithmetic on a character is refused by name" \
        "badforbiddenchar|a character literal holds one character|a raw control character is refused by name" \
        "badradix|every digit must denote a value below it|radix twelve may not mix its two alphabets"; do
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

# --------------------------------------------------------------- mutations
#
# file|from|to|label. No `|` in any field: the table is split on IFS='|'.

MUTATIONS=(
  'crates/lexer/src/raw.rs|const HEX_CODE_POINT_DIGITS: usize = 4;|const HEX_CODE_POINT_DIGITS: usize = 99;|stop reading a run of hex digits as a code point'
  'crates/lexer/src/lib.rs|u32::from_str_radix(digits, 16)|u32::from_str_radix(digits, 10)|decode a code point in decimal instead of hex'
  'crates/lexer/src/lib.rs|            "TAB" => |            "tab" => |stop reading the word TAB as a tab'
  'crates/types/src/lib.rs|            if !comparison {|            if false {|let arithmetic through on a Char'
  # THE RADIX DIGIT VALUES, at the two places they can be wrong: `X` is ten and
  # `E` is eleven ONLY at radix twelve.
  "crates/lexer/src/raw.rs|        'X' => Some(10),|        'X' => Some(15),|give X a value other than ten"
  "crates/lexer/src/raw.rs|        'E' if radix == 12 => Some(11),|        'E' if radix == 13 => Some(11),|make E fourteen at radix twelve too"
)

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
    local survived=0 broken=0
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
            rm -rf "$build"; mkdir -p "$build"
            shapes; oracle; radix; refusals
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
        shapes
        oracle
        radix
        refusals
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

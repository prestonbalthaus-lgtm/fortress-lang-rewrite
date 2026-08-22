#!/usr/bin/env bash
#
# The tuple gate. READ THIS BEFORE ASSUMING IT GATES TUPLE VALUES.
#
# SPIKE-COMPOSITE-TYPE HAS LANDED AND TUPLE VALUES HAVE NOT, and the distance
# between those two sentences is what this gate now pins.
#
# WHAT LANDED: `Type::Tuple(&'static [Type])`, interned, with `Type` still
# `Copy` -- asserted by a compiled `const _: () = assert_copy::<Type>()` rather
# than claimed. Four exhaustive matches were forced by the variant and answered;
# the two catch-alls a tuple would have been silently WRONG in were given
# explicit arms. That is the spike's pricing, delivered.
#
# WHAT DID NOT: a tuple VALUE. There is no boxing in this backend, so a tuple
# needs a representation, and `overloading.tex:124-126` makes `f(x: (A,B))` and
# `f(a:A,b:B)` THE SAME DECLARATION -- so M3c's dispatch has to arity-flatten
# before any of it is sound. `registry.rs`'s `resolve` still refuses
# `TypeRef::Tuple` by name and is the SINGLE CONSTRUCTION GATE, which is what
# makes the twenty non-exhaustive sites safe today rather than merely
# unexercised.
#
# SO THIS GATE ASSERTS BOTH HALVES, and neither is decoration:
#   A. the variant exists, interns, dedups, and `Type` is still `Copy`
#   B. every SOURCE position a tuple can be written in is still refused cleanly
#      and by name -- five positions, plus the assertion that a parenthesised
#      single expression is NOT a tuple, which is what keeps the other five
#      honest
#   C. the refusal is still the ONLY gate: nothing outside types.rs and its
#      tests constructs a `Type::Tuple`
#
# TUPLE VALUES PARTLY LANDED ON 2026-08-22 AND PART B WAS CONVERTED, which is
# what the previous version of this line asked whoever landed them to do.
# Part B is now positive fixtures asserted BY VALUE; part B2 is what is still
# refused. Part A is unchanged.
#
# THE VALUES ARE THE ASSERTION. Without a binder node `(a,b) = (1,2)` parses as
# INFIX EQUALITY -- a discarded Boolean comparison -- and tupleTest1/2 have no
# asserts and no `.test`, so that reading compiles, exits 0, does nothing at
# all, and counts as two files gained. An exit code cannot tell the two apart.
#
#   ./tools/tuple-gate.sh              run the gate
#   ./tools/tuple-gate.sh --selftest   only prove the assertions can refuse
#
# There is still no --mutate. An UNCONSTRUCTABLE variant gives a mutation
# nothing to fire on, and the only other thing to break is the refusal itself,
# which `--selftest`'s negative cases already cover and which landing tuple
# values will do for real. A table that deletes a diagnostic to watch a refusal
# stop would assert nothing this file does not already assert.
#
# FORTRESSC pins the binary. KEEP THE PINNED COPY OUTSIDE fortressc/build/.
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build/tuple
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# Measured over the whole corpus with tools/triage.sh. The number is here so
# that when it MOVES, someone asks why -- it is the size of what tuple VALUES
# unlock, and landing the type variant moved none of it.
# 35 at the M6 merge, 40 at the consolidation: the corpus set itself moved
# (api check mode, and the refusals the consolidation added), not the feature.
TUPLE_FIRST_BLOCKERS=38

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

# <label> <expected stdout> <body>
runs() {
    mkdir -p "$build"
    local label=$1 want=$2 body=$3 got
    printf 'component t\nexport Executable\n%s\nend\n' "$body" > "$build/r.fss"
    if ! "$fortressc" "$build/r.fss" -o "$build/r" >"$build/r.err" 2>&1; then
        bad "$label compiles" "$(grep -v '^fortressc: ' "$build/r.err" | head -1)"
        return
    fi
    got=$(timeout 20 "$build/r" 2>&1)
    if [[ $got == "$want" ]]; then
        ok "$label -> $(printf '%s' "$got" | tr '\n' ' ')"
    else
        bad "$label" "got '$(printf '%s' "$got" | tr '\n' ' ')', want '$(printf '%s' "$want" | tr '\n' ' ')'"
    fi
}

run_gate() {
    printf '== part B: what LANDED, asserted by VALUE ==\n'
    printf '   THE VALUES ARE THE ASSERTION AND EXIT CODES ARE NOT ENOUGH. Without\n'
    printf '   a binder node `(a,b) = (1,2)` parses as INFIX EQUALITY -- a\n'
    printf '   discarded Boolean. tupleTest1/2 have no asserts and no .test, so\n'
    printf '   that reading compiles, exits 0, does nothing, and reads as a win.\n\n'

    runs 'a tuple binder BINDS' "$(printf '1\n2')" \
'run(): () = do
  (a, b) = (1, 2)
  println(a)
  println(b)
end'

    # Elements are checked BEFORE any name is declared, so both right-hand
    # names still mean the OUTER bindings.
    runs 'a binder reads the OUTER names' "$(printf '2\n1')" \
'run(): () = do
  a = 1
  b = 2
  (a2, b2) = (b, a)
  println(a2)
  println(b2)
end'

    runs 'a three-element binder' "$(printf '1\n2\n3')" \
'run(): () = do
  (a, b, c) = (1, 2, 3)
  println(a)
  println(b)
  println(c)
end'

    # In statement position a tuple is its elements, evaluated. Both run.
    runs 'a tuple in statement position runs both' "$(printf '1\n2\n9')" \
'run(): () = do
  (println(1), println(2))
  println(9)
end'

    printf '\n== part B2: what has NOT landed, still refused BY NAME ==\n'

    probe 'a tuple TYPE in a parameter' 'cannot be a parameter' \
'f(p: (ZZ32, ZZ32)): ZZ32 = 1
run(): () = println(1)'

    probe 'a tuple TYPE as a return type' 'cannot be the result' \
'f(x: ZZ32): (ZZ32, ZZ32) = x
run(): () = println(1)'

    probe 'a tuple EXPRESSION whose value is USED' 'a tuple expression' \
'run(): () = do
  t = (1, 2)
  println(1)
end'

    probe 'a CALL on the right of a binder' 'unless it is written as a tuple' \
'g(x: ZZ32): ZZ32 = x
run(): () = do
  (a, b) = g(1)
  println(a)
end'

    probe 'a binder whose arity disagrees' 'names 2 value(s) and its initializer has 3' \
'run(): () = do
  (a, b) = (1, 2, 3)
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

    # ---------------------------------------------------------------- part A
    printf '\n== the representation: interned, and `Type` is still Copy ==\n'
    local types=$repo/fortressc/crates/types/src/types.rs

    if grep -q "Tuple(&'static \[Type\])" "$types"; then
        ok '`Type` carries an INTERNED tuple variant'
    else
        bad '`Type` carries an interned tuple variant' \
            'SPIKE-COMPOSITE-TYPE landed this; if it is gone, say why'
    fi
    if grep -q 'const _: () = assert_copy::<Type>();' "$types"; then
        ok '`Type` is asserted `Copy` BY THE COMPILER, not by a comment'
    else
        bad '`Type` is asserted Copy by the compiler' \
            'the whole interning trick is that a shared reference is Copy'
    fi
    if grep -q 'pub fn intern_types' "$types"; then
        ok 'the element-list interner exists'
    else
        bad 'the element-list interner exists'
    fi
    # A PLACEHOLDER NAME WOULD BE A SYMBOL COLLISION, not a cosmetic loss: two
    # different tuples sharing one `symbol()` collide in the emitted object.
    if grep -q 'pub fn name(self)' "$types" && grep -q 'pub fn symbol(self)' "$types"; then
        ok '`name` and `symbol` BUILD the answer rather than returning one string'
    else
        bad '`name` and `symbol` build the answer' \
            'if either is `const fn` again it is returning a placeholder per tuple'
    fi

    # ---------------------------------------------------------------- part C
    printf '\n== the two gates, and they are DIFFERENT gates now ==\n'
    # THIS SECTION USED TO ASSERT THAT `resolve` REFUSED. It BUILDS now -- that
    # is Stage A -- so the invariant moved rather than went away: one site
    # CONSTRUCTS from source, and one site REFUSES the positions a defined
    # function would have to lower.
    local built
    built=$(grep -c 'Type::Tuple(crate::types::intern_types' \
            "$repo/fortressc/crates/types/src/registry.rs")
    if [[ $built -eq 1 ]]; then
        ok 'registry.rs BUILDS a tuple type at exactly one site'
    else
        bad 'registry.rs builds a tuple type at exactly one site' "found $built"
    fi
    local refusal
    refusal=$(grep -c 'TypeError::TupleNotStorable' \
              "$repo/fortressc/crates/types/src/lib.rs")
    if [[ $refusal -ge 1 ]]; then
        ok "the checker refuses a tuple in a lowered position ($refusal site(s))"
    else
        bad 'the checker refuses a tuple in a lowered position' 'no TupleNotStorable'
    fi
    # AN api MAY NAME ONE. That is the whole point of the split, and without
    # this the refusal above could simply be blanket and FortressLibrary.fsi
    # would still be stuck at :1730.
    mkdir -p "$build"
    printf 'api t\nf(x: (ZZ32, ZZ32)): ZZ32\ng(): (ZZ32, String)\nend\n' > "$build/api.fsi"
    if "$fortressc" "$build/api.fsi" >/dev/null 2>&1; then
        ok 'an api signature MAY name a tuple -- it is never lowered'
    else
        bad 'an api signature MAY name a tuple' \
            "$("$fortressc" "$build/api.fsi" 2>&1 | grep -v '^fortressc: ' | head -1)"
    fi
    # CODEGEN MAY NOT PANIC ON ONE. `basic_type`'s arm was an `unreachable!`
    # and it became reachable the moment `resolve` started building: exit 101
    # on user source. The checker is the real gate; this is the backstop.
    if grep -q 'Type::Tuple(_) => None,' "$repo/fortressc/crates/codegen/src/lib.rs"; then
        ok 'codegen has no `unreachable!` for a tuple type'
    else
        bad 'codegen has no `unreachable!` for a tuple type' \
            'the arm that panicked at exit 101 is back'
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

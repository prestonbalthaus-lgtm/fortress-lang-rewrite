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
# THE RESULT DIRECTION LANDED ON 2026-08-24 and part B2 was converted again.
# A tuple RESULT is an LLVM aggregate return -- `insertvalue` into a struct,
# `extractvalue` at the call -- and it is STILL NON-MATERIALISING: the value
# lives in SSA registers, so there is no allocation, no tag and no `alloca`, and
# this gate reads both facts off the IR. What stays refused is NESTING and a
# whole tuple held by ONE name.
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
#   ./tools/tuple-gate.sh --mutate     break the compiler ten ways and prove the
#                                      gate refuses each one
#
# THE "NO --mutate" PARAGRAPH IS GONE. It was true while the variant was
# unconstructable; there are eight rows now, and two of them are SILENT WRONG
# ANSWERS -- a swapped field index in either direction exits 0 with the wrong
# number, which only a VALUE assertion catches.
#
# ONE ROW WAS WRITTEN AND TAKEN BACK OUT, and it is worth saying which. The
# element check in `tuple_value` -- `typed.ty != *want` -- is a BACKSTOP with no
# reachable exerciser: six shapes were probed for an expression that ignores its
# expected type (a name, a call, a `throw`, an `if`, a `do` block, a literal at
# the wrong type) and `require` refuses every one of them FIRST. A mutation row
# that can never fail reports SURVIVED forever, which is worse than not having
# one, so the row came out and the reason is written at the check.
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
# 53 before the multi-value return, 32 after: the RESULT direction took 21
# files off this list, and only two of them onto the compile list -- the rest
# moved to a later wall, which is what a wall-unstacking milestone looks like.
# What is left is 12 `a tuple expression is not implemented`, 7 initializers
# that are neither a written tuple nor a flattened name, 5 tuple PARAMETERS
# that flattening did not reach, 3 parenthesised variable lists, 2 flattened
# names used as a value, and one each of a nested tuple and a mutable one.
TUPLE_FIRST_BLOCKERS=32

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
    # THE VALUE COMPARISON MUST BE ABLE TO SAY NO, or every positive fixture in
    # part B passes against any output at all.
    if [[ "$(printf '1\n2')" == "$(printf '1\n2')" ]]; then
        ok 'the value comparison accepts a match'
    else bad 'the value comparison accepts a match'; fi
    if [[ "$(printf '1\n2')" == "$(printf '2\n1')" ]]; then
        bad 'the value comparison rejects a swap' 'it accepted 2 1 as 1 2'
    else ok 'the value comparison rejects a swap'; fi
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

    # ARITY FLATTENING LANDED 2026-08-23 and this part said so by going RED:
    # `a tuple TYPE in a parameter is refused cleanly` printed its own note --
    # "if tuples have LANDED, this gate has done its job: rewrite it as a real
    # tuple gate". The parameter row is a RUNS row now; the RESULT row is
    # untouched, because the result direction is exactly what did not land.

    runs 'a tuple TYPE in a parameter is FLATTENED' "$(printf '3')" \
'f(p: (ZZ32, ZZ32)): ZZ32 = do
  (a, b) = p
  a + b
end
run(): () = println(f((1, 2)))'

    # THE RESULT DIRECTION LANDED 2026-08-24 and this row went red saying so:
    # it asserted `cannot be the result` and got `expected (ZZ32, ZZ32), found
    # ZZ32` -- an ordinary mismatch, which is what a lowered type gives. A RUNS
    # row now, and the two values differ so a swapped extraction cannot pass.
    runs 'a tuple RESULT is an aggregate, destructured at the call' "$(printf '3\n4')" \
'split(): (ZZ32, ZZ32) = (3, 4)
run(): () = do
  (a, b) = split()
  println(a)
  println(b)
end'

    # WHAT IS STILL REFUSED IN THE RESULT DIRECTION: nesting. Lowering it to a
    # nested struct would make the ABI decision depend on a type's shape two
    # levels down, and it is measured at zero corpus files.
    probe 'a NESTED tuple result' 'an element of another tuple' \
'f(x: ZZ32): (ZZ32, (ZZ32, ZZ32)) = x
run(): () = println(1)'

    # And a whole tuple held by ONE name. `t = split()` compiled before this
    # refusal was added -- inert only while nothing read `t`.
    probe 'a tuple result held by one name' 'held by one name' \
'split(): (ZZ32, ZZ32) = (3, 4)
run(): () = do
  t = split()
  println(1)
end'

    # AND THE ARITY BETWEEN A RESULT AND ITS BINDER. A tuple type excludes every
    # tuple type of a different arity (`types-vals-vars.tex:274`), so this is a
    # refusal and not a truncation -- and taking two of three fields silently
    # would exit 0. THIS PROBE EXISTS BECAUSE ITS MUTATION ROW SURVIVED: nothing
    # in the gate reached the check.
    probe 'a binder whose arity disagrees with a tuple RESULT' 'names 3 value(s)' \
'split(): (ZZ32, ZZ32) = (3, 4)
run(): () = do
  (a, b, c) = split()
  println(a)
end'

    probe 'a tuple EXPRESSION whose value is USED' 'a tuple expression' \
'run(): () = do
  t = (1, 2)
  println(t)
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

    # RE-PINNED. The rewrite dropped this row and the path changed underneath
    # it: a tuple static argument now RESOLVES and is refused afterwards by
    # `tuple_free`, once the stamp has a body. It stays refused DELIBERATELY
    # rather than incidentally.
    probe 'a tuple TYPE as a static argument to a defined generic' 'cannot be a parameter' \
'f[\T\](x: T): ZZ32 = 1
run(): () = println(f[\(ZZ32, ZZ32)\](1))'

    # `(a, a) = (1, 2)` COMPILED AND PRINTED 2 on the day the binder landed:
    # the second part overwrote the first, and every fixture used distinct
    # names so nothing caught it.
    probe 'a binder that repeats a name' 'is bound twice by one tuple binding' \
'run(): () = do
  (a, a) = (1, 2)
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
    # CODEGEN BUILDS A STRUCT FOR ONE, AND STILL DOES NOT PANIC. This row read
    # `Type::Tuple(_) => None,` until 2026-08-24 and went red on the multi-value
    # return: the invariant MOVED rather than went away. `basic_type`'s arm was
    # an `unreachable!` once and became reachable the moment `resolve` started
    # building -- exit 101 on user source -- so what has to stay true is that
    # the arm EXISTS and is not a panic, and what is new is that it produces an
    # aggregate instead of "no storage".
    local tuple_arm
    tuple_arm=$(grep -c 'Type::Tuple(elems) => {' "$repo/fortressc/crates/codegen/src/lib.rs")
    if [[ $tuple_arm -eq 1 ]]; then
        ok 'codegen BUILDS an aggregate for a tuple type'
    else
        bad 'codegen builds an aggregate for a tuple type' \
            "found $tuple_arm arms -- if it is `None` again, every tuple is one word"
    fi
    if grep -q 'struct_type(&fields, false)' "$repo/fortressc/crates/codegen/src/lib.rs"; then
        ok 'the aggregate is an LLVM struct built from the element types'
    else
        bad 'the aggregate is an LLVM struct' 'a placeholder would collide two tuples'
    fi
    if ! grep -q 'Type::Tuple(_) => unreachable' "$repo/fortressc/crates/codegen/src/lib.rs"; then
        ok 'codegen has no `unreachable!` for a tuple type'
    else
        bad 'codegen has no `unreachable!` for a tuple type' \
            'the arm that panicked at exit 101 is back'
    fi
    # AND NOTHING MATERIALISES. The aggregate lives in SSA registers: no
    # allocation, no tag, no `alloca`. Read off the IR rather than asserted in
    # a comment -- an extra `fortress_alloc` here would be a second allocation
    # path, which the allocation rule forbids outright.
    mkdir -p "$build"
    # THE ELEMENTS ARE RUNTIME VALUES ON PURPOSE. With two literals LLVM folds
    # the `insertvalue` chain into a constant aggregate and emits neither
    # instruction, so a constant fixture asserted nothing about how the value
    # is BUILT -- it passed the `extractvalue` half and failed the other for
    # the wrong reason.
    printf 'component t\nexport Executable\nsplit(k: ZZ32): (ZZ32, ZZ32) = (k, k + 1)\nrun(): () = do\n  (a, b) = split(3)\n  println(a)\n  println(b)\nend\nend\n' \
        > "$build/agg.fss"
    local agg
    agg=$("$fortressc" "$build/agg.fss" --emit-ir 2>/dev/null)
    if [[ $agg == *insertvalue* && $agg == *extractvalue* ]]; then
        ok 'the aggregate is built with insertvalue and taken apart with extractvalue'
    else
        bad 'the aggregate uses insertvalue/extractvalue' 'it is going through memory'
    fi
    if [[ $agg != *'fortress_alloc'* ]]; then
        ok 'a tuple result allocates NOTHING'
    else
        bad 'a tuple result allocates nothing' \
            'an aggregate reached the heap -- that is a second allocation path'
    fi

    printf '\n%d passed, %d failed\n' "$passed" "$failed"
    printf 'tuple first-blockers in the corpus at the last measurement: %s\n' \
        "$TUPLE_FIRST_BLOCKERS"
    [[ $failed -eq 0 ]]
}

# THE RATIONALE FOR HAVING NO --mutate DIED WHEN TUPLES LANDED. It was "an
# UNCONSTRUCTABLE variant gives a mutation nothing to fire on" -- true while
# `resolve` refused. There is now a parser probe, a checker split and two
# codegen arms, and a gate asserting landed behaviour that has never been shown
# to refuse is not evidence.
#
# Rows are bar-free AND SINGLE LINE. The table splits on `|`, so a closure's
# bars cannot appear -- and `read` stops at the first newline, so a multi-line
# pattern is silently truncated and matches the wrong thing. Both were paid for
# here: the first draft of the third row was multi-line, and it reported
# `the mutation pattern is not unique (13 hits)` with an EMPTY label.
MUTATIONS=(
  'crates/parser/src/lib.rs|        Ok(Some(fortress_ast::TupleBinding { names, value, span }))|        Ok(None)|delete the binder parse, so the silent INFIX EQUALITY reading comes back'
  'crates/types/src/lib.rs|            self.declare(name.clone(), ty, false);|            let _ = ty;|bind nothing, so the names are not in scope'
  'crates/types/src/lib.rs|                if earlier == name {|                if false {|let a binder repeat a name, so the second part silently overwrites the first'
  'crates/types/src/lib.rs|        if items.len() != b.names.len() {|        if false {|drop the arity check on a binder'
  'crates/codegen/src/lib.rs|.build_extract_value(aggregate, index as u32, name)|.build_extract_value(aggregate, 0, name)|hand every destructured name FIELD 0 -- a silent wrong answer that exits 0'
  'crates/codegen/src/lib.rs|.build_insert_value(aggregate, value, index as u32, "tuple")|.build_insert_value(aggregate, value, 0, "tuple")|build the aggregate with every element in FIELD 0'
  'crates/types/src/lib.rs|if elems.len() != b.names.len() {|if false {|drop the arity check between a tuple RESULT and its binder'
  'crates/types/src/lib.rs|if Self::is_a_whole_tuple(ty) {|if false {|let one name hold a whole tuple again'
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
    local broken=0 survived=0
    for entry in "${MUTATIONS[@]}"; do
        IFS='|' read -r file from to label <<<"$entry"
        printf '\n== mutation: %s ==\n' "$label"
        hits=$(grep -F -c -- "$from" "$repo/fortressc/$file" 2>/dev/null || echo 0)
        if [[ $hits -ne 1 ]]; then
            printf 'FAIL  the mutation pattern is not unique (%s hits in %s)\n' "$hits" "$file"
            broken=$((broken + 1)); continue
        fi
        MUT_PATH=$repo/fortressc/$file MUT_FROM=$from MUT_TO=$to python3 -c '
import os, pathlib
p = pathlib.Path(os.environ["MUT_PATH"])
p.write_text(p.read_text().replace(os.environ["MUT_FROM"], os.environ["MUT_TO"], 1))
'
        ( cd "$repo/fortressc" && cargo build --workspace >/dev/null 2>&1 )
        status=$?
        if [[ $status -ne 0 ]]; then
            printf 'REFUSED  the mutated compiler does not build, which is a refusal too\n'
        else
            passed=0; failed=0
            run_gate >/dev/null 2>&1
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

case ${1:-} in
    --selftest) selftest ;;
    --mutate)   selftest; mutate; exit $? ;;
    '')         if [[ ! -x $fortressc ]]; then
                    printf 'no compiler at %s -- cargo build first\n' "$fortressc" >&2
                    exit 2
                fi
                # THE SELF TEST RUNS FIRST on a bare invocation. It did not
                # before, so a green tail said nothing about whether the
                # instrument could refuse at all.
                selftest
                run_gate ;;
    *)          printf 'unknown argument %s\n' "$1" >&2; exit 2 ;;
esac

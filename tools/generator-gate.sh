#!/usr/bin/env bash
#
# The GENERATOR gate: the indexed generator protocol, and the two lowerings
# that ride on it.
#
# WHAT THIS GATES, and it is deliberately NOT 1.0's protocol. 1.0's is
# `generate[\R\](r: Reduction[\R\], body: E->R): R` -- internal iteration
# through a monoid object -- and three separate things block that form here:
# there is no first-class `Reduction`, a `()` arrow CODOMAIN is refused by
# name, and a COMPONENT cannot name `Generator`/`Indexed`/`Condition` because
# the implicit core-api import is api-side only. So nominal membership is
# unavailable and the protocol has to be a set of MEMBERS recognised
# structurally.
#
# The members are 1.0's own. `Library/FortressLibrary.fsi:1205`'s
# `trait Indexed[\E, I\]` declares `getter size()` and `opr [i: I]: E`, and its
# doc comment supplies the contract that makes walking it by index its
# GENERATION ORDER -- "self[i] = v", "stripping away the i yields exactly the
# results of v <- self". `Library/CompilerLibrary.fsi` is 1.0's own NATIVE
# compiler library and it cuts the protocol down the same way, to a monomorphic
# `GeneratorZZ32` with two ground result types. Cutting is the precedent.
#
# SEVEN THINGS cargo cannot check:
#   * that `opr []` DISPATCHES on an object at all -- the declaration parsed
#     long before this milestone and only the USE was refused
#   * that `for x <- <collection>` walks IN ORDER. An exit code cannot tell an
#     ordered walk from a shuffled one, and neither can a fixture that binds an
#     element and never prints it
#   * that `for x <- <array>` still works -- the path this milestone had to
#     leave alone, and the 432-object byte-identical IR run is its other half
#   * that a comprehension over a collection is SEQUENTIAL. It is lowered to a
#     `while` and not a `for` precisely because a `for` body is OUTLINED and
#     appending to one shared `List` from several workers is a race
#   * that a generator source is evaluated ONCE for a walk and ONCE PER ROUND
#     for a `while` binding condition. Those are different requirements and a
#     lowering that got either backwards still compiles
#   * that BOTH member spellings work -- 1.0 declares `size`/`holds`/`get` as
#     GETTERS and the minted `List[\T\]` declares plain methods
#   * that a source which is not a generator is refused BY NAME, naming the
#     member it does not answer rather than reporting an absence
#
#   ./tools/generator-gate.sh              run the gate
#   ./tools/generator-gate.sh --selftest   only prove the assertions can refuse
#   ./tools/generator-gate.sh --mutate     break the compiler six ways and prove
#                                          the gate refuses each one
#
# FORTRESSC pins the binary. KEEP THE PINNED COPY OUTSIDE fortressc/build/ --
# that directory is shared and sixteen gates `rm -rf` it. FORTRESSC and
# --mutate do not mix and --mutate refuses when it is set.
set -uo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build=$repo/fortressc/build/generator
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# Measured with tools/triage.sh at 12ca542e3, over all 1956 files. The number is
# here so that when it MOVES someone asks why. IT IS SMALL ON PURPOSE and was
# known to be small before this was built: 172 corpus files WRITE a generator
# construct and 144 of them die in the PARSER, and of the rest almost all import
# a Library module whose `.fss` does not compile -- so the protocol is NECESSARY
# for all 172 and SUFFICIENT for none. It is a prerequisite, not a lever.
GENERATOR_FIRST_BLOCKERS=11

passed=0
failed=0
ok()  { passed=$((passed + 1)); printf 'ok    %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL  %s\n' "$1"; [[ -n ${2:-} ]] && printf '      %s\n' "$2"; }

refused_cleanly() { [[ $1 -eq 1 ]]; }
names_mechanism() { grep -q -F -- "$2" <<<"$1"; }

# <label> <expected stdout> <body>
runs() {
    mkdir -p "$build"
    local label=$1 want=$2 body=$3 got
    printf 'component g\nexport Executable\n%s\nend\n' "$body" > "$build/r.fss"
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

# <label> <expected substring> <body>
probe() {
    mkdir -p "$build"
    local label=$1 want=$2 body=$3 err rc
    printf 'component g\nexport Executable\n%s\nend\n' "$body" > "$build/p.fss"
    err=$("$fortressc" "$build/p.fss" --emit-obj -o /dev/null 2>&1 >/dev/null); rc=$?
    if refused_cleanly $rc; then
        ok "$label is refused cleanly"
    else
        bad "$label is refused cleanly" "exit $rc"
    fi
    if names_mechanism "$err" "$want"; then
        ok "$label -- the diagnostic names the mechanism"
    else
        bad "$label -- the diagnostic names the mechanism" "got: $(head -1 <<<"$err")"
    fi
}

# An in-tree fixture, COMPILED AND RUN, asserting its exact output. In the tree
# rather than in $build because a fixture that writes `import List.{...}`
# resolves nothing outside the source path.
fixture_runs() {
    mkdir -p "$build"
    local label=$1 name=$2 want=$3 got
    if ! "$fortressc" "$repo/fortressc/tests/$name.fss" -o "$build/$name" \
            >"$build/$name.err" 2>&1; then
        bad "$label compiles" "$(grep -v '^fortressc: ' "$build/$name.err" | head -1)"
        return
    fi
    got=$(timeout 20 "$build/$name" 2>&1)
    if [[ $got == "$want" ]]; then
        ok "$label -> $(printf '%s' "$got" | tr '\n' ' ')"
    else
        bad "$label" "got '$(printf '%s' "$got" | tr '\n' ' ')', want '$(printf '%s' "$want" | tr '\n' ' ')'"
    fi
}

# An in-tree fixture that must be REFUSED, asserting the MESSAGE and not the
# exit code: both readings of every one of these refuses, so only the message
# separates them.
fixture_refused() {
    mkdir -p "$build"
    local label=$1 name=$2 want=$3 err rc
    err=$("$fortressc" "$repo/fortressc/tests/$name.fss" --emit-obj -o /dev/null 2>&1 >/dev/null); rc=$?
    if refused_cleanly $rc; then
        ok "$label is refused cleanly"
    else
        bad "$label is refused cleanly" "exit $rc"
    fi
    if names_mechanism "$err" "$want"; then
        ok "$label -- the diagnostic names the mechanism"
    else
        bad "$label -- the diagnostic names the mechanism" "got: $(head -1 <<<"$err")"
    fi
}

selftest() {
    printf '== gate self test ==\n'
    # THE VALUE COMPARISON MUST BE ABLE TO SAY NO, and it must reject a
    # PERMUTATION specifically -- ORDER is what half these fixtures assert, and
    # a comparison that only counts lines would pass a shuffled walk.
    if [[ "$(printf '1\n2\n3')" == "$(printf '1\n2\n3')" ]]; then
        ok 'the value comparison accepts a match'
    else bad 'the value comparison accepts a match'; fi
    if [[ "$(printf '1\n2\n3')" == "$(printf '3\n2\n1')" ]]; then
        bad 'the value comparison rejects a REVERSED walk' 'order is not being checked'
    else ok 'the value comparison rejects a REVERSED walk'; fi
    if [[ "$(printf '1\n2\n3')" == "$(printf '1\n1\n1')" ]]; then
        bad 'the value comparison rejects a REPEATED element' \
            'an `opr []` that ignores its index would pass'
    else ok 'the value comparison rejects a REPEATED element'; fi
    if refused_cleanly 1; then ok 'exit 1 is a clean refusal'
    else bad 'exit 1 is a clean refusal'; fi
    for status in 0 70 101 139; do
        if refused_cleanly "$status"; then
            bad "status $status reads as a clean refusal"
        else
            ok "status $status is not a clean refusal"
        fi
    done
    if names_mechanism 'Box is not a generator here: it declares no `opr []`' 'no `opr []`'; then
        ok 'names_mechanism finds its substring'
    else bad 'names_mechanism finds its substring'; fi
    # THE TWO PROTOCOLS ARE DIFFERENT PROTOCOLS. A generator diagnostic must not
    # satisfy a condition assertion; that confusion is what would let the
    # binding-condition rows pass against the `for` lowering.
    if names_mechanism 'Box is not a generator here' 'is not a condition here'; then
        bad 'a generator diagnostic reads as a condition diagnostic'
    else ok 'a generator diagnostic does not read as a condition diagnostic'; fi
    if names_mechanism 'expected an array, found ZZ32' 'is not a generator here'; then
        bad 'the old array refusal reads as the new named refusal'
    else ok 'the old array refusal does not read as the named refusal'; fi
    printf '\n%d passed, %d failed\n' "$passed" "$failed"
    [[ $failed -eq 0 ]]
}

run_gate() {
    printf '== part A: the ELEMENT half -- `opr []` dispatches on an object ==\n'

    runs 'a declared `opr []` is REACHED, and its index is USED' "$(printf '30\n31\n32')" \
'object Row(base: ZZ64)
  opr [i: ZZ64]: ZZ64 = base + i
end
run(): () = do
  r = Row(30)
  println(r[0])
  println(r[1])
  println(r[2])
end'

    runs 'an array subscript is untouched by the object path' "$(printf '7\n9')" \
'run(): () = do
  a: ZZ64[2] = [7 9]
  println(a[0])
  println(a[1])
end'

    printf '\n== part A: `for x <- <collection>`, and ORDER is the assertion ==\n'

    runs 'a `for` over an object walks size elements IN ORDER' "$(printf '100\n101\n102\n103')" \
'object Row(n: ZZ64)
  size(): ZZ64 = n
  opr [i: ZZ64]: ZZ64 = 100 + i
end
run(): () = for x <- Row(4) do println(x) end'

    runs 'a `for` over an ARRAY still works' "$(printf '5\n6\n7')" \
'run(): () = do
  a: ZZ64[3] = [5 6 7]
  for x <- a do println(x) end
end'

    # 1.0 declares `size` as a GETTER on `Indexed` and the minted `List[\T\]`
    # declares the plain method. Both are real Fortress and both must reach the
    # protocol, or the library spelling and the compiler's own disagree.
    runs 'a GETTER spelling of `size` also carries the protocol' "$(printf '0\n1')" \
'object Row(n: ZZ64)
  getter size(): ZZ64 = n
  opr [i: ZZ64]: ZZ64 = i
end
run(): () = for x <- Row(2) do println(x) end'

    # THE SOURCE IS EVALUATED ONCE. A lowering that re-evaluated it per element
    # would print the marker four times and still exit 0.
    runs 'the generator source is evaluated ONCE for a walk' "$(printf 'made\n0\n1\n2')" \
'object Row(n: ZZ64)
  size(): ZZ64 = n
  opr [i: ZZ64]: ZZ64 = i
end
mk(n: ZZ64): Row = do
  println("made")
  Row(n)
end
run(): () = for x <- mk(3) do println(x) end'

    printf '\n== part A: a comprehension over a COLLECTION ==\n'

    runs 'a comprehension over a comprehension-built List, in order' "$(printf '10\n20\n30\n40')" \
'run(): () = do
  xs = <|[\ZZ64\] x | x <- 1:4 |>
  ys = <|[\ZZ64\] 10 y | y <- xs |>
  for z <- ys do println(z) end
end'

    runs 'a comprehension over an ARRAY' "$(printf '14\n16\n18')" \
'run(): () = do
  a: ZZ64[3] = [7 8 9]
  ys = <|[\ZZ64\] 2 y | y <- a |>
  for z <- ys do println(z) end
end'

    runs 'a comprehension over a collection WITH a guard' "$(printf '3\n4\n5')" \
'run(): () = do
  xs = <|[\ZZ64\] x | x <- 1:5 |>
  ys = <|[\ZZ64\] y | y <- xs, y > 2 |>
  for z <- ys do println(z) end
end'

    runs 'a comprehension over a user object' "$(printf '0\n2\n4')" \
'object Row(n: ZZ64)
  size(): ZZ64 = n
  opr [i: ZZ64]: ZZ64 = 2 i
end
run(): () = do
  ys = <|[\ZZ64\] y | y <- Row(3) |>
  for z <- ys do println(z) end
end'

    printf '\n== part A: the BINDING CONDITION, both keywords ==\n'

    runs 'an `if x <- g` takes the then arm when it HOLDS' "$(printf '7')" \
'object Just1(v: ZZ64)
  getter holds(): Boolean = true
  getter get(): ZZ64 = v
end
run(): () = if x <- Just1(7) then println(x) else println(0) end'

    runs 'an `if x <- g` takes the else arm when it does NOT' "$(printf '0')" \
'object Nothing1()
  getter holds(): Boolean = false
  getter get(): ZZ64 = 5
end
run(): () = if x <- Nothing1() then println(x) else println(0) end'

    # A METHOD spelling of the two `Condition` members, which is what an object
    # written without `getter` declares.
    runs 'a METHOD spelling of `holds`/`get` also carries the protocol' "$(printf '4')" \
'object Just1(v: ZZ64)
  holds(): Boolean = true
  get(): ZZ64 = v
end
run(): () = if x <- Just1(4) then println(x) else println(0) end'

    # A `while` BINDING CONDITION RE-EVALUATES ITS SOURCE ONCE PER ROUND. That
    # is what makes it a while-CONDITION; a lowering that bound the source once
    # would loop forever on the FIRST value, and one that lowered it as an `if`
    # would print 3 and stop.
    runs 'a `while x <- g` re-evaluates its source each round' "$(printf '3\n2\n1\n99')" \
'object Countdown(n: ZZ64)
  holds(): Boolean = n > 0
  get(): ZZ64 = n
end
run(): () = do
  k: ZZ64 := 3
  while x <- Countdown(k) do
    println(x)
    k := k - 1
  end
  println(99)
end'

    printf '\n== part B: what is refused, and BY NAME ==\n'

    probe 'an object with `size` and no `opr []`' 'declares no `opr []`' \
'object Half(n: ZZ64)
  size(): ZZ64 = n
end
run(): () = for x <- Half(2) do println(x) end'

    probe 'an object with `opr []` and no `size`' 'declares no `size`' \
'object Half(n: ZZ64)
  opr [i: ZZ64]: ZZ64 = i
end
run(): () = for x <- Half(2) do println(x) end'

    probe 'a SCALAR as a generator source' 'expected an array' \
'run(): () = for x <- 5 do println(x) end'

    probe 'a rank two array is still refused for ITERATION' 'rank 2 array' \
'run(): () = do
  a: ZZ64[2,2] = [1 2; 3 4]
  for x <- a do println(x) end
end'

    probe 'a SCALAR as a binding condition' 'is not a condition here' \
'run(): () = if x <- 5 then println(x) end'

    probe 'an object with `holds` and no `get`' 'declares no `get`' \
'object Half(n: ZZ64)
  getter holds(): Boolean = true
end
run(): () = if x <- Half(1) then println(1) end'

    # A `Condition` yields ONE element. Destructuring it is the tuple binder,
    # which this node is walked before, so it is refused rather than half-taken.
    probe 'a binding condition with TWO binders' 'not implemented' \
'object Just1(v: ZZ64)
  getter holds(): Boolean = true
  getter get(): ZZ64 = v
end
run(): () = if (a, b) <- Just1(1) then println(a) end'

    printf '\n== part C: the protocol is decided in ONE place ==\n'
    local lib=$repo/fortressc/crates/types/src/lib.rs
    local sites
    sites=$(grep -c 'self.generator_extent(' "$lib")
    if [[ $sites -eq 2 ]]; then
        ok 'exactly TWO callers ask for the extent -- the `for` and the comprehension walk'
    else
        bad 'exactly two callers ask for the extent' \
            "found $sites -- a third caller means a third reading of the protocol"
    fi
    if grep -q 'fn protocol_gap' "$lib"; then
        ok 'one helper answers WHICH member is missing'
    else
        bad 'one helper answers which member is missing'
    fi
    # `SeqIterate` IS LOWERED BY THE CHECKER AND MUST NOT REACH CODEGEN. If it
    # ever does, the sequential guarantee has quietly become codegen's problem.
    if [[ $(grep -c 'SeqIterate' "$repo/fortressc/crates/codegen/src/lib.rs") -eq 0 ]]; then
        ok '`SeqIterate` never reaches codegen'
    else
        bad '`SeqIterate` never reaches codegen' \
            'the checker is supposed to be the only thing that knows about it'
    fi
    # The minted collection must carry the protocol in 1.0's spelling, or
    # `for x <- aList` works by accident through a name this compiler invented.
    if grep -q 'opr \[i: ZZ64\]: T = store\[i\]' "$repo/fortressc/crates/types/src/List.fss"; then
        ok 'the minted `List[\T\]` declares `opr []`, not an invented `get`'
    else
        bad 'the minted List declares `opr []`' 'the element half is not 1.0 spelling'
    fi
    if grep -q 'size(): ZZ64 = count' "$repo/fortressc/crates/types/src/List.fss"; then
        ok 'the minted `List[\T\]` declares `size`'
    else
        bad 'the minted List declares `size`'
    fi

    printf '\n== part D: the SET comprehension ==\n'

    # THE FIXTURES FOR THIS PART LIVE IN THE TREE AND NOT IN $build, and that
    # is not tidiness: two of them write `import List.{...}`, and a probe
    # outside the source path does not get the implicit core import and does
    # not resolve an import at all. It measures a different compiler.
    fixture_runs 'a set comprehension dedups, keeps first-occurrence order, and walks a collection' \
        setcomprehension "$(printf '5\n0\n1\n2\n3\n4\n3\n1\n3\n3\n2\n6')"

    # LINK 5's RULE 1, ONE LEVEL DOWN. Both imports bring a `List` and a `Set`
    # an api DECLARES and never DEFINES, so neither was ever constructible;
    # the minted collections replace them instead of colliding with them.
    fixture_runs 'a MERGED `List` and `Set` lose to the minted ones' \
        comprehensionmerged "$(printf '3\n1')"

    # AND A DECLARATION THE FILE WROTE ITSELF STILL WINS, because that one IS
    # constructible and the program means it.
    fixture_refused "a file's OWN \`List\` is still a refusal" \
        badcomplisttaken 'declares one of its own under that name'
    fixture_refused "a file's OWN \`Set\` is a refusal too" \
        badcompsettaken 'mints its own `Set`'

    # A MAP COMPREHENSION IS THE SET'S BRACKETS WITH TWO STATIC ARGUMENTS, and
    # it is refused BY NAME rather than let through to build a set of the
    # wrong thing. `not_working_static_tests/SetComprehension.fss` is the
    # corpus witness.
    fixture_refused 'a MAP comprehension is refused by name' \
        badmapcomprehension 'a map comprehension, written'

    # A SET LITERAL IS A `Call` TO `{_}`, NOT A LITERAL NODE, and it lowers onto
    # the SAME minted collection. Order and dedup are both asserted by value:
    # `{7, 7, 7, 3, 7}` is five written and two distinct, in that order.
    fixture_runs 'a set LITERAL dedups, keeps written order, and answers `IN`' \
        setliteral "$(printf '5\n0\n4\n2\n7\n3\n2\n0\ntrue\nfalse')"

    fixture_refused 'an untyped set literal is refused by name' \
        badsetliteral "element type is not written anywhere"

    # The minted Set has to carry the protocol in 1.0's spelling for the same
    # reason the List does, or `for x <- aSet` works through a name this
    # compiler invented.
    if grep -q 'opr \[i: ZZ64\]: T = store\[i\]' "$repo/fortressc/crates/types/src/Set.fss"; then
        ok 'the minted `Set[\T\]` declares `opr []`, not an invented `get`'
    else
        bad 'the minted Set declares `opr []`' 'the element half is not 1.0 spelling'
    fi
    # THE MEMBERSHIP TEST IS THE ONE THING THAT MAKES IT A SET. A mutation row
    # makes it always answer `false`; this asserts the shape that row matches.
    if grep -q 'if store\[i\] = x then found := true end' "$repo/fortressc/crates/types/src/Set.fss"; then
        ok 'the minted `Set[\T\]` decides membership with `=` over its store'
    else
        bad 'the minted Set decides membership with `=`' \
            'the mutation row that makes the test lie will not match'
    fi

    printf '\n%d passed, %d failed\n' "$passed" "$failed"
    printf 'generator first-blockers in the corpus at the last measurement: %s\n' \
        "$GENERATOR_FIRST_BLOCKERS"
    [[ $failed -eq 0 ]]
}

# Rows are bar-free AND SINGLE LINE: the table splits on IFS='|', and `read`
# stops at the first newline so a multi-line pattern is silently truncated and
# matches the wrong thing. Every pattern was BASELINED at exactly one hit before
# being written in, and `cargo fmt` had already run -- fmt splitting a long
# `if let` across lines is what made the second row's first draft match zero.
MUTATIONS=(
  'crates/types/src/lib.rs|return self.dispatch_method(base, SUBSCRIPT, indices, span, span, expected);|let _ = SUBSCRIPT;|delete the object-subscript dispatch, so a declared `opr []` is unreachable again'
  'crates/types/src/lib.rs|self.protocol_gap(source, &[(SIZE, SIZE, 1), (SUBSCRIPT, "opr []", 2)])|self.protocol_gap(source, &[(SIZE, SIZE, 1)])|stop requiring `opr []` of a generator, so the named refusal stops naming it'
  'crates/types/src/lib.rs|if self.declared_as_a_getter(name) {|if false {|always CALL a protocol member, so the getter spelling breaks'
  'crates/types/src/lib.rs|if !loops {|if true {|lower `while x <- g` as an `if`, so the loop runs once and stops'
  'crates/types/src/lib.rs|op: BinOp::Lt,|op: BinOp::Le,|walk one element past the extent in the comprehension lowering'
  'crates/types/src/List.fss|opr [i: ZZ64]: T = store[i]|opr [i: ZZ64]: T = store[0]|make the minted List ignore its index -- a SILENT WRONG ANSWER that only a value assertion catches'
  'crates/types/src/Set.fss|opr [i: ZZ64]: T = store[i]|opr [i: ZZ64]: T = store[0]|make the minted Set ignore its index, the same silent wrong answer one collection over'
  'crates/types/src/Set.fss|if store[i] = x then found := true end|if false then found := true end|MAKE THE MEMBERSHIP TEST LIE -- every element reads as new, the set stops deduplicating and becomes a list, and NOTHING but the size assertion in setcomprehension.fss can tell'
  'crates/types/src/comprehension.rs|builder: "insert",|builder: "append",|build a set with the LIST builder, so duplicates survive and the brackets stop meaning what they say'
  'crates/types/src/comprehension.rs|named(decl) == Some(name) && !merged(decl)|named(decl) == Some(name)|refuse a MERGED collection name again, taking the three corpus files back down'
  'crates/types/src/comprehension.rs|for element_expr in elements {|for element_expr in elements.into_iter().rev() {|build a set literal BACKWARDS -- same size, same elements, wrong order, and only a value assertion sees it'
  'crates/types/src/comprehension.rs|(None, Some(from_slot)) => from_slot,|(None, Some(_from_slot)) => return Err(TypeError::SetLiteralElementUnwritten { span }),|stop taking a set literal element type from the slot it initialises'
  'crates/types/src/Set.fss|opr IN(x: T, self): Boolean = contains(x)|opr IN(x: T, self): Boolean = NOT contains(x)|make `IN` answer the opposite, which every exit code accepts'
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
    local entry file from to label hits status before after
    local broken=0 survived=0
    for entry in "${MUTATIONS[@]}"; do
        IFS='|' read -r file from to label <<<"$entry"
        printf '\n== mutation: %s ==\n' "$label"
        hits=$(grep -F -c -- "$from" "$repo/fortressc/$file" 2>/dev/null || echo 0)
        if [[ $hits -ne 1 ]]; then
            printf 'FAIL  the mutation pattern is not unique (%s hits in %s)\n' "$hits" "$file"
            broken=$((broken + 1)); continue
        fi
        before=$(md5sum "$repo/fortressc/$file" | cut -d' ' -f1)
        MUT_PATH=$repo/fortressc/$file MUT_FROM=$from MUT_TO=$to python3 -c '
import os, pathlib
p = pathlib.Path(os.environ["MUT_PATH"])
p.write_text(p.read_text().replace(os.environ["MUT_FROM"], os.environ["MUT_TO"], 1))
'
        after=$(md5sum "$repo/fortressc/$file" | cut -d' ' -f1)
        # A SED THAT MATCHED NOTHING READS AS A CLEAN ESCAPE. The hit count above
        # is checked against the file, but the WRITE can still be a no-op if the
        # replacement equals the pattern, so the bytes are compared too.
        if [[ $before == "$after" ]]; then
            printf 'FAIL  the mutation did not change the file -- it would read as an escape\n'
            broken=$((broken + 1))
            git -C "$repo" checkout HEAD -- "fortressc/$file"
            continue
        fi
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
                selftest
                run_gate ;;
    *)          printf 'unknown argument %s\n' "$1" >&2; exit 2 ;;
esac

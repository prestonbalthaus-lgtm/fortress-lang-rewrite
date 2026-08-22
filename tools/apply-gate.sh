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
fortressc=${FORTRESSC:-$repo/fortressc/target/debug/fortressc}
export LLVM_SYS_221_PREFIX=${LLVM_SYS_221_PREFIX:-$HOME/.local/opt/llvm22-root/usr/lib64/llvm22}
export CPATH=${CPATH:-$HOME/.local/opt/gc-root/usr/include}
export LIBRARY_PATH=${LIBRARY_PATH:-$HOME/.local/opt/gc-root/usr/lib64}

# Measured 2026-08-19 at the end of M3k, not taken from the design document.
# M3f left this at 187, M3h took it to 205, M3i to 222, M3j to 242. M3k takes
# it to 262 in four measured steps: +9 for AND/OR/NOT and Boolean equality,
# +5 for print/ignore/assert, +5 for `^`, and +1 more when `^` stopped
# requiring its operands to agree. Zero regressions across the whole
# milestone -- see the M3k note.
# M4's parallel `for` takes it to 266: four legacy files whose first blocker was
# the reserved word `for`. The milestone's evidence is tools/parallel-gate.sh,
# not this number.
# 280 files compile at the M5 tip. The floor is 279 rather than 280 on purpose:
# ProjectFortress/tests/XXXimmutable0.fss is a must-FAIL negative test that we
# ACCEPT, because the shadowing rule it exercises has never been implemented and
# the file used to fail earlier, on the block-level `var` M5 added. Refusing it
# properly must not break this floor.
# 285 at the opr parse spike: +5 for operator declarations, zero lost, and every
# one of the pre-existing 280 emits byte-identical IR. The floor stays one below
# the count for the same reason it did at 279 -- XXXimmutable0.fss is a must-FAIL
# negative test we accept, and refusing it properly must not break this floor.
# 291 at the M6 declaration parser: +6 for modifiers, continuation-line topology
# clauses and `comprises { ... }`, zero lost, and every one of the 285 that
# compiled at the opr spike emits byte-identical IR.
# 2026-08-21, the semantics pass: 291 -> 288 and the floor from 290 to 287,
# and the count went DOWN because the metric got HONEST. Everything after the
# component's closing `end` used to be silently discarded, so three files with a
# spare trailing `end` compiled. All three are must-FAIL tests and the legacy
# implementation's exact expected error is on disk for each of them:
#   Compiled0.e.fss  XXX0e.test  `Unmatched delimiter "end".` at 18:1-3
#   Compiled0.u.fss  XXX0u.test  same, at 15:1-3
#   Compiled1.c.fss  XXX1c.test  same, at 19:1-3
# The floor stays ONE below the count for the same reason it always has --
# XXXimmutable0.fss is a must-FAIL negative test we still accept, and refusing
# it properly must not break this floor. That single unit of slack is spoken
# for; it was not spent here.
# 2026-08-21, SPIKE-TEMPLATE-WELLFORMEDNESS half one: 288 -> 285, floor 287 to
# 284. Again the count went DOWN because the metric got HONEST, and again every
# lost file is a must-FAIL test whose expected error is on disk -- this time
# matching OUR diagnostic at the same LINE AND COLUMN:
#   Compiled1.ae.fss  XXX1ae.test  `D is undefined.`        14:32
#   Compiled1.n.fss   XXX1n.test   `Garbage is undefined.`  15:25
#   Compiled10.e.fss  XXX10e.test  `S is undefined.`        20:26
# Refusing `Object` and `Any` as well would cost FOUR more -- Compiled12.a0,
# Compiled12.b0, Go0b and Library/TypeProxy.fss, a quarter of the Library files
# that compile at all -- and that is the Object/Any seeding decision, not this
# one. See docs/superpowers/specs/2026-08-21-template-wellformedness.md.
# 2026-08-21, THE THREE-LANE MERGE: 285 -> 307, floor 284 to 306. The path is
# not one number and each leg was swept and diffed on its own: the codegen lane
# took 285 -> 290 (+6 features, and -1 for
# SpecData/examples/preliminaries/Overview.List.fss, whose abstract member names
# the unimplemented `Object` -- that lane measured and named the same file), and
# the frontend lane took 290 -> 307 (+20, and -3 must-FAIL tests the
# operator-word rule now refuses: XXXLabel, XXXWrongTraitName, XXXtest.OPR.name).
# The IR BODY of every file compiling on both sides of every leg is byte for
# byte unchanged -- measured with the unconditional runtime `declare` lines
# filtered, which is the only thing each leg adds to a module it does not
# otherwise touch.
# 2026-08-21, THE CONSOLIDATION: THE METRIC IS SPLIT, because one number stopped
# meaning one thing. SPIKE-API-CHECK-MODE makes an api CHECKED instead of refused
# as the first statement of `Checker::run`, and AN API EMITS NO OBJECT -- so the
# single count went 308 -> 366 while the number of files that produce a `.o`
# went DOWN by one. A floor on the sum would have read 58 newly-checked
# signatures as feature growth and hidden the regression underneath it.
#   OBJECT_FLOOR   .fss files that compile end to end AND EMIT AN OBJECT
#   API_FLOOR      .fsi files whose headers resolve and whose bounds discharge
# Measured on the consolidated tree: 290 objects, 60 apis, 350 together.
# GETTERS: 328 objects, 62 apis, 390 together. A getter is a nullary dotted
# method underneath and a field read on the surface; thirteen .fss gained and
# TWO lost, both must-fail tests the merge now refuses for the reason 1.0 gives.
# ACCESSORS + the named-import fix: 318 objects, 62 apis, 380 together. The
# `asString` builtin is 12 of the 13 gained .fss; the resolver reading its
# import list is the other one.
# SPIKE-OPEXPR, the operator DECLARATION half: 305 objects, 62 apis, 367
# together, and the two commits are separable -- the mutation table measured
# each rather than the arithmetic being inferred:
#   the parser work (subscripted assignment, postfix declarations)  NET +-1
#      gained not_passing_yet/OverloadsA.fsi, and LOST
#      SpecData/examples/advanced/Overloading.fss to MAX_INSTANTIATIONS on
#      `Indexed` -- because FortressLibrary.fsi now PARSES and resolution
#      merges more of it, not because anything got worse
#   `||` as a guarded builtin                                       +9 .fss
#      exactly the 9 that mutation Q5 loses when the builtin is removed
# D7 ADOPTED, `nat`/`int`/`bool` OPEN: 297 objects, 61 apis, 358 together. The
# eight are genericTest1/2, tparams0/1/2, Compiled1.av, Compiled6.af and
# test_library/TestNative.fsi, and genericTest1 checks its own arithmetic --
# `f[\1\]() + g[\2,3\]() = 6` prints `pass`.
#
# ARRAY TYPES `T[n]`: 332 objects, 62 apis. FOUR files, zero lost, and the IR
# body of all 390 modules that compiled before is BYTE FOR BYTE unchanged --
# which is the acceptance test, not this count.
#   ProjectFortress/tests/NatParamOverloading.fss           runs, exit 0
#   SpecData/.../Overview.Expression.aggregate.a.fss        runs, exit 0
#   ProjectFortress/not_passing_yet/singletonArray.fss      COMPILES, then
#     halts at run time with `array index out of bounds (1, 1)`, which is
#     correct: it declares `RR64[1]` and reads `a[1]`.
#   ProjectFortress/parser_tests/XXXequalityTesting.fss     a must-FAIL by the
#     XXX convention, and the slack below is where it is accounted for.
# The first-blocker count for this syntax was 62 and the move is 4. That is a
# 15x inflation of the same kind first-blocker counting has produced five
# milestones running; the buckets behind it are 32 files needing a second
# dimension and 23 needing something else entirely.
#
# THE SLACK IS 38 AND NOT 1, and it is COUNTED rather than traditional:
# tools/oracle-accepted-must-fail.txt names 37 programs that MUST FAIL and that
# this compiler still accepts, every one of them a `.fss` inside the object
# count. The comment here used to say 38 against a list of 39, which was wrong
# twice; the list is now 37 because the oracle gate reported two entries as
# NEWLY REFUSED (Compiled6.g, Compiled6.l -- refused since the getters commit,
# which owed the deletion) and its own header says a refused file must come out
# in the same commit. The thirty-eighth is XXXequalityTesting.fss, a must-FAIL
# by the XXX convention and by its own source comments, which NO `.test` file
# names -- so the oracle ratchet cannot see it and it needs its slack here.
# Refusing any of them properly is a ratchet forward and must not break this
# floor -- which is the same reason the floor was ever one below, applied to
# the real population instead of to the one file that was known when the rule
# was written. When that list shrinks, this floor rises with it, and the two
# move in the same commit.
#
# DIMENSIONS AND UNITS, sub-phase 4d rung one: 333 objects, 63 apis. TWO files,
# `Library/incomplete/basic/Fortress.InformationUnits.fsi` and its `.fss`, and
# reserving the seven unit operators cost ZERO -- measured over all 394 files
# that compiled before it, comments and strings stripped. The other three
# dimension-first-blocked files are refused for REAL reasons the rung now
# names: `Fortress.SIUnits` writes `dim Mass default kilogram` and kilogram is
# `gram` with an SI prefix, which is not generated; `dimensionUnitDecl.fss`
# writes `dim Mass default Kilogram` with no such unit declared anywhere.
# THE GROWING-MEMBER CUT: 334 objects, 63 apis. ONE file, zero lost, and the IR
# body of all 397 modules that compiled before is BYTE FOR BYTE unchanged --
# which is the acceptance test, not this count.
#   ProjectFortress/tests/nestedInst.fss   runs, exit 0
# That file exists to test exactly this and says so: "We want to support
# polymorphic recursion without falling off a cliff instantiating types with
# ever deeper nesting. This is extracted from the FingerTree code."
# It was NOT a budget that wanted raising. `Library/FortressLibrary.fsi:1138`
# declares `getter indexValuePairs(): Indexed[\(I,E),I\]` on
# `trait Indexed[\E,I\]`, and a trace of the instantiation queue put 793 of
# the 4096 on that single `Indexed -> Indexed` edge.
#
# AND RUNNING THAT ONE GAINED FILE IS WHAT FOUND THE GETTER DEFECT. It printed
# a blank line where its own source says "1": every accessor was skipped by the
# inferred-return fixpoint, so a getter with an omitted return type returned
# the Void placeholder at exit 0. The count read 397 either way. A compile-count
# check would have called this milestone clean.
# THE SI LIBRARY PATCH: 334 objects, 64 apis. The gain is
# `Library/incomplete/basic/Fortress.SIUnits.fsi`, an API, so OBJECT_FLOOR does
# not move and API_FLOOR does. THE API FLOOR HAS NO SLACK -- an api emits no
# object, so none of the 37 accepted must-fails is inside this count.
# Three defects in the SHIPPED 1.0 library, found by the dimension rule rather
# than by reading, and each is a different KIND:
#   `Current`  is declared NOWHERE in the file; the dimension is
#              `ElectricCurrent`. Five sites.
#   `Second`   is a UNIT of `Time`, not a dimension. Two sites.
#   `kilogram` is `gram` under an SI prefix, and prefixes are not generated;
#              stubbed as a real unit of Mass rather than generating prefixes.
# `Voltage` was NOT one of them and reads like one: it is used at :35 and :38
# and declared at :67, and the checker is not order-sensitive, so the forward
# reference resolves. Renaming it broke its own alias into
# `dim ElectricPotential = ElectricPotential`.
# The `.fss` half does NOT gain, and its wall is real rather than a typo:
# `unit degreeOfAngle degrees: Angle = (180/pi) radian` -- `pi` is a numeric
# CONSTANT and the conversion evaluator knows only units.
#
# THE OBJECT/ANY MERGE, semantics/phase2: 346 objects, 64 apis. `Object` and
# `Any` are 1.0's root traits and this compiler had NEITHER -- `mono.rs` merely
# TOLERATED the two names in a declaration header and `Registry::resolve` then
# refused them, so `x: Object` was `unknown type`. They are seeded in
# `Checker::new` now, `Object` under `Any`, every user trait under `Object`, and
# every object under both.
# THE NUMBERS ARE MEASURED ON THE MERGED TREE AND NOT CARRIED OVER. The branch
# recorded 285 -> 293 against a base nine commits behind this one; on this tree
# the move is 334 -> 346, TWELVE files, zero lost, and the IR body of all 398
# modules that compiled before is BYTE FOR BYTE unchanged -- which is the
# acceptance test, not the count.
#   Compiled9.{Overriding,DiamondOverriding,MultipleOverriding,
#              RedundantOverriding,AsString}   five, and four of them are
#     verified against the legacy's OWN recorded output rather than exit 0:
#     their .test files say run_out_equals=O.m and they print `O.m`.
#   Compiled190  Compiled5.e  Compiled5.j  SimpleTrait  extendObject
#   Trait.Decl   Overview.List
# THE SLACK RISES TO 39 WITH IT. `Compiled5.j.fss` is one of the twelve AND a
# must-FAIL: the legacy refuses it for invalid overloading, we accept it under
# the recorded CLOSED-WORLD exclusion rule, and it is in
# tools/oracle-accepted-must-fail.txt with that reason written out. So the list
# is 38 and the slack is 38 + XXXequalityTesting.fss = 39, and the floor is
# 346 - 39 = 307.
#
# `pi` AS A DIMENSIONLESS CONSTANT: 347 objects, 64 apis. ONE file,
# `Library/incomplete/basic/Fortress.SIUnits.fss`, and the floor rises with it
# to 347 - 39 = 308.
#
# MULTI-DIMENSIONAL ARRAYS: 347 objects, 64 apis. ZERO, AND THE ZERO WAS
# DECLARED BEFORE THE WORK STARTED. Every one of the ten corpus files that
# first-block on a second dimension ALSO writes a `[3 4; 5 6]` matrix
# aggregate, which is a separate unbuilt parser feature -- measured, not
# assumed. The acceptance test here is that nothing was LOST and that the IR
# BODY of all 411 modules that compiled before is byte for byte unchanged, with
# the two unconditional runtime `declare` lines filtered; the gates are where
# the feature is proved.
#
# THE MATRIX AGGREGATE `[3 4; 5 6]`: 358 objects, 64 apis. ELEVEN files, zero
# lost, and the IR body of all 422 modules that compiled before is byte for byte
# unchanged. The floor rises to 358 - 39 = 319.
#   arrayBig  arrayTest1  arrayTest2
#   Expr.Array.{a,b,c,d,e,f}   Overview.Expression.aggregate.{b,c}
# THE PREDICTION WAS ELEVEN AND THE DELIVERY IS ELEVEN, which is the first time
# on this project a first-blocker count has held -- and it held because the
# count was taken with the compiler after `T[m,n]` and `a[i,j]` had already
# landed, so nothing was hiding behind them. The SETS differ: `arrayTest3` needs
# matrix PASTING and stays refused, and three files that were behind other walls
# came through.
# THE ORACLE IS THE SPECIFICATION AND NOT THE COUNT. No `.test` file names any
# of the eleven, so part A of oracle-gate cannot see them; `aggregate.tex:150`
# can -- "then A(1,0) evaluates to 4" -- and arrayTest2 encodes each element's
# coordinates in its own value.
#
# CHARACTER LITERALS AND THE `Char` TYPE: 361 objects, 64 apis. THREE files,
# zero lost, and the IR body of all 422 modules that compiled before is byte for
# byte unchanged.
#   ProjectFortress/other_compiler_tests/Char.fss      the real one, and it
#     PASSES ITS ORACLE: `Char.test` records `run_out_equals=a` and it prints
#     `a`, so a character prints as ITSELF and not as its code point.
#   ProjectFortress/not_working_static_tests/OrWorks.fss
#   ProjectFortress/parser_tests/XXXOprMethod.fss     see the slack below.
#
# THE FIRST-BLOCKER COUNT SAID 61 AND THE CEILING WAS ALWAYS 8. `triage`'s
# `alone*` said so before the work started, and a spike -- `Char` aliased to
# ZZ32 and `'x'` lexed as an int -- delivered FOUR. The three above are what a
# correct implementation delivers, because `XXXforbiddenCharacters.fss` is
# REFUSED by a correct one: it writes a raw tab, which
# `lexical-structure.tex:844-850` makes a static error. Of the 61, three
# compile; the rest move on to `found LGeneric` (15), the matrix aggregate (8)
# and twenty other walls.
#
# THE SLACK RISES TO 40. `XXXOprMethod.fss` is a must-FAIL by the XXX
# convention and has NO `.test`, so the oracle ratchet cannot see it -- the same
# position `XXXequalityTesting.fss` is in. It declares `opr IN(c:Char): Boolean`
# and `opr [i:ZZ32]: ZZ32` as object members and was refused HERE only because
# `Char` was not a type; SPIKE-OPEXPR records operator declarations and nothing
# reads them, so what it compiles to is a program whose two declarations do
# nothing. We do not know 1.0's specific ground for refusing it and are not
# guessing at one. So the list is 38, the slack is 38 + XXXequalityTesting +
# XXXOprMethod = 40, and the floor is 361 - 40 = 321.
OBJECT_FLOOR=321
API_FLOOR=64

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
    # `.claude` holds agent worktrees, which are FULL REPO COPIES. Counting
    # one would read the corpus at several times its real size.
    ds[:] = [x for x in ds if x not in ('.git', 'target', 'fortressc', '.claude')]
    # `examples/` at the ROOT is hand-written demo code, not corpus. Pruned by
    # path and not by name, because SpecData/examples IS corpus -- pruning the
    # name took 137 legacy files out of the metric.
    if d == '.':
        ds[:] = [x for x in ds if x != 'examples']
    files += [os.path.join(d, f) for f in fs if f.endswith(('.fss', '.fsi'))]
files.sort()
c = collections.Counter()
objects = apis = 0
cc = os.environ.get('FORTRESSC', 'fortressc/target/debug/fortressc')
for p in files:
    r = subprocess.run([cc, p, '--emit-obj', '-o', '/dev/null'],
                       capture_output=True, text=True)
    c[r.returncode] += 1
    if r.returncode == 0:
        if p.endswith('.fss'):
            objects += 1
        else:
            apis += 1
print(objects, apis, sum(n for code, n in c.items() if code not in (0, 1)))
PY
)
    read -r objects apis broken <<<"$report"
    if [[ ${objects:-0} -ge $OBJECT_FLOOR ]]; then
        ok "$objects corpus .fss files compile AND EMIT AN OBJECT (floor $OBJECT_FLOOR)"
    else
        bad "${objects:-0} corpus .fss files emit an object" "floor is $OBJECT_FLOOR"
    fi
    if [[ ${apis:-0} -ge $API_FLOOR ]]; then
        ok "$apis corpus .fsi files check (floor $API_FLOOR)"
    else
        bad "${apis:-0} corpus .fsi files check" "floor is $API_FLOOR"
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
    # anything staged during the run -- and the worktree and the index would
    # then agree with each other while both are wrong.
    if ! git -C "$repo" diff --quiet HEAD -- fortressc/crates; then
        printf 'refusing to mutate: fortressc/crates differs from HEAD\n' >&2
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
        runs_and_prints
        evaluated_once
        refusals
        compile_metric
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

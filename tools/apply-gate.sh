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
# DEV-15, 2026-08-22: `ProjectFortress/BirdyLib/Tuple.fsi` -- three bodiless
# `first` declarations at three static arities. 66 apis check; the api floor has
# no slack, so it rises with the count.
# `()` AS A STATIC ARGUMENT, same day: `ProjectFortress/LibraryBuiltin/
# CompilerBuiltin.fsi`, the bootstrap root's own root. 67.
# THE IMPLICIT BUILTIN IMPORT, same day: TWELVE apis gained and TWO lost, both
# of the losses `test_library` support files that redeclare a builtin functional
# method (`RecA.fsi`'s `odd(x:ZZ32)` against `odd(self)` on `ZZ32`). 77, and the
# .fss count does not move at all -- the implicit import is api-side only.
#
# `var`, 2026-08-23: 387 objects and 79 apis. The two apis are
# `Library/String.fsi` and `Library/FortressLibrary.fsi` -- THE BOOTSTRAP ROOT
# AND THE ONE FILE IT WAS WAITING ON -- and the four objects are
# `InitOrderWithMutable`, `ObjectFieldShadowing`, `overloadTest1` and
# `overloadTest2`. Nothing lost, nothing crashed.
# THE API FLOOR MOVES TO 79 AND THE OBJECT FLOOR DOES NOT MOVE, and that
# asymmetry is the one the comment above already states: an api emits no object
# so no accepted must-fail is inside the api count, which is why it can sit at
# the measurement. The object count carries all 38 accepted must-fails, and
# every one of those going the right way -- being REFUSED -- would take the
# count down with it. 321 is the room that ratchet needs.
#
# `Any` AS A TOP TYPE, 2026-08-23: 388 objects and 80 apis. The api is
# `CompilerLibrary/FileSupport.fsi`; the objects are `Compiled3.e`, `.l`, `.n`
# and `.o` -- whose bounds now discharge -- MINUS `Compiled2.f`, `Compiled10.q`
# and `Compiled10.s`, three must-FAIL tests that the component-side
# declaration check now refuses. Net +2 objects and +1 api.
# API_FLOOR MOVES TO 80 AND THE OBJECT FLOOR STILL DOES NOT MOVE, for the
# reason two entries above: no accepted must-fail is inside the api count, and
# all 38 of them are inside the object one.
#
# THE IMPLICIT CORE-api IMPORT, 2026-08-23: 388 objects and 114 apis.
# `basic/components/source-code.tex:305` -- "Every component implicitly imports
# the Fortress core APIs". THIRTY-FOUR apis and no objects, which is what an
# api-side milestone looks like, and twenty-two of them are `Library/` files
# that were waiting on `ZeroIndexed`, `LexicographicOrder` and
# `MonoidReduction`. Nothing lost.
# `self` AS A JUXTAPOSITION OPERAND, 2026-08-23: 391 objects and 114 apis.
# `starts_juxt_operand` had no `KwSelf` arm, so `"Reader on " self.fileName`
# stopped the run and the parser asked for a newline. Three objects --
# `Library/StatDigest.fss`, `long_term_not_working/closures/DottedMethods.fss`
# and `long_term_not_working/overriding/SimpleOverriding.fss` -- and thirteen
# more files moved off `found KwSelf` onto a LATER blocker. Nothing lost.
# NEITHER FLOOR MOVES: the api count did not change, and the object floor keeps
# the 38 accepted must-fails' room. That is precisely why this milestone's
# assertion is the `selfjuxt` case above and its mutation row -- the compile
# metric cannot see a three-file gain over that much slack.
#
# ROW 5 OF THE COLLISION MATRIX, 2026-08-23: 391 objects and 116 apis. Both
# `File.fsi` copies -- `Library/` and `CompilerLibrary/` -- were refused for a
# rule 1.0 does not have, and `Compiled9.c.fss` carries 1.0's own matrix in a
# comment to say so. Nothing lost. API_FLOOR MOVES TO 116 and the object floor
# does not move, for the reason two entries above.
#
# THE NUMERIC HIERARCHY'S MEET RULE, 2026-08-23: 391 objects and 117 apis.
# `ProjectFortress/LibraryBuiltin/FortressBuiltin.fsi`, and it is the same
# defect the Comparison hierarchy had -- `value object NN32` inherits `>`, `<=`
# and `>=` from `StandardTotalOrder[\NN32\]` and from `NN64` and declares
# neither meet. Three `v1 SOURCE CORRECTION` declarations at (NN32, NN32).
# THE API FLOOR IS THE RATCHET FOR A SOURCE CORRECTION: no mutation row can
# reach corpus source, and the api floor sits at the measurement with no slack,
# so reverting the three declarations takes the count below it.
#
# `Self` IS A TYPE VARIABLE, 2026-08-23: 394 objects and 120 apis. 1.0 reserves
# the word and spells it back in exactly two places -- `Type.rats:203` (a
# TypeRef) and `NoNewlineHeader.rats:343` (a static PARAMETER) -- both feeding
# the node an ordinary `Id` feeds. There is no self-type to bound. Accepted in
# those two positions only, and the receiver placeholder renamed to an
# unwritable `$Self` first, or monomorphization substitutes it along with the
# static parameter that shares its name. +6, nothing lost.
#
# A CONSTRUCTOR IS A SIGNATURE, 2026-08-23: 395 objects and 120 apis. One file,
# `ProjectFortress/tests/OverloadConstructor1.fss`, which is 1.0's own positive
# test for matrix cell 5-3 -- and it now BUILDS AND RUNS, printing what its
# source says. The number is beside the point: the milestone is that every one
# of the 394 .fss that already compiled emits BYTE-IDENTICAL IR, measured file
# by file with two binaries, because a set of one lowers to the same
# `call Name$new` as `Target::ObjectNew` did.
# `true : Boolean` IS A PARSE ERROR AND IT COST FIVE apis, 2026-08-23: 395
# objects and 125 apis. `true` and `false` are 1.0 reserved words
# (`Keyword.rats:49`) and `Library/FortressLibrary.fsi:2584-2585` already has
# the same two lines commented out; the `CompilerLibrary/` copy was missed. An
# api that does not PARSE is `unreadable` to the resolver and merges nothing, so
# `List.fsi`, `Map.fsi`, `Pairs.fsi`, `Set.fsi` and `System.fsi` each reported a
# core type that is declared in the very file they could not read.
# LINK 5, THE COMPONENT-SIDE IMPLICIT CORE IMPORT, 2026-08-23: 413 objects and
# 126 apis. +20 gained, -1 lost. `unknown type` as a first blocker goes from 93
# corpus files to 26. Four rules, each with a mutation row: a merged declaration
# is MARKED (api-side only -- a component DEFINES and its objects keep their
# constructors), a merged declaration whose name is a BUILTIN is skipped and its
# supertype edges to builtins dropped, a merged functional method is NOT lifted
# into the importing component, and a merged object is lowered only if its
# layout is buildable.
#
# `AND:` AND `OR:`, 2026-08-23: 416 objects and 126 apis. The conditional
# logical operators, 206 corpus sites, and the three files gained are
# `Compiled9.aj`, `InliningTest3a` and `InliningTest9`. The object floor keeps
# its slack; the api count did not move.
# ANONYMOUS OBJECTS, 2026-08-23: 423 objects and 126 apis. `object ... end` in
# EXPRESSION position, hoisted the way a lambda already was -- a minted
# top-level declaration whose value parameters are the locals its members read,
# and a construction of it left behind -- so no member body is rewritten and
# codegen gains nothing. +7, nothing lost. The api count did not move: an api
# has no bodies to write one in.
# `var` VALUE PARAMETERS AND `:=` FIELD INITIALIZERS, 2026-08-23: 426 objects
# and 126 apis. Two halves of `Variable.rats`: `AbsVarMod` is legal in an
# OBJECT's parameter list, because an object's value parameters ARE its fields,
# and `InitVal = ("=" / ":=")` makes `:=` a field initializer that also declares
# the field mutable. +3, nothing lost. The flag has to survive monomorphization,
# which rebuilt every `Param` and defaulted it -- the declaration parsed and the
# assignment then reported the field immutable.
# RULE 3 RETIRED AND THE MEET RULE IN, 2026-08-23: 426 objects and 126 apis,
# UNCHANGED, and that is the honest number -- twenty-five files moved onto a
# LATER blocker and none moved onto the compile list. `Library/CompilerAlgebra
# .fss` is the one that matters: its `=` ambiguity is discharged and it now
# stops on `unknown name ===`. The compile metric cannot see any of this, so
# the assertions are `meetrule`, `concatbeside`, `badnomeet`,
# `badmergedfunctional` and their four mutation rows.
# THE LIST COMPREHENSION, 2026-08-23: 426 objects and 126 apis, UNCHANGED, and
# that is the measured answer. Nine corpus files moved onto a MORE SPECIFIC
# diagnostic -- a `{_}` bracket, a generator over a collection, an unwritten
# element type -- and none onto the compile list, because every corpus
# comprehension is a SET or MAP one or ranges over a collection. Route 4 was
# ordered with that ceiling already reported, twice. The assertion is
# `listcomp` and its five mutation rows, not the count.
# ARITY FLATTENING, 2026-08-23: 432 objects and 126 apis. +6, nothing lost, and
# ALL 426 PRE-EXISTING OBJECTS EMIT BYTE-IDENTICAL IR -- measured file by file
# with two binaries, the instrument self-tested both ways first. `overloading
# .tex:125` makes `f(x:(A,B))` and `f(a:A,b:B)` one declaration, so the honest
# way to have the first is to lower it into the second: a tuple-typed name
# becomes SEVERAL names and no tuple is ever built. The RESULT direction stays
# refused -- that needs the callee to hand back several values.
# A WRAPPED VALUE-PARAMETER LIST, 2026-08-24: 435 objects and 134 apis, +9 and
# NOTHING LOST, from ONE `skip_newlines_before(&Kind::LParen)`. API_FLOOR moves
# 126 -> 134 with it.
# EIGHT OF THE NINE ARE `BirdyLib/*.fsi` AND THAT IS THE FINDING. They were
# first-blocked on `unknown type DefaultGeneratorImplementation`, which is
# declared in `Library/GeneratorLibrary.fsi` -- a file that did not PARSE. The
# resolver takes an imported api's declarations after PARSING it, so a CHECK
# error in an imported api does not block its importer and a PARSE error does.
# `GeneratorLibrary.fsi` still fails to check; its importers no longer care.
# A diagnostic naming a missing type is not always about the type: ask whether
# the file declaring it parsed.
# MULTI-VALUE RETURN, 2026-08-24: 434 objects and 126 apis, +2 and nothing lost.
# `tupleTypeParam.fss` and `Expr.VarRef.fss`, and the second is the file the
# state file named. THE ESTIMATE WAS "ROUGHLY TEN" AND THE MEASUREMENT IS TWO:
# the other witnesses walk onto later walls -- a missing `DIV`, a generic
# `split` whose result type disagrees, and `only a variable or an array element
# can be assigned to`. Said in the design doc before it was built, not after.
# ALL 432 PRE-EXISTING OBJECTS EMIT BYTE-IDENTICAL IR.
# `badtupleresult.fss` LEFT this list and is `tupleresult.fss`, a positive case.
# THE GENERATOR PROTOCOL, 2026-08-24: 432 objects and 126 apis, UNCHANGED, and
# that was PREDICTED. 172 corpus files write a generator construct and 144 die
# in the PARSER; almost all the rest import a Library module whose `.fss` does
# not compile, so the protocol is NECESSARY for all 172 and SUFFICIENT for none.
# It is a prerequisite three milestones stop on, not a lever, and it was built
# with that written down first rather than discovered afterwards.
# THREE ROWS OF THIS TABLE MOVED AND EVERY ONE WAS RIGHT TO. `listcomp` called
# `.get(i)` on the minted `List`, which is `opr [i]` now -- 1.0's spelling, not
# one this compiler invented. `badcompgenerator.fss` LEFT the refusal list and
# became `compgenerator.fss`, a positive case, because the thing it refused now
# works. And the two binding-condition rows kept their files: what is refused
# MOVED from "the lowering is not implemented" to "a `ZZ32` is not a
# `Condition`", which is permanent where the other was temporary.
# A BINDING CONDITION, 2026-08-23: 432 objects and 126 apis, UNCHANGED, and
# that is what the triage bucket predicted -- `generator-bindings` had 27 first
# blockers and an `alone*` ceiling of ZERO. `expected `then`, found Lt` goes
# from 27 corpus files to NONE and exactly ONE lands on the lowering: the other
# 26 walk on to a later wall. This is a wall-unstacking milestone, and the
# assertions are `badbindingif`, `badbindingwhile` and three mutation rows.
OBJECT_FLOOR=321
API_FLOOR=135

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
ifnoend|a pass\nb pass\nthen taken\nelse taken\nc pass|`end` is elided from an `if` enclosed by parentheses, both branch directions
rr64literal|1.75|an integer literal in RR64 position is a float constant
varvalue|15\n101\n7|a `var` top-level value is storage and an assignment target
anyreturn|7|a trait-typed result still travels through the dispatch table
selfjuxt|Point(3, 4)\n4 done|`self` is an operand in a juxtaposition run
traitfn|42|a top-level function beside a TRAIT of its own name
selftypeparam|42\ntrue|`Self` is a static parameter, and the receiver is still the trait
ctoroverload|107\n7|a constructor and a function of one name are ONE overload set
setterfires|SETTER RAN\n105\n9\nBOX 7\nBASE 7|a declared setter FIRES and an ordinary method does not
mergedaccessor|42|a merged getter name does not capture this file's own method
seqvoperator|true\nfalse|`===` reaches the overload set and is not a spelling of `=`
prefixopword|6\n16\n12\nfalse\n10|a prefix operator word binds tighter than every infix
elidedparam|42|an abstract declaration may elide a parameter NAME
typedascription|5\n10\n3|`e typed T` pins the literal and takes the whole expression
bigoperator|7\n42\n10|`BIG` folds into the operator NAME at the use site too
conditionalops|false\ntrue\ntrue\nfalse|`AND:` and `OR:` are the conditional forms and SHORT CIRCUIT
objectexpr|8\n100\n42|an anonymous `object` captures a local and gets a tag of its own
varfield|7\n11\n2\n108\n4|a `var` value parameter and a `:=` field are BOTH assignable
meetrule|3|a bodiless meet makes a declaration SET valid, and a bodied one runs
concatbeside|Ux\n5|concatenation survives an unrelated declaration of its name
listcomp|5\n10\n16\n4\n7\n6\n32\n5\n36\nq\n40\n40|a list comprehension builds a real monomorphized `List` and it GROWS
tupleflat|7\n30\nHello World!\n7\n7\n0.25|a tuple parameter, a tuple value and a written tuple are FLATTENED
compgenerator|10\n20\n30\n11\n21\n31\n100\n101\n102\n2\n20|a comprehension walks a COLLECTION -- an array, a List and a user object
setcomprehension|5\n0\n1\n2\n3\n4\n3\n1\n3\n3\n2\n6|a SET comprehension deduplicates, keeps first-occurrence order, and walks a collection
comprehensionmerged|3\n1|a MERGED `List` and `Set` lose to the minted collections instead of colliding
setliteral|5\n0\n4\n2\n7\n3\n2\n0\ntrue\nfalse|a SET LITERAL dedups, keeps written order, takes its type from the slot or the brackets, and `IN` answers
varargs|0\n1\n3\n0\n2\n10\n4|a varargs parameter collects its trailing arguments
mapliteral|2\n9\n8\n1\n6\n3\n2\n6\n2|a MAP literal and a MAP comprehension, key replacement and all, on the SET's brackets
arraycomprehension|17\n0\n32\n0\n9|an ARRAY comprehension, indexed, taking its extent from the slot it fills
bindingcond|7\n1\n2\n3\n2\n1\n99|a binding condition yields zero or one value, and `while` re-evaluates it
tupleresult|3\n4\n7\nhi\n10\n20\n30\n7\n16\n41\n42\nmade\n11\n3|a tuple RESULT is an LLVM aggregate, and the source is evaluated ONCE
wrappedparams|7\nhi\n9\n42|an object's value-parameter list may begin on the NEXT line
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
    # THE NAME CARRIES ITS EXTENSION. An api-side refusal has to be listed here
    # too, and every one of the `var` rows below is separated from its mutation
    # by the MESSAGE and not by the exit code: drop the parenthesised-list
    # refusal and `identifier` reports a missing name, drop the
    # delayed-initialization one and the block reports `expected an
    # expression`. Both are still exit 1.
    local name phrase err status
    while IFS='|' read -r name phrase; do
        [[ -z $name ]] && continue
        err=$("$fortressc" "$repo/fortressc/tests/$name" --emit-obj -o /dev/null 2>&1 >/dev/null)
        status=$?
        if refused_cleanly "$status" && [[ $err == *"$phrase"* ]]; then
            ok "$name is refused (exit $status)"
        else
            bad "$name is refused" "status $status: $err"
        fi
    done <<'CASES'
juxtnary.fss|a juxtaposition of 3 elements led by a function is not implemented
juxtsingleton.fss|neither multiplication nor concatenation
localfn.fss|a local function declaration is not implemented
badlocalfntyped.fss|a local function declaration is not implemented
localfnspaced.fss|a local function declaration is not implemented
localfnspaceduntyped.fss|a local function declaration is not implemented
badifnoend.fss|an `if` whose `end` is elided must have an `else`
badifnoendelif.fss|an `if` whose `end` is elided must have an `else`
badelidedbody.fss|this declaration has a BODY
badelidedfield.fss|an object's value parameters are its FIELDS
baduntypedparam.fss|the parameter `v` has no written type
baduntypedfield.fss|the field `x` has no written type
badchainsense.fss|chained ordering operators must have the same sense
badarrowtype.fss|an arrow type is not implemented
badvartuple.fsi|a parenthesised variable list declares a tuple of variables
badlocalvarnoinit.fss|is declared with no initializer
badvarnotype.fss|a mutable top-level value must write its type
badanyscalar.fss|has no representation in one
baddeclonlyoverload.fss|`g` is ambiguous for (Both)
badanyreturn.fss|a result of a wider type
badvoidarg.fss|`()` has no value, so it cannot be stored in a parameter of a wider type
badsingletonfn.fss|`Marker` is defined twice
badctordup.fss|`Box` is declared twice on the same argument types (ZZ32)
badctortie.fss|`Pair` is ambiguous for (Both, Both)
badsingletoncall.fss|`Marker` is a singleton object; write `Marker`, not `Marker(...)`
badselfvalue.fss|reserved word `Self` is not in the implemented subset
badsettercompound.fss|is a setter, so `o.n := e` is a call
badvarargsany.fss|is not a supported array element type
badvarargsnotlast.fss|is followed by the parameter `b`
badvarargstwice.fsi|is followed by the parameter `b`
badthrowscalar.fss|`throw` can only throw objects of Exception type, and this expression is of type ZZ32
badthrownotexception.fss|and this expression is of type FooExn
badmergedfunction.fss|unknown name `gcd`
badmergedconstruct.fss|comes from an imported api, which declares it and does not define it
badtry.fss|`try` parses and its lowering is not implemented
badseqv.fss|unknown name `===`
badbigand.fss|is not one of the reduction operators this lowering reaches
badcomprehension.fss|expected ZZ32, found ZZ64
badmutablecapture.fss|is mutable, and a closure captures it BY VALUE here
badimmutableparam.fss|field `w` is immutable
badnomeet.fss|is ambiguous for (O, O)
badprefixand.fss|expected an expression, found OpWord("AND")
badasif.fss|is a type ASSUMPTION
badtypedsubtype.fss|an integer literal cannot be used where Boolean is required
badmergedfunctional.fss|where Cup is required
badsetcomp.fss|element type is not written anywhere
badbracketcomp.fss|comprehension parses and its lowering is not implemented
badcompsettaken.fss|mints its own `Set`, and this component declares one of its own
badmapcomprehension.fss|whose body must be written `k |-> v`
badsetliteral.fss|set literal's element type is not written anywhere
badmapmixed.fss|is one entry of a map and is not a value on its own
badmappingvalue.fss|a mapping body, which only the `{ }` brackets build
badarraycompextent.fss|takes its EXTENT from the binding it fills
badarraycompindex.fss|body must be written `index |-> value`
badcompelement.fss|element type is not written anywhere
badcomplisttaken.fss|mints its own `List`, and this component declares one of its own
badtuplevalue.fss|is a tuple and tuples are FLATTENED here
badtuplemutable.fss|a mutable tuple binding is not flattened
badtupleoverload.fss|`g` is declared twice on the same argument types (ZZ32, ZZ32)
badtuplewhole.fss|a tuple expression is not implemented in this subset
badbindingif.fss|is not a condition here
badbindingwhile.fss|is not a condition here
CASES

    # `badvaluebinding.fss` LEFT THIS LIST when component-level values landed,
    # and became its own POSITIVE case. It used to assert a refusal, because a
    # value carried as a nullary function compiles the program and SILENTLY
    # NEVER RUNS the initializer. Now the initializer runs, and the assertion
    # is the ORDER: it is emitted inside `main` after `fortress_runtime_init`
    # and before `run`, so its line must come FIRST.
    local out
    if "$fortressc" "$repo/fortressc/tests/badvaluebinding.fss" \
            -o "$build/badvaluebinding" >/dev/null 2>&1; then
        out=$("$build/badvaluebinding" 2>&1)
        if [[ $out == "INITIALIZER RAN"$'\n'"run"$'\n'"7" ]]; then
            ok 'a component-level initializer runs, and runs BEFORE `run`'
        else
            bad 'a component-level initializer runs, and runs BEFORE `run`' \
                "got $(printf '%s' "$out" | tr '\n' '/')"
        fi
    else
        bad 'badvaluebinding.fss compiles' \
            "$("$fortressc" "$repo/fortressc/tests/badvaluebinding.fss" 2>&1 \
               | grep -v '^fortressc: ' | head -1)"
    fi
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

# THE IMPLICIT BUILTIN IMPORT, and the three defects landing it exposed.
# `library/structure.tex:16-18`: the default libraries "are automatically
# imported by every Fortress component and API". The api half only -- the
# component half would give a merged object a type tag and construct a merged
# singleton in `main`, perturbing the IR of every module that already compiles.
implicit_builtin_import() {
    printf '== the implicit builtin import ==\n'
    local err status out

    err=$("$fortressc" "$repo/fortressc/tests/implicitcore.fsi" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok 'an api names `Maybe` and `RR32` with no import written'
    else
        bad 'an api names `Maybe` and `RR32` with no import written' "status $status: $err"
    fi

    err=$("$fortressc" "$repo/fortressc/tests/implicitbuiltin.fsi" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok 'an api names `RR32` with no import written'
    else
        bad 'an api names `RR32` with no import written' "status $status: $err"
    fi

    # AND THE COMPONENT HALF IS IN NOW, TYPES ONLY. Built and RUN, because the
    # whole risk of the component half is that a merged declaration shadows a
    # BUILTIN of the same name: the literal, the `String` and the `||` in this
    # fixture all have to keep typing against the compiler's own.
    if "$fortressc" "$repo/fortressc/tests/implicitbuiltin.fss" \
            -o "$build/implicitbuiltin" 2>"$build/implicitbuiltin.err"; then
        out=$("$build/implicitbuiltin" 2>&1)
        if [[ $out == "7"$'\n'"still a String" ]]; then
            ok 'a COMPONENT gets the core apis, and the BUILTIN keeps its name'
        else
            bad 'a COMPONENT gets the core apis, and the BUILTIN keeps its name' \
                "$(printf '%s' "$out" | tr '\n' '/')"
        fi
    else
        bad 'implicitbuiltin.fss compiles' \
            "$(grep -v '^fortressc: ' "$build/implicitbuiltin.err" | head -1)"
    fi

    # NOT INTO THE BUILTIN ITSELF. Observable in the count the driver prints:
    # `CompilerBuiltin.fsi` writes two imports of its own and must resolve two.
    out=$("$fortressc" "$repo/ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi" \
            --emit-obj -o /dev/null 2>&1)
    if [[ $out == *'resolved 2 api(s)'* ]]; then
        ok 'the builtin does not implicitly import itself'
    else
        bad 'the builtin does not implicitly import itself' "$out"
    fi

    # A STATIC PARAMETER IN A `comprises` CLAUSE IS NOT A TYPE NAME.
    err=$("$fortressc" "$repo/fortressc/tests/staticcomprises.fsi" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok '`trait Equality[\T\] comprises T` beside an unrelated `trait T`'
    else
        bad '`trait Equality[\T\] comprises T` beside an unrelated `trait T`' "status $status: $err"
    fi

    # A MERGED `comprises` CLAUSE IS NOT THE IMPORTER'S TO ANSWER FOR.
    err=$("$fortressc" "$repo/fortressc/tests/comprisesuser.fsi" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok 'an imported clause naming a name this file also declares'
    else
        bad 'an imported clause naming a name this file also declares' "status $status: $err"
    fi

    # AND WHEN THE TWO DECLARATIONS TAKE A DIFFERENT NUMBER OF STATIC
    # PARAMETERS, THE IMPORTER DOES NOT SPEAK FOR THE api. The block above is
    # the SAME-arity case and it still drops: an importer's declaration wins
    # the name, because the shipped libraries are layered COPIES and
    # identifying them is what the layering is for. A different arity is proof
    # they are not copies -- no substitution makes `[\R\]` and `[\R,L\]` one
    # declaration -- so the api keeps its own under an unwritable name and its
    # own references follow it.
    err=$("$fortressc" "$repo/fortressc/tests/scopedarityuse.fsi" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok 'a merged reference keeps the arity the api declared it at'
    else
        bad 'a merged reference keeps the arity the api declared it at' "status $status: $err"
    fi

    # THE CORPUS WITNESS, and it is the file the fixture above is a miniature
    # of. `Library/GeneratorLibrary.fsi` declares `ReductionWithZeroes[\R\]`;
    # `FortressLibrary.fsi:1871` declares `[\R,L\]` and six of its own objects
    # name it at two. Asserted on the real file because the fixture cannot
    # prove the source path, the implicit core import and the merge order all
    # line up on it.
    err=$("$fortressc" "$repo/Library/GeneratorLibrary.fsi" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok '`Library/GeneratorLibrary.fsi` checks beside the core library'
    else
        bad '`Library/GeneratorLibrary.fsi` checks beside the core library' "status $status: $err"
    fi

    # TWO REQUESTS FOR ONE api AT DIFFERENT NAME SETS ARE TWO REQUESTS.
    err=$("$fortressc" "$repo/fortressc/tests/twoimports.fsi" \
            --emit-obj -o /dev/null 2>&1 >/dev/null)
    status=$?
    if [[ $status -eq 0 ]]; then
        ok 'both name sets land when one api is imported twice'
    else
        bad 'both name sets land when one api is imported twice' "status $status: $err"
    fi

    # AND THE DIAGNOSTIC IS THE SAME EVERY RUN. `comprises` reported the FIRST
    # violation out of a `HashMap`, so the same binary named `T` on one run and
    # `S` on the next. No mutation row can reach this -- swapping the iteration
    # back is not a one-line change -- so it is asserted by repetition.
    local first= this= same=1 i
    for i in 1 2 3 4 5; do
        this=$("$fortressc" "$repo/ProjectFortress/parser_tests/XXXComprisesHidden.fss" \
                --emit-obj -o /dev/null 2>&1 >/dev/null | head -1)
        if [[ -z $first ]]; then
            first=$this
        elif [[ $this != "$first" ]]; then
            same=0
        fi
    done
    if [[ $same -eq 1 && -n $first ]]; then
        ok 'the `comprises` diagnostic is the same on five runs'
    else
        bad 'the `comprises` diagnostic is the same on five runs' "$first vs $this"
    fi
}

# AN UNCAUGHT `throw` HALTS, and every throw is uncaught in this subset because
# there is no `catch`. Three separate claims and each has its own assertion: the
# work before the throw runs, the OPERAND runs (a throw is not a no-op that
# skips its argument), and the halt NAMES the exception.
throw_halts() {
    printf '== an uncaught throw halts ==\n'
    if ! "$fortressc" "$repo/fortressc/tests/throwhalts.fss" \
            -o "$build/throwhalts" 2>"$build/throwhalts.err"; then
        bad 'throwhalts.fss compiles' "$(cat "$build/throwhalts.err")"
        return
    fi
    local out err status
    out=$("$build/throwhalts" 2>"$build/throwhalts.run")
    status=$?
    err=$(cat "$build/throwhalts.run")
    if [[ $status -eq 1 ]]; then
        ok 'an uncaught throw exits 1'
    else
        bad 'an uncaught throw exits 1' "status $status"
    fi
    if [[ $out == "7"$'\n'"OPERAND RAN" ]]; then
        ok 'the work before the throw ran, and so did the operand'
    else
        bad 'the work before the throw ran, and so did the operand' \
            "stdout: $(printf '%s' "$out" | tr '\n' '/')"
    fi
    if [[ $err == *'uncaught exception NotFound'* ]]; then
        ok 'the halt names the exception'
    else
        bad 'the halt names the exception' "stderr: $err"
    fi
}

# ROW 5 OF `Compiled9.c.fss`'s COLLISION MATRIX. A top-level function may share
# its name with a TRAIT (5-1) and with an OBJECT CONSTRUCTOR (5-3), and may not
# with a SINGLETON object (5-2). The two acceptances are apis because a call is
# a separate question -- `badctorcall.fss` in the refusals is the other half --
# and BOTH ORDERS are written: a fixture that puts the function first only tests
# the order the shipped library happens to use.
collision_matrix() {
    printf '== the collision matrix, row 5 ==\n'
    local name err status
    while IFS='|' read -r name label; do
        [[ -z $name ]] && continue
        err=$("$fortressc" "$repo/fortressc/tests/$name" --emit-obj -o /dev/null 2>&1 >/dev/null)
        status=$?
        if [[ $status -eq 0 ]]; then
            ok "$label"
        else
            bad "$label" "status $status: $err"
        fi
    done <<'CASES'
ctorfn.fsi|a function declared BEFORE the object constructor of its name
ctorfnrev.fsi|and a function declared AFTER it
CASES
}

MUTATIONS=(
  # AT MOST ONE VARARGS AND NO ORDINARY PARAMETER AFTER IT. The guard has ZERO
  # corpus exercisers -- the seven sites that write a varargs followed by
  # something die at a KEYWORD parameter's `=` inside `params` first -- so
  # `badvarargsnotlast.fss` and `badvarargstwice.fsi` are the only things
  # holding it, and this row is what proves they still reach it. `nth(9)` on an
  # iterator this short is None, so the guard returns Ok and both fixtures
  # compile. The pattern is bar-free on purpose: the row splits on IFS.
  'crates/parser/src/lib.rs|        let Some(after) = rest.next() else {|        let Some(after) = rest.nth(9) else {|stop refusing a parameter that follows a varargs one'
  # ARITY IS IN THE STAMP'S NAME, and that is what dissolves the mangle
  # collision without touching `mangle`. Collapse the arities to one name and
  # `varargs.fss` -- which calls `count` at 0, 1 and 3 -- gets one declaration
  # for three different parameter lists.
  'crates/types/src/mono.rs|    format!("{name}$va{arity}")|    format!("{name}$va")|collapse every varargs arity onto one stamp name'
  # A VARARGS TEMPLATE IS NOT EMITTED. Leave it in and it survives with its
  # varargs flag dropped by `build_signatures`, i.e. as `f(es: ZZ32)`, so its
  # own body stops checking: `length(es)` reports `expected an array, found
  # ZZ32`. This row is what holds that.
  'crates/types/src/mono.rs|                if self.is_varargs_template(decl) {|                if self.is_varargs_template(decl) && false {|emit the varargs template beside its own arity stamps'
  # A WRAPPED VALUE-PARAMETER LIST. One line, +9 corpus files, and the risk it
  # carries is the opposite one: skipping newlines before `(` must not swallow a
  # MEMBER. `wrappedparams.fss`'s third object exists for exactly that.
  'crates/parser/src/lib.rs|        self.skip_newlines_before(&Kind::LParen);|        let _ = Kind::LParen;|stop reading a value-parameter list that begins on the next line'
  # `AND:` AND `OR:` ARE THE CONDITIONAL FORMS. The colon must be GLUED and it
  # must be CONSUMED -- left behind it is `expected an expression, found Colon`,
  # which is where 27 corpus files stopped.
  'crates/parser/src/lib.rs|        let colon_is_glued_on =|        let colon_is_glued_on = false; let _unused =|stop reading a glued colon as the conditional operator'
  # A COMPREHENSION PARSES. Two axes: the bare-`|` separator, and the static
  # arguments that go INSIDE the opener.
  'crates/parser/src/lib.rs|                if self.comprehension_bar_here() {|                if false {|stop reading the bare bar as a comprehension separator'
  'crates/parser/src/lib.rs|            static_args = self.type_args()?;|            static_args = Vec::new();|refuse static arguments inside an enclosing opener'
  # `BIG` FOLDS INTO THE OPERATOR NAME AT THE USE SITE. Without the arm it is
  # a bare reserved word again and the thirteen files that write one are filed
  # under the parser instead of under what they actually need.
  'crates/parser/src/lib.rs|            Kind::Reserved("BIG") => self.big_operator(),|            Kind::Reserved("BIGX") => self.big_operator(),|refuse every other `BIG` as a bare reserved word again'
  # `===` IS NOT `=`. Reading it as `=` gets the numeric case right by luck
  # and the reference case wrong by construction.
  'crates/parser/src/lib.rs|            Kind::EqEqEq => "===",|            Kind::EqEqEq => "==",|send `===` to a different operator name'
  # `try` PARSES AND IS REFUSED BY NAME. Without the parser arm it is a bare
  # reserved-word refusal again, which files it under the parser instead of
  # under exceptions and says nothing about what is missing.
  'crates/parser/src/lib.rs|Kind::Reserved("try") => self.try_expr(),|Kind::Reserved("tryx") => self.try_expr(),|refuse `try` at the parser again, as a bare reserved word'
  # AN UNCAUGHT `throw` HALTS. Three axes: refuse it at the parser again, take
  # away its bottom type so it cannot stand in value position, and stop the
  # halt naming the exception.
  'crates/parser/src/lib.rs|Kind::Reserved("throw") => {|Kind::Reserved("throwx") => {|refuse `throw` at the parser again'
  'crates/types/src/lib.rs|let bottom = expected.unwrap_or(Type::Void);|let bottom = Type::Void;|take the bottom type away from a throw in value position'
  'runtime/shims.c|fortress: uncaught exception %s|fortress: something happened %s|stop the halt naming the exception'
  # A DECLARED SETTER FIRES. Three axes: never route to it (the store goes
  # straight to the slot, which is the defect this closes), route to ANY dotted
  # method of the name (an ordinary `n(x: T)` must not capture `o.n := e`), and
  # read the written modifier as the wrong KIND.
  'crates/types/src/lib.rs|if self.setters.contains(name) {|if false {|store into the field instead of calling the setter'
  'crates/types/src/lib.rs|if self.setters.contains(name) {|if self.methods.contains_key(name) {|let an ordinary dotted method capture an assignment'
  'crates/parser/src/lib.rs|            Some(Accessor::Setter)|            Some(Accessor::Getter)|read the `setter` modifier as `getter`'
  # `Self` IS A TYPE VARIABLE, NOT A SELF-TYPE. Three axes: the receiver
  # placeholder must be UNWRITABLE or monomorphization substitutes it along
  # with the static parameter that shares its name, and the two positions
  # 1.0's grammar spells `Self` in must both accept it.
  'crates/ast/src/lib.rs|pub const SELF_TYPE_PLACEHOLDER: &str = "$Self";|pub const SELF_TYPE_PLACEHOLDER: &str = "Self";|let monomorphization substitute the receiver placeholder'
  'crates/parser/src/lib.rs|let (name, span) = self.type_name("a static parameter name")?;|let (name, span) = self.identifier("a static parameter name")?;|refuse `Self` as a static parameter name'
  'crates/parser/src/lib.rs|let (mut name, span) = self.type_name("a type name")?;|let (mut name, span) = self.identifier("a type name")?;|refuse `Self` in type position'
  # AND IT IS STILL A RESERVED WORD EVERYWHERE ELSE. `Self = 5` and
  # `object Self` are errors in 1.0 and stay errors here, which is why this
  # is a narrow acceptance and not a line deleted from RESERVED.
  'crates/lexer/src/token.rs|    "Self",|    "Zelf",|stop reserving `Self` at all, so it is an ordinary name everywhere'
  # ROW 5 OF THE COLLISION MATRIX, all three cells. The first row puts the old
  # over-broad rule back -- the whole type namespace, which refuses 5-1 and 5-3
  # too -- and the second drops the singleton cell it was right about.
  'crates/types/src/lib.rs|if self.registry.is_singleton(&f.name) {|if self.registry.is_object(&f.name) {|refuse a function beside the object CONSTRUCTOR of its name'
  'crates/types/src/lib.rs|if self.registry.is_singleton(&f.name) {|if false {|accept a function beside a SINGLETON object of its name'
  # A CONSTRUCTOR IS A SIGNATURE, three axes. Registering none puts the
  # constructor back above the overload set; registering them in an api makes
  # the shipped `File.fsi` a duplicate; registering a SINGLETON hands codegen
  # a `Marker$new` nothing defines.
  'crates/types/src/lib.rs|        self.declare_constructors(component)?;|        let _unregistered = component;|register no constructor in the overload set'
  'crates/types/src/lib.rs|        let component_side = !component.is_api;|        let component_side = true;|register a constructor in an api too'
  'crates/types/src/lib.rs|            if info.singleton {|            if false {|register a constructor for a SINGLETON object'
  # `self` AS A JUXTAPOSITION OPERAND. The compile metric cannot hold this --
  # three files over the object floor's deliberate slack -- so the `selfjuxt`
  # case is the assertion, and this row is what proves the case can refuse.
  'crates/parser/src/lib.rs|if matches!(self.peek_kind(), Some(Kind::KwSelf)) {|if matches!(self.peek_kind(), Some(Kind::Eof)) {|stop a juxtaposition run before a `self` operand'
  # THE IMPLICIT CORE-api IMPORT, two axes. The layering is one WORD.
  'crates/driver/src/resolve.rs|const IMPLICITLY_IMPORTED: [&str; 2] = ["CompilerBuiltin", "FortressLibrary"];|const IMPLICITLY_IMPORTED: [&str; 1] = ["CompilerBuiltin"];|implicitly import the builtin and NOT the library above it'
  'crates/driver/src/resolve.rs|const IMPLICITLY_IMPORTED: [&str; 2] = ["CompilerBuiltin", "FortressLibrary"];|const IMPLICITLY_IMPORTED: [&str; 2] = ["FortressLibrary", "CompilerBuiltin"];|order the core apis the other way, so the builtin takes the library'
  # THE RESULT DIRECTION. Its own line because the parameter guard on the line
  # above it does NOT speak for it -- a cell winner returning a `String` where
  # the caller reads `Any` compiled AND RAN.
  'crates/types/src/lib.rs|self.result_fits_its_slot(winner.returns, returns, span)?;|let _unguarded = returns;|let a result with no representation out of a dispatch cell'
  # `Any` AS A TOP TYPE, three axes. The first two are the two halves of one
  # decision -- the TYPE says yes and the STORAGE says no -- and inverting
  # either alone must go red.
  'crates/types/src/registry.rs|if wanted == "Any" {|if false {|stop making `Any` the top type'
  'crates/types/src/lib.rs|&& !Self::occupies_a_trait_slot(found)|&& false|let a scalar into a trait slot'
  'crates/types/src/lib.rs|self.overloads_are_unambiguous()?; // component side|let _unchecked = 0;|check a component overload set only where it is called'
  # `var` AT DECLARATION LEVEL, three axes. Every target line is bar-free, and
  # two of the three are caught by the MESSAGE rather than the exit code.
  'crates/parser/src/lib.rs|Some(Kind::KwVar) => Ok(Decl::Value(self.value_decl(modifiers, true)?)),|Some(Kind::KwVar) => Ok(Decl::Value(self.value_decl(modifiers, false)?)),|read a declaration-level `var` as IMMUTABLE'
  'crates/parser/src/lib.rs|let parenthesised_list = self.at(&Kind::LParen);|let parenthesised_list = false;|stop refusing a parenthesised variable list by name'
  'crates/parser/src/lib.rs|_ if modifier && ty.is_some() => {|_ if false => {|stop refusing a local `var` with no initializer by name'
  'crates/types/src/lib.rs|if self.lookup(name).is_some() {|if false {|drop the shadowing guard on a function element'
  # LINK 5, five axes. The old pair of rows here toggled a component-side
  # early return that no longer exists: the component half is IN, and what
  # holds it up is which declarations are marked, which are skipped, which
  # are lifted and which are lowered.
  'crates/driver/src/resolve.rs|            if from_api {|            if false {|stop marking an api declaration as merged, so all of them are lowered'
  'crates/driver/src/resolve.rs|            if from_api && !component.is_api {|            if false && !component.is_api {|let a merged declaration shadow the builtin of its own name'
  # RETIRED 2026-08-23: this row toggled the ban on lifting a merged
  # functional method, and the ban is GONE. Its replacement puts the ban BACK,
  # and lives with the rest of Phase B at the end of the table.
  'crates/types/src/lib.rs|            if info.merged {|            if false {|give an unlowerable merged object a constructor anyway'
  'crates/types/src/lib.rs|            let merged_names_are_not_ours = merged_decl(decl) && !component.is_api;|            let merged_names_are_not_ours = false;|let a merged accessor name capture a method the importing file declares'
  # RETARGETED 2026-08-23: the guard is per-name now, and it is a `break` --
  # a core api takes the ones BELOW it and no more. Dropping it lets the
  # builtin implicitly import itself AND the layer above it.
  'crates/driver/src/resolve.rs|if component.name == name {|if false {|let a core api implicitly import itself and the layer above it'
  'crates/driver/src/resolve.rs|let key = (name.clone(), import.items.clone());|let key = (name.clone(), ImportItems::OnDemand);|key the resolver on the api name alone again'
  # A MERGED DECLARATION THAT LOSES TO A DIFFERENT ARITY KEEPS ITS IDENTITY.
  # Putting the drop back is what `Library/GeneratorLibrary.fsi` and
  # `scopedarityuse.fsi` both died of: the api's own two-argument reference
  # re-points at the importer's one-parameter declaration.
  'crates/driver/src/resolve.rs|            if mine == theirs {|            if true {|drop a merged declaration that loses to a DIFFERENT static arity'
  # AND A STATIC PARAMETER OF THE CONTESTED NAME MUST NOT BE CAPTURED. Without
  # the carve-out `Shadower[\Zeroed\]`'s parameter keeps its name while
  # `pick(): Zeroed` becomes `pick(): $scopedarityapi$Zeroed`, which is a
  # two-parameter trait named with no static arguments.
  'crates/driver/src/resolve.rs|            if !bound.contains(name.as_str()) {|            if true {|let the rename capture a static parameter of the contested name'
  # THE PREFIX OPERATOR WORD. Three separate claims, three rows.
  # The KILL SWITCH: without the arm, `DBL 3` is `expected an expression`.
  'crates/parser/src/lib.rs|        if !matches!(self.table_fixity_at(self.pos), TableFixity::Prefix) {|        if true {|stop reading an operator word as a prefix operator'
  # THE ORDER IS LOAD BEARING. `primary` is DOWNSTREAM of `unary`, so without
  # this guard `SUM[i <- 1:4] i` is taken as a prefix operator over a subscript
  # and `big_reduction` never runs.
  'crates/parser/src/lib.rs|        if self.big_reduction_here(0) {|        if false {|let the prefix arm steal a BIG reduction'
  # AND `AND`/`OR`/`NOT` HAVE REAL CODEGEN. Taken as prefix words they become a
  # call to a function nobody declared.
  'crates/parser/src/lib.rs|        if CODEGEN_OPERATOR_WORDS.contains(&word) {|        if false {|let AND and OR be taken as prefix operator words'
  # THE TYPE ANNOTATIONS. One production, two keywords, THREE claims.
  'crates/parser/src/lib.rs|                Some(Kind::Reserved("typed")) => false,|                Some(Kind::Reserved("nonesuch")) => false,|stop reading `typed` as a type ascription'
  # `asif` IS NOT `typed`. Treating the assumption as the ascription is a SILENT
  # WRONG ANSWER for a dispatching receiver, so the refusal is the invariant.
  'crates/types/src/lib.rs|                if *assumption {|                if false {|let `asif` be treated as the static ascription'
  # AND THE OPERAND IS CHECKED AGAINST THE ASCRIBED TYPE. Without that the
  # ascription only RELABELS: `5 typed ZZ64` stays a ZZ32 constant.
  'crates/types/src/lib.rs|                let inner = self.expr(value, Some(want))?;|                let inner = self.expr(value, None)?;|check an ascription operand without the ascribed type'
  # A LOCAL FUNCTION WITH TYPED PARAMETERS reaches the refusal the untyped one
  # already reached. Without the probe it dies in the parser at the `:`.
  'crates/parser/src/lib.rs|        self.params(false).ok()?;|        return None;|stop reading a typed parameter list as a local function header'
  # THE ELIDED PARAMETER NAME. Three claims, three rows.
  'crates/parser/src/lib.rs|            if !named && !mutable {|            if false {|stop taking a bare TYPE as a parameter whose name is elided'
  # AND BOTH HALVES OF THE SPEC SENTENCE. The type may NOT be omitted, so
  # elision is refused where it is not licensed.
  'crates/parser/src/lib.rs|            if !named && mutable_allowed {|            if false {|let an object elide a FIELD name'
  'crates/parser/src/lib.rs|        if body.is_none() {|        if true {|let a declaration WITH A BODY elide a parameter name'
  # AND WHICH RULE IT BROKE IS DECIDED BY THE SHAPE. A bare identifier is an
  # untyped PARAMETER needing inference (`Parameter.rats:96`); only something
  # structured, which cannot be a `BindId`, is an attempted elision. Inverting
  # the test swaps both messages at once, so both fixtures speak.
  'crates/parser/src/lib.rs|            TypeRef::Named { name, args, .. } if args.is_empty() => Some(name.clone()),|            TypeRef::Named { name, args, .. } if !args.is_empty() => Some(name.clone()),|swap the untyped-parameter and elided-name diagnostics'
  # THE SAME SPLIT ON AN OBJECT'S VALUE PARAMETERS, where elision is not even a
  # possible reading -- `TraitObject.rats:185` routes them through the same
  # `Params`. Without the bare-name reading every untyped FIELD is called an
  # elision again.
  'crates/parser/src/lib.rs|        Some((*n).to_owned())|        None|stop reading a bare identifier as an untyped field'
  # `end` MAY BE ELIDED FROM AN `if` IMMEDIATELY ENCLOSED BY PARENTHESES,
  # `if.tex:71-73`. THREE CLAIMS, THREE ROWS: the licensing test, the terminator
  # set that lets the block reach the closing parenthesis at all, and the
  # `else` the same spec sentence requires.
  'crates/parser/src/lib.rs|                Some(Kind::LParen) => return true,|                Some(Kind::LParen) => return false,|stop licensing the elided `end` inside parentheses'
  'crates/parser/src/lib.rs|            arms.push(Kind::RParen);|            let _unused = Kind::RParen;|run an if-block onto the closing parenthesis instead of stopping at it'
  'crates/parser/src/lib.rs|            if !saw_else {|            if false {|let an elided `end` go without the `else` the spec requires'
  'crates/types/src/comprises.rs|if r.is_own_static(sub) {|if false {|read a static parameter in a comprises clause as a type name'
  'crates/types/src/comprises.rs|if !r.clause_is_ours() {|if false {|report a merged comprises clause against the importing file'
  'crates/parser/src/lib.rs|if is_literal(operand) {|if true {|duplicate every chain operand instead of binding it'
  'crates/parser/src/lib.rs|Some((seen, earlier)) if seen != this => {|Some((seen, earlier)) if false => {|drop the chain sense check'
  # THE LOCAL FUNCTION HEADER, two claims. The guard is what tells a
  # declaration from a discarded equality, and the parenthesis need NOT be
  # glued to the name -- `LocalDecl.rats:75` separates them with `w`.
  'crates/parser/src/lib.rs|        let named = matches!(self.peek_kind(), Some(Kind::Ident(_)));|        let named = false;|drop the local function declaration guard'
  'crates/parser/src/lib.rs|        named && matches!(self.peek_ahead(1), Some(Kind::LParen))|        named && matches!(self.peek_ahead(1), Some(Kind::LParen)) && self.glued_left(self.pos + 1)|require the local function parameter list to be glued to its name again'
  'crates/types/src/lib.rs|Decl::Value(v) => Some(v),|Decl::Value(_) => None,|see no component-level values at all, so no initializer runs'
  # AN ANONYMOUS OBJECT, three axes: the parser must reach the expression at
  # all, the hoist must carry the locals its members read, and each one must
  # get a NAME of its own or two of them are one declaration and one tag.
  'crates/parser/src/lib.rs|            Kind::KwObject => {|            Kind::Eof => {|refuse `object` in expression position again'
  'crates/types/src/closure.rs|free_names(&probe, &mut Vec::new(), &mut free);|let _uncaptured = &probe;|hoist an anonymous object without the locals its members read'
  'crates/types/src/closure.rs|let object_name = format!("obj${index}");|let object_name = "obj$".to_owned();|mint one name for every anonymous object in the component'
  # A CAPTURE COPIES, so closing over a MUTABLE local is refused by name at
  # BOTH hoists. Reading one compiled and printed the value at construction
  # time; 1.0 captures the cell. Zero corpus files, measured.
  'crates/types/src/closure.rs|                (t, true) => scope.declare_mutable(name, t),|                (_t, true) => scope.declare_opaque(name),|forget that a local was declared mutable, so a closure may copy it'
  # AN OBJECT'S VALUE PARAMETERS ARE ITS FIELDS, so `var` belongs in ITS
  # parameter list and nowhere else, and the flag has to survive
  # monomorphization -- dropping it there made the declaration parse and the
  # assignment report the field immutable.
  'crates/parser/src/lib.rs|let params = self.params(true)?;|let params = self.params(false)?;|refuse `var` in an object parameter list again'
  'crates/parser/src/lib.rs|let mutable = mutable_allowed && self.at(&Kind::KwVar);|let mutable = false;|read a `var` value parameter as immutable'
  'crates/types/src/mono.rs|mutable: p.mutable,|mutable: false,|drop the mutable flag in monomorphization'
  # AND `:=` IS A FIELD INITIALIZER, which is the other half of the same
  # grammar rule. Refusing it, and taking it as immutable, are separate.
  'crates/parser/src/lib.rs|let assigned = self.at_field_initializer();|let assigned = false;|refuse `:=` where a field initializer goes'
  'crates/parser/src/lib.rs|            is_a_mutable_field = true;|            is_a_mutable_field = false;|read a `:=` field as immutable'
  # RULE 3 IS RETIRED AND THE MEET RULE IS IN, four axes. The concatenation
  # fallback tested for the NAME `||` rather than for a declaration that could
  # reach a pair of Strings, and that one defect was the whole cost of the ban
  # on lifting a merged functional method. The operator cannot be written in a
  # row -- `IFS` splits on its own spelling -- so it is a named const.
  'crates/types/src/lib.rs|if name == CONCAT && !self.concat_applies_to(&STRING_PAIR) {|if name == CONCAT && !self.functions.contains_key(CONCAT) {|take concatenation away from any declaration of the name'
  'crates/types/src/lib.rs|            if self.concat_applies_to(&statics) {|            if false {|refuse a non-String pair instead of letting a declaration have it'
  'crates/types/src/lib.rs|let every = self.applicable(&group, &tuple, false);|let every = Vec::new();|stop the meet rule discharging a tie'
  'crates/types/src/lib.rs|            let liftable = members_of(decl);|            let liftable = if merged_decl(decl) { &[][..] } else { members_of(decl) };|put the ban on lifting a merged functional method back'
  # THE LIST COMPREHENSION, five axes. The bracket pair cannot be written in a
  # row -- `IFS` splits on the bar it is made of -- so it is a named const, and
  # `List.fss` is a mutation target because `include_str!` puts it in the
  # dependency graph.
  'crates/types/src/comprehension.rs|let Some(kind) = kind_for(bracket, mapping) else {|let Some(kind) = KINDS.first() else {|lower EVERY comprehension bracket as a list, so an array comprehension silently builds one'
  'crates/types/src/comprehension.rs|            (true, Some(slot)) => slot,|            (true, Some(_slot)) => return Err(TypeError::ComprehensionElementUnwritten { span }),|stop taking the element type from the slot it initialises'
  'crates/types/src/comprehension.rs|                *flag = true;|                *flag = false;|lower a comprehension without minting the collection it names'
  'crates/types/src/comprehension.rs|            infix_le(var(&counter, span), hi, span)|            infix_lt(var(&counter, span), hi, span)|read an inclusive range as exclusive'
  'crates/types/src/List.fss|if count >= length(store) then reserve() end|if false then reserve() end|stop the minted List growing its storage'
  # ARITY FLATTENING, four axes. `overloading.tex:125` makes `f(x:(A,B))` and
  # `f(a:A,b:B)` ONE declaration, so the two halves are: flatten the parameter
  # list, and spread the argument -- and spread it ONLY where a declaration of
  # that arity exists, or `println(t)` reports an arity nobody wrote.
  'crates/types/src/tuple.rs|self.flatten_params(&mut f.params)?;|let _unflattened = &f.params;|stop flattening a function parameter list'
  'crates/types/src/tuple.rs|                            spread = true;|                            spread = false;|stop spreading a whole tuple in argument position'
  'crates/types/src/tuple.rs|        seen.contains(&arity)|        true|spread a tuple across a callee that has no declaration of that arity'
  'crates/types/src/tuple.rs|            Some(TypeRef::Tuple { elems, .. }) => Some(elems.clone()),|            Some(TypeRef::Tuple { elems: _, .. }) => None,|stop splitting a binding written with a tuple type'
  # A BINDING CONDITION, three axes. `DelimitedExpr.rats:37,39,40,216` makes the
  # condition a GeneratorClause, so the decision needs lookahead for a `<-`
  # before the closing keyword; `then` is optional AND may sit on the next line;
  # and the two keywords differ in nothing but whether the body repeats.
  'crates/parser/src/lib.rs|if let Some(binders) = self.binding_condition_here(&Kind::KwDo) {|if let Some(binders) = Option::<Vec<String>>::None {|refuse a `while` binding condition at the parser again'
  'crates/parser/src/lib.rs|        if self.at(&Kind::KwThen) {|        if false {|stop taking a `then` that sits on the next line'
  'crates/parser/src/lib.rs|                loops: true,|                loops: false,|read a `while` binding condition as an `if`'
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
            runs_and_prints; evaluated_once; implicit_builtin_import
            throw_halts; collision_matrix; refusals
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
        implicit_builtin_import
        throw_halts
        collision_matrix
        refusals
        compile_metric
        ;;
esac

printf '\n%d/%d\n' "$passed" "$failed"
[[ $failed -eq 0 ]]

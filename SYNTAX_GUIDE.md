# Fortress Syntax Guide

Every piece of syntax that appears in a `.fss` file in this repository, arranged as a
"Learn X in Y minutes" guide: read it top to bottom for the tour, or jump to a section
and copy a block.

It was built by grepping the corpus, not by reading the specification. The corpus is
**1789 `.fss` files, 62,720 code lines**: the Sun/Oracle Project Fortress sources this
repository forked, plus the 61 test programs the Rust rewrite in `fortressc/` is built
against. Every construct below carries a `path:line` citation you can go read. If
something is not here, it is not in the corpus.

Frequency numbers ("N uses in M files") are word-boundary counts over all 1789 committed
files with comments and string literals removed, so a keyword that only appears in prose is
not counted. The stripper follows the compiler's own lexer rules
(`fortressc/crates/lexer/src/raw.rs:51-92`): `(* *)` nests, `(*)` runs to end of line at top
level and is inert inside a block comment. That last rule matters more than it sounds. Get it
wrong and a block comment runs away and swallows the rest of the file.

## Two languages in one tree

Fortress 1.0 is a big language. The Rust/LLVM compiler in `fortressc/` implements a
subset of it that grows milestone by milestone, so constructs are tagged:

| tag | meaning |
|---|---|
| `[fortressc]` | compiles and runs with the current compiler |
| `[parses]` | lexes and parses, then the type checker refuses it as out of subset |
| `[legacy]` | real Fortress syntax, present in the corpus, not accepted by the rewrite yet |

Tags are applied per construct, not per line, and a section that is legacy end to end
says so in its first sentence instead of tagging every block.

The tags were checked against master at `cd2458cc0` (2026-08-19): several hundred throwaway
programs were compiled and run, and where one contradicted a tag the tag lost.

> ## ⚠ THE TAGS ARE STALE. READ THIS BEFORE TRUSTING ONE.
>
> **Audited 2026-08-23 against `faef66205`, 188 commits after the pin.** The corpus half of
> this guide — the syntax shapes, the frequency counts, the `path:line` citations — is
> still good. **The compiler half is not.** Roughly thirty-five constructs tagged
> `[parses]` or `[legacy]` now COMPILE AND RUN, and a further dozen are still refused but
> with different wording than the message quoted here.
>
> The audit SAMPLED, at about 120 probe programs. Every finding it made has been corrected
> below and marked `⚠ 2026-08-23:`. **What it did not reach has not been re-derived**, so an
> untouched `[parses]` or `[legacy]` tag on this page means "was true on 2026-08-19", not
> "is true". Treat one as a question, not an answer, and settle it the way this guide was
> written in the first place: write the four-line program and run it.
>
> Landed since the pin, in bulk: `Self` as a type variable, getters as readable accessors,
> mutable object fields and field assignment, `Any`/`Object` as real types, `opr`
> declarations, character literals and radix numerals, most Unicode, multi-dimensional and
> sized arrays with the matrix aggregate, arrow types and `fn`, tuple destructuring,
> `nat`/`int`/`bool` static parameters, compound assignment, `value`/`private`/`abstract`/
> `native`, `atomic`/`also`/`spawn`/`label`/`exit`, reduction variables, an enforced
> `where` clause, `end`-name validation, a real MODULE SYSTEM with `.fsi` apis, component
> level values whose initializers run, varargs, `throws`, and `||` concatenation.
>
> **Owed:** a full re-derivation of every tag and every quoted diagnostic against the
> current binary. It is tracked in `04-state.md`; until it is done this banner stands.

`fortressc/crates/lexer/src/token.rs` reserves **73** words (⚠ 2026-08-23: was 66 at the pin,
and the guide said 90); its own comment says twenty are acted on, and the audit found roughly
twenty acted on by the parser, so the "the rest give one identical message" claim below is
false for about a third of the list.

**Out of scope:** user-definable syntax. Fortress lets a program extend the grammar with
`grammar` and `syntax` declarations, but those live in `.fsi` api files and the word
`grammar` appears in exactly zero `.fss` files in this tree. The rewrite drops the feature
on purpose (see `README.md`), which is what keeps the frontend a plain lexer plus
recursive descent parser. `.fsi` files are not covered here at all.

⚠ 2026-08-23: `.fsi` files are no longer out of the COMPILER's scope even though they are
still out of this guide's. `fortressc Widget.fsi` checks an api -- "headers resolved and
bounds discharged" -- and 125 corpus apis do. User-definable syntax is still dropped.

## The whole language in one program

This compiles and runs today. `fortressc tour.fss -o tour && ./tour`.

```fortress
component tour              (* one component per file, named after the file *)
export Executable           (* Executable's contract is a top-level run() *)

trait Shape end             (* a trait is a type with no representation *)

object Circle(r: ZZ64) extends {Shape}   (* constructor params ARE the fields *)
   name: String = "circle"               (* a field with a default, no param *)
end

object Box(w: ZZ64, h: ZZ64) extends {Shape} end

area(s: Circle): ZZ64 = 3 s.r s.r   (* juxtaposition is multiplication: 3 * r * r *)
area(s: Box): ZZ64 = s.w s.h        (* overloads dispatch on run-time argument types *)

object Cell[\T\](held: T) end       (* [\ \] are the static (generic) argument brackets *)

run(): () = do                      (* () is the void type; do ... end is a block *)
   c: Circle = Circle(4)            (* `:` annotates, `=` binds immutably *)
   println(c.name)
   println(area(c))                 (* 48 *)
   println(area(Box(3, 5)))         (* 15 *)

   n: ZZ64 = 8
   a: Array[\ZZ64\] = array(n)      (* subscripts are ZZ64, so arrays pass 2^31 *)
   for i <- 0#n do                  (* `for` runs in PARALLEL by default *)
      a[i] := i i                   (* `:=` assigns; `i i` is i*i *)
   end

   j: ZZ64 := 0                     (* `:=` at a declaration makes it mutable *)
   while j < n do
      println(a[j])
      j := j + 1
   end

   b: Cell[\String\] = Cell[\String\]("boxed")   (* monomorphized, not boxed *)
   println(b.held)
   println(if n > 4 then "big" else "small" end) (* if is an expression *)
end
end
```

## At a glance

```fortress
x: ZZ64 = 5                              (* immutable binding, annotated *)
y: ZZ64 := 0                             (* mutable binding *)
y := y + 1                               (* assignment *)
f(a: ZZ64, b: ZZ64): ZZ64 = a + b        (* function, expression body *)
g(): () = do println("hi") end           (* function, block body *)
opr SMAX(x: T, y: T): T = ...            (* infix operator definition *)
trait Ordered[\T\] comprises T
  abstract opr <(self, other: T): Boolean   (* an abstract method, no body *)
end
object P(x: ZZ64) extends {Ordered[\P\]} end   (* object; constructor params are fields *)
h[\T\](v: T): T = v                      (* generic function, static params in [\ \] *)
if c then a else b end                   (* if is an expression *)
while c do ... end                       (* the only sequential loop keyword *)
for i <- 0#n do ... end                  (* for runs in PARALLEL by default *)
for i <- seq(0#n) do ... end             (* seq() opts out of the parallelism *)
atomic do ... end                        (* transaction *)
<|1, 2, 3|>                              (* list literal *)
{0, 1, 2}      { k |-> v }               (* set and map literals *)
<|[\RR64\] random(n) | i <- 0#n |>       (* comprehension *)
SUM[(k,a) <- c] a.size                   (* reduction *)
try ... catch e  SomeException => ... end    (* exceptions *)
(* comment, and they nest *)
```

## Contents

1. [Program structure, imports, and comments](#1-2-program-structure-imports-and-comments)
2. [Variables, bindings and literals](#3-variables-bindings-and-literals)
3. [Operators and expression syntax](#4-operators-and-expression-syntax)
4. [Functions](#5-functions)
5. [Objects](#6-objects)
6. [Traits](#7-traits)
7. [Generics and static parameters](#8-generics-and-static-parameters)
8. [Control flow](#9-control-flow)
9. [Loops, generators and comprehensions](#10-loops-generators-and-comprehensions)
10. [Parallelism and atomicity](#11-parallelism-and-atomicity)
11. [Exceptions](#12-exceptions)
12. [Types, tuples, dimensions and units](#13-types-tuples-dimensions-and-units)
13. [Contracts, tests and modifiers](#14-contracts-tests-and-modifiers)
14. [What the Rust rewrite compiles today](#15-what-the-rust-rewrite-compiles-today)

## 1-2. Program structure, imports, and comments

### Program structure

A file is one component: header, imports/exports, declarations, bare `end`. [fortressc]

```fortress
component t1              (* one component per file, named after the file *)
export Executable         (* the corpus convention; fortressc does NOT enforce it *)
run() = println("hi")     (* Executable's contract is a top-level run() *)
end                       (* a bare `end` at column 0 closes the file *)
```

*Seen in: fortressc/tests/arraysum.fss:1-4, Library/List.fss:501*

Imports and exports may come in either order, and `end` may repeat the name.

```fortress
component List
import OneShotFlag.{...}   (* 385 files put imports first ... *)
import NativeArray.{...}
export List                (* ... a library component exports the api of its own name *)
end
```

```fortress
component Shuffle
export Shuffle             (* ... and 33 files put the export first; both compile *)
import List.{...}
end Shuffle                (* `end` may name the component; 25 files, and unchecked *)
```

*Seen in: Library/List.fss:12-17, Library/Shuffle.fss:12-14, Library/Shuffle.fss:43*

fortressc stops at the `end` token and discards every token after it, so `end Shuffle` is accepted incidentally rather than recognised. A file ending `end QQQ ZZZ nonsense` typechecks clean.  ⚠ 2026-08-23: INVERTED. Nothing after the closing `end` is discarded any more, and the NAME is checked: on `component c`, `end QQQ ZZZ nonsense` answers `` `end QQQ` closes a declaration named `c` ``, and a stray `end` that opened nothing answers `expected end of file; this `end` closes nothing`. A matching `trait T end T` is accepted.

A component name may be dotted, from a dotted filename, from dotted directories, or from neither. 334 files. [fortressc]

```fortress
component Compiled0.b         (* dots in the FILENAME: Compiled0.b.fss - 328 of 334 *)
component a.b.c.d.hello       (* dots from DIRECTORIES: a.b/c.d/hello.fss - 5 files *)
component fortress.executable (* NEITHER: buffons.fss has no dot in its path at all - 1 file *)
end component XxXending.Name  (* the `end component <dotted name>` spelling; exactly 1 file *)
```

*Seen in: ProjectFortress/compiler_tests/Compiled0.b.fss:11, ProjectFortress/tests/a.b/c.d/hello.fss:12, Fortify/example/buffons.fss:12, ProjectFortress/parser_tests/XXXending.Name.fss:41*

A component header may itself carry a `comprises` clause, naming the sub-components a compound component is assembled from. 3 files, all parser tests. [legacy]

```fortress
component compoundComponent comprises { A, B, C } end component compoundComponent
                                  (* the whole file: header, clause and `end` on one line *)
component XXXCompoundComponent2 comprises A   (* one name needs no braces, exactly like a trait's *)
export Executable
run() = ()
end
```

*Seen in: ProjectFortress/parser_tests/compoundComponent.fss:12, ProjectFortress/parser_tests/XXXCompoundComponent2.fss:12-15*

fortressc stops at the clause: "expected a newline or `;`, found KwComprises".

The wrapper is optional. 375 of 1789 files are headerless: no `component`, no closing `end`. [fortressc]

```fortress
export Executable          (* file starts straight at the export *)
import QuickCheck.{...}
import List.{...}
```

*Seen in: ProjectFortress/tests/QuickCheckTest.fss:6-9, ProjectFortress/compiler_tests/CoercionsApi.fss:12-16*

Exports name apis. The bare identifier is the only form fortressc reads; the braced list is [legacy].

```fortress
export Executable                (* bare identifier, 1766 files *)
export { Executable }            (* braced list, 11 files *)     [fortressc]
export { FirstAPI , SecondAPI }  (* two apis on one line, 2 files *) [legacy]
(*) ⚠ 2026-08-23: the single-name braced form `export { Executable }` compiles.
```

*Seen in: fortressc/tests/skeleton.fss:2, ProjectFortress/compiler_tests/Compiled250.fss:13, ProjectFortress/compiler_tests/Compiled1.i.fss:13, ProjectFortress/parser_tests/AbsFieldTest.fss:13*

`run()` is the entry point. Whitespace around `:` and `=` is entirely free, which is why the corpus spells it ten ways.

```fortress
run() = println f(3.141592)   (* no return type: the spec examples' house style *)
run() = ()                    (* a do-nothing entry point, 104 files *)
run():()=do                   (* fully tight; `run(): () = do` and `run():() = do` also common *)
   needleLength = 20
end
```

*Seen in: SpecData/examples/basic/Expr.Do.f.fss:24, Fortify/example/buffons.fss:18-19*

`Executable` is never declared in any .fss file, it lives in the api layer. fortressc does not require the name `run`: a component without one compiles and links, and the binary does nothing. When `run` is present it must take no parameters, because the generated `main` calls it with none, and `run(x: ZZ32)` is refused with "`run` is the entry point and is called with no arguments, but this one declares 1".

The command line comes in as a varargs parameter in 4 files; everything else imports it. [legacy]

```fortress
run(args:String...):() = do      (* varargs entry point, tight *)
run(args : String...) : () = do  (* the same thing spaced out *)
import CompilerSystem.args       (* what most programs do instead: 15 files *) [fortressc]
```

*Seen in: ProjectFortress/demos/Words.fss:70, ProjectFortress/demos/BiCGSTAB.fss:75, ProjectFortress/compiler_tests/Compiled5.f.fss:12*

### Imports

The `.` before `{` is part of the import syntax, not a field access. fortressc parses the brace group as a balanced token run and DISCARDS it: there is no module system yet, so an import compiles and has no effect. [fortressc]  ⚠ 2026-08-23 (later): AND A COMPONENT GETS THE CORE APIS WITH NO WRITTEN IMPORT.
`Generator`, `Maybe`, `Number` and the rest of `FortressLibrary`/`CompilerBuiltin`
resolve in a `.fss` that imports nothing. TYPES only -- an api's functions and
values are obligations the component must satisfy, so `gcd(4, 6)` is still
`unknown name` -- and a merged declaration LOSES to a builtin of the same name,
so the library's own `trait String` does not shadow `Type::String`. An object
that came from an api can be named in a signature; CONSTRUCTING one is refused
unless this file names it and its layout is buildable, and never if it is a
singleton.

⚠ 2026-08-23: THERE IS A MODULE SYSTEM NOW. An import resolves apis off the source path -- the driver prints `resolved N api(s)` and names what it could not find -- and an imported TYPE is genuinely visible: with a `Shapes.fsi` declaring `trait Shape end`, `object Circle extends Shape end` compiles with the import and gives `unknown type `Shape`` without it. `.fsi` files are first-class and 125 corpus apis check. Only an api's TRAITS and OBJECTS merge; its function and value declarations are the importer's obligation, not its scope.

```fortress
import List.{...}                (* import every exported name - the dominant form *)
import CompilerAlgebra.{ ... }   (* spaced variant, 12 uses in 11 files; the tight one dominates *)
import GeneratorLibrary.{DefaultGeneratorImplementation, voidReduction}  (* selective *)
import CompilerSystem.args       (* single name, no braces *)
import FlatString.FlatString     (* a type pulled out of the api of the same name *)
```

*Seen in: Library/List.fss:13-15, Library/GeneratorLibrary.fss:13, ProjectFortress/BirdyLib/LPairs.fss:20, ProjectFortress/compiler_tests/Compiled5.f.fss:12, Library/File.fss:13*

Inside a selector, `=>` renames and `opr` names an operator. An enclosing operator is written as its two halves separated by a space.

```fortress
import Foo.{ f => g }                          (* rename on import *)
import CoercionsApi.{A => AA, B, C, D => DD}   (* aliased and plain names mixed *)
import List.{opr <| |>}                        (* the enclosing bracket pair `<| |>` *)
import Operators.{opr OP, opr |, opr | |}      (* bare `|` and the pair `| |` *)
import List.{Cons => CC, opr <| => ||}         (* operators alias too *)
```

*Seen in: ProjectFortress/compiler_tests/Compiled9.CompiledCoercions.fss:12, Library/FortressLibrary.fss:22, ProjectFortress/parser_tests/XXXPreparser.c.fss:15*

`except` subtracts from what was imported. 8 lines in the whole corpus, 3 of them in real Library files. [fortressc]

```fortress
import Map.{...} except { opr BIG UNION }      (* exclude one operator *)
import Map.{...} except { opr BIG UNION, opr BIG INTERSECTION, opr BIG SYMDIFF }
import FortressBuiltin.{...} except Boolean    (* one name needs no braces ... *)
import AbstractDef.{...} except {ShellTrait}   (* ... but may have them *)
```

*Seen in: Library/Relation.fss:14, Library/PrefixSet.fss:35, SpecData/examples/basic/StatParam.Bool.fss:16*

Two shapes worth recognising: importing an api as a unit, which is a one-off, and packing imports onto one line with `;`, which the BirdCount demos do in 7 files.

```fortress
import api Collection      (* the ONLY occurrence of the `api` keyword in any .fss file *)
```

```fortress
import File.{...}; import FileSupport.{...}; import FlatString.{...}; import List.{...}
import Map.{...}; import Pairs.{...}; import Set.{...}; import System.{getProperty}
```

*Seen in: Library/incomplete/Sequence.fss:12-14, ProjectFortress/demos/BirdCount1y.fss:12-13*

`;` is the general statement separator, so the second is not import-specific syntax; the 7 files that write imports this way are all ProjectFortress/demos/BirdCount*.fss.

One more form puts the names first and the component last, joined by `from`. 2 files, both unit libraries, and the `from` may sit on its own continuation line. [legacy]

```fortress
import { Length, Area, Volume, Time, Mass, millimeters, liters, grams }
       from Fortress.SIUnits   (* names first, then `from`, then the component; no `.{ }` anywhere *)
import { Length, Volume, Mass, Time, Force, Energy, Power, Temperature, Angle,
         millimeters, kilogram, liter } from Fortress.SIunits    (* or all on one line *)
```

*Seen in: Library/incomplete/basic/Fortress.EnglishUnits.fss:13-14, Library/incomplete/basic/Fortress.Potrzebie.fss:12-13*

fortressc swallows the brace group and then stops at the `from`: "expected a newline or `;`, found Ident("from")" when it is on the same line, "expected `(`, found Ident("Fortress")" when it is on the next.

A component whose bodies live in the JVM is `native`, and pulls in Java statics with `import java`. All of this is [legacy] by construction: it is the boundary the rewrite exists to replace, and fortressc refuses it at the modifier: "reserved word `native` is not in the implemented subset".  ⚠ 2026-08-23: `native component c ... end` COMPILES now; the modifier is recorded and not read. `import java ...` is a separate question and was not re-probed.

```fortress
native component File                                     (* 9 files in the whole corpus *)
import FlatString.FlatString
export File

private language="java"                                   (* the foreign runtime ... *)
private package="com.sun.fortress.interpreter.glue.prim"  (* ... and its package *)
```

```fortress
import java com.sun.fortress.nativeHelpers.{systemOps.getArgs => jGetArgs}  (* a JVM static, aliased in *)
import java java.lang.{Math.random => jrandom}   (* straight out of the JDK; language name is unquoted *)
import "java" java.util.{Map.Entry}              (* the quoted-language variant, 1 file *)
```

*Seen in: Library/File.fss:12-18, Library/CompilerSystem.fss:13, ProjectFortress/parser_tests/foreignLang.fss:12-15*

### Comments

One general form, and it NESTS. [fortressc]

```fortress
(* a block comment, any number of lines *)
(* outer (* inner *) still outer *)   (* the inner close does not end the outer block *)
randomR(*[\U\]*)(range:FullScalarRange[\ZZ32(*U*)\]): ZZ32(*U*) =
                    (* inline mid-signature, disabling static parameters without deleting them *)
```

*Seen in: ProjectFortress/compiler_tests/Compiled6.bz.fss:21-23, Library/Random.fss:55*

`(*)` runs to end of line and needs no closing delimiter. 1274 uses across 179 files, a real but minority style. [fortressc]

```fortress
(*) A comment-to-end-of-line
f(w: ZZ32) = w+1     (*) Local function declaration
y = x+1              (*) Local variable declaration (immutable)
var z: RR64 = 0      (*) Local variable declaration (mutable)
(*)rotate(x:Node(Tree, _, _)): Tree = x (*) never called
```

*Seen in: Fortify/example/buffons.fss:16, Documentation/Specification/Code/Block1.fss:22-25, ProjectFortress/compiler_tests/patternMatching1.fss:47*

Only the `(*)` markers are the point there: the declarations are Block1.fss's own, and inside a `do` fortressc takes `y = x+1` but refuses the local function declaration, "expected `)`, found Colon", and `var`, "expected an expression, found KwVar".

`//` is NOT a comment in Fortress, it is an operator (`r := r // "["`), and `#` is the range operator (`1#3000`). Neither ever introduces a comment.

`(**` opens a doc comment for the declaration below it. There is no attachment rule in the syntax; it is lexically an ordinary block comment and the doc tool (Fortify/) is what gives it meaning. [fortressc]

```fortress
(** minimum and maximum refer to the key **)
minimum():Maybe[\(Key,Val)\]

(** left subtree, value (if any) at kk, right subtree, original depth *)
                (* opened `(**` and closed with a single star: 219 of the 1417 doc comments, and it lexes fine *)
```

*Seen in: Library/Map.fss:75-76, Library/Avl.fss:25*

Two conventions live inside doc comments. `%…%` marks up code for Fortify, and all-caps markers let the specification build slice a file.

```fortress
(** %write(FlatString)% and %write(Char)% are the primitive mechanisms
    for writing characters to  a %WriteStream%. **)
```

```fortress
(** EXAMPLE **)          (* SpecData examples put the interesting lines between these *)
f(x:RR64) = do
  (sin(x) + 1)^2
end
(** END EXAMPLE **)      (* the component and run() around it are scaffolding *)
```

*Seen in: Library/Stream.fss:35-36, Library/CaseInsensitiveString.fss:15-16, SpecData/examples/basic/Expr.Do.f.fss:18-22*

Both marker pairs are ordinary comments and compile away; the only thing fortressc refuses in that example is `sin`, which it has no name for yet: "unknown name `sin`".

1546 of the 1789 files open with a `(****` copyright banner, and in SpecData that banner is itself wrapped in `(** COPYRIGHT **)` / `(** END COPYRIGHT **)` so the spec build can drop it.

## 3. Variables, bindings and literals

### Bindings

`=` binds, and what it binds is immutable. There is no `let`, `const`, `val` or `final`; immutability is the default and needs no keyword. [fortressc]

```fortress
pi = 3.141592653589793238462643383279502884197169399375108209749445923078  (* type inferred *)
pi: RR64 = 3.141592653589793238462643383279502884197169399375108209749445923078
sz:ZZ32 = 1 + left + right          (* whitespace around the colon is free *)
sz:ZZ32 = 1 + |left| + |right|      (* Map.fss's own line; the enclosing bars are [legacy] -
                                       fortressc: "expected an expression, found Bar" *)
up : ZZ32 = if upper <= 0 then 1024 else upper end   (* initializer is any expression *)
i = Cell[\ZZ32\](7)
```

*Seen in: SpecData/examples/basic/Var.Top.b.fss:19, Library/Map.fss:274, Library/FileSupport.fss:151*

`:=` in a declaration position is what makes a name assignable. This is the only mutable form the Rust compiler accepts; a bare `x := 5` on an undeclared name is refused with "`x` is not declared; write `x:T := ...` to declare it". [fortressc]

```fortress
i:ZZ64 := 0             (* declares i mutable; the := is what allocates the slot *)
total: ZZ64 := 0
n0 : ZZ := numerator(a) (* the spaced form compiles; the TYPE `ZZ` is [legacy] -
                           fortressc: "unknown type `ZZ`" *)
```

*Seen in: fortressc/tests/arraysum.fss:8, Library/FortressLibrary.fss:472, fortressc/tests/badparallelescape.fss:9*

Every `:=` binding gets an alloca hoisted to the function's entry block, so a mutable declared inside a loop body costs one stack slot per iteration.

`var` is the explicit mutable keyword, 505 uses in 193 files. It takes either initializer token with the same meaning, `=` outnumbering `:=` about 5 to 3, and may be left uninitialized. [parses]

```fortress
var a: ZZ32 = 1               (* the dominant spelling, 232 of the 505 uses *)
var current:String := string  (* := instead, same meaning, 145 uses *)
var s:String                  (* no initializer at all *)
var mutable:ZZ32 := 3         (* a var field inside an object body *)
```

*Seen in: SpecData/examples/basic/Expr.Assign.b.fss:22, Library/Format.fss:186, ProjectFortress/compiler_tests/Compiled6.ax.fss:18*

fortressc's parser acts on `var` only inside a trait or object body, and only with `=` or with no initializer at all: `var x:T := e` is a parse error everywhere, "expected a newline or `;`, found ColonEq". In an object body the checker then refuses what parses ("`var mutable`: mutable fields are not implemented"); in a trait body the same declaration compiles clean. In a `do` block it is "expected an expression, found KwVar" and in an object parameter list "expected a parameter name, found KwVar". Everything `var` does at statement level is covered by `x: T := e`.  ⚠ 2026-08-23: ALL THREE OF THOSE ARE STALE. A `var` field in an object body is real storage and assignable. `var x: T := e` PARSES -- `InitVal = ("=" / ":=")` at `Variable.rats:37` -- and so does a bare `x: T := e`, which declares the field MUTABLE by the same rule `value_decl` already used at component level. `object O(var v: ZZ32)` parses too, because an object's value parameters ARE its fields; a FUNCTION's parameter list still refuses `var` at `identifier`, since a parameter is not storage. What is still refused by name is a LOCAL `var x: T` with no initializer, which needs definite-assignment analysis.

A name may be declared with only its type and initialized on a later line. [legacy]

```fortress
pi: RR64          (* type on one line ... *)
pi = 3.141592653589793238462643383279502884197169399375108209749445923078
(* ... value on the next, and the = keeps the binding immutable *)
var (player1, player2): String...   (* uninitialized, and a tuple with a rest type *)
```

*Seen in: SpecData/examples/basic/Var.Local.fss:20-21, ProjectFortress/demos/tennisRanking.fss:250*

A binding written straight in the component body, outside any function, object or trait, needs no keyword and no indentation. [parses]

```fortress
pi : FloatLiteral = 3.141592653589793 (* Double whose sin is closest to 0 *)
infinity : RR64 = 1.0 / 0.0
language:String = "java"
var tmp:ZZ32 = 0                      (* a MUTABLE component-level value; fortressc does not even
                                         parse this one: "expected a function name, found KwVar" *)
```

*Seen in: Library/Constants.fss:15-17, Library/Reader.fss:17, ProjectFortress/tests/ObjectFieldShadowing.fss:15*

fortressc parses the non-`var` lines and then refuses: "`language`: a component-level value declaration is parsed but not implemented; its initializer would have to run at component initialization, and it is not a nullary function". The `FloatLiteral` line stops one step earlier, on "unknown type `FloatLiteral`".  ⚠ 2026-08-23: COMPONENT-LEVEL VALUES WORK AND THEIR INITIALIZERS RUN, in declaration order, inside `main` after the runtime is up and before `run`. Component-level `var` works too. `fortressc/tests/badvaluebinding.fss` is the gate fixture that asserts the ORDER.

### Assignment

The same `:=` token reassigns, distinguished only by there being no annotation and the name already being in scope. [fortressc]

```fortress
i := i + 1        (* no annotation, so this reassigns rather than declares *)
x := f(0)
squares[i] := i i (* store through a subscript; every index is bounds checked *)
lkeys[0 # lsize] := self.keys[0 # lsize]   (* [legacy] the target may be a SLICE, not one index;
                                              fortressc: "expected `]`, found Hash" *)
```

*Seen in: fortressc/tests/arraysum.fss:11, SpecData/examples/basic/Expr.Assign.b.fss:36, Library/SkipList.fss:288*

Field assignment is the same syntax, and may route to a declared `setter`. 12 uses in 10 files. [parses]

```fortress
o.foo := "hi"     (* assign to a mutable field of another object *)
player.fld := 5   (* same syntax when a setter is what actually runs *)
```

*Seen in: ProjectFortress/compiler_tests/Compiled6.ar.fss:24, ProjectFortress/tests/setterTest.fss:39*

Measured: `b.n := 5` parses under fortressc and the checker refuses it, "only a variable or an array element can be assigned to"; declaring the field `var` instead gives "`var n`: mutable fields are not implemented".  ⚠ 2026-08-23: BOTH INVERTED. Declaring the field `var` and assigning to it works; an immutable field answers `` field `n` is immutable; declare it `var n: T = ...` to assign to it ``.

A parenthesised list of targets on the left of `:=` assigns several at once. The right side is fully evaluated first, which is how a swap is written; there is no `:=:` operator. 54 uses in 19 files. [legacy]

```fortress
(a,b,c) := (b,c,a)                  (* right side evaluated first, so this rotates *)
(A[k,j],A[m,j]) := (A[m,j],A[k,j])  (* targets may be subscripts and fields, not just names *)
```

*Seen in: SpecData/examples/basic/Expr.Assign.b.fss:38, ProjectFortress/demos/lutx.fss:74*

`x op= e` updates in place. The rule is the whole operator table, not a fixed list: any infix operator, symbolic or named, glues to `=`. [legacy]

```fortress
x += 1
(x,y) += (delta_x,delta_y)
(a[i],b.x,c) += f(t,u,v)  (* targets can be a subscript, a field and a name at once *)
result[i] MIN= y          (* a NAMED operator glued to =, with no space before it *)
x BITAND= x-1
A[j,k] /= A[k,k]          (* divide and store; =/= is inequality, NOT a compound assign *)
```

*Seen in: SpecData/examples/basic/Expr.Assign.a.fss:30, Library/CompilerLibrary.fss:475, ProjectFortress/demos/lutx.fss:79*

Counts: `+=` 243 uses in 117 files, `||=` 37, `-=` 17, `UNIONCAT=` 10, `UPLUS=` 10, `TIMES=` 7, `MIN=` 2, `MAX=` 2, `MAXMIN=` 2, and `BITAND=`, `CUP=`, `/=`, `//=` once each. fortressc: "expected an expression, found Eq". There are no `++` or `--` operators.  ⚠ 2026-08-23: COMPOUND ASSIGNMENT WORKS: `x: ZZ32 := 1` then `x += 1` prints 2. Of the other spellings only `+=` was probed; `TIMES=` is measured at zero corpus files and refused by name.

### Tuples, patterns and shadowing

Five spellings of a tuple binding; the untyped one is what Library actually uses. The whole family is [legacy] - fortressc refuses tuple values entirely, and the annotated forms give "expected a newline or `;`, found Colon".

```fortress
(x, y, z): (ZZ64, ZZ64, ZZ64) = (0, 1, 2)  (* one annotation over the whole left side *)
(x: ZZ64, y: ZZ64, z: ZZ64) = (0, 1, 2)    (* each binder annotated inside the parens *)
(x, y, z): ZZ64... = (0, 1, 2)  (* one type, ... applies it to every element (5 uses, 3 files) *)
var (x, y): ZZ64... = (5, 6)
(old,eval) = attempt()                     (* no annotation at all: the ordinary form *)
```

*Seen in: SpecData/examples/basic/Var.Top.d.fss:19, SpecData/examples/basic/Var.Top.f.fss:19, Library/Lazy.fss:22*

The `...` here is a type-level repeat on a binding, not a varargs parameter.

`_` binds a value you are throwing away and is the one name that may be rebound in the same scope. [fortressc]

```fortress
_ = println "top-level variable test"  (* _ works at component level too; [parses] there -
                                          fortressc refuses every component-level value *)
_ = println "first _"
_ = println "second _"    (* the same name bound twice in one block, which only _ may do *)
_ = x := 8                (* := used as an expression; [legacy] - fortressc:
                             "expected a newline or `;`, found ColonEq" *)
second[\T1,T2,T3\](x:(T1,T2,T3)): T2 = do (_,b,_) = x; b end
```

*Seen in: ProjectFortress/tests/Wildcards.fss:33-35, ProjectFortress/tests/Wildcards.fss:16, ProjectFortress/BirdyLib/Tuple.fss:22*

In fortressc `_` is just an ordinary identifier (`_ = 5` compiles), so the reuse rule is not enforced there, and `_ = println(...)` is refused because `()` has no value to store, not because of the underscore. The `second` declaration parses and typechecks only while nothing instantiates it; `second[\ZZ64,ZZ64,ZZ64\]((1,2,3))` gives "a tuple type is not implemented in this subset". At component level the underscore fares no better than any other name: `_ = 5` there gives "`_`: a component-level value declaration is parsed but not implemented".  ⚠ 2026-08-23: `_ = 5` at component level compiles now.

The annotation position may hold a constructor pattern that both checks the type and binds the fields. 7 files in the whole corpus. [legacy]

```fortress
tree1: Tree(k) = Leaf(1)
tree2: Node(l,i,r) = Node(tree1, 2, tree1)
tree3: Node(l':Tree, i': ZZ32, r', d = depth) = Node(tree2, 3, tree1)
(* d = depth binds d to the field or getter named depth; fields may be typed individually *)
tuple_tree:(Leaf(i_t'), Node(l_t, i_t, r_t)) = (tree1, tree2)  (* a tuple of patterns *)
p : T(q, r, s=c) = T1(2, "Test")                               (* the same at component level *)
```

*Seen in: ProjectFortress/compiler_tests/patternMatching1.fss:53-55, ProjectFortress/compiler_tests/patternMatching1.fss:58, ProjectFortress/parser_tests/patternMatching6.fss:31*

Primed names (`l'`, `i''`) are ordinary identifiers, not pattern syntax.

An inner binding of a name already in scope is a new, separate binding, and this is the trap the language ships an expected-failure fixture for.

```fortress
var x:ZZ32 = 0
if true then
  x = 1 (* Should fail here, immutable binding of shadowed mutable *)
(* = SHADOWS the outer mutable with a fresh immutable; := is what assigns to it *)

var tmp:ZZ32 = 0

object Obj()
    var tmp:ZZ32 = 0   (* an object field may shadow a component-level var *)
    x():ZZ32 = tmp
```

```fortress
f(x: ZZ64): ZZ64 = x + 1

apply(f: ZZ64, y: ZZ64): ZZ64 = f y  (* a parameter shadowing a top-level function *)
```

*Seen in: ProjectFortress/tests/XXXimmutable0.fss:17-19, ProjectFortress/tests/ObjectFieldShadowing.fss:15-19, fortressc/tests/juxtshadow.fss:4-6*

fortressc allows a nested `do` block to rebind an outer name, and the parameter shadowing above compiles. [fortressc]

### Literals

An integer is a run of decimal digits. There is no sign (a negative value is unary minus applied to the literal) and no size or type suffix; an unadorned numeral is `IntLiteral` until an annotation constrains it. [fortressc]

```fortress
n:ZZ64 = 100           (* a plain run of digits; IntLiteral until the annotation fixes it *)
var a: ZZ32 = 1        (* the corpus spelling of the same thing; `var` is [parses], see Bindings *)
bits: ZZ32 = 1100_16   (* a numeral takes its type from the annotation, but the RADIX suffix is
                          [legacy] - fortressc: "radix numerals are not in the M1 subset" *)
```

*Seen in: fortressc/tests/arraysum.fss:5, SpecData/examples/basic/Expr.Assign.b.fss:22, ProjectFortress/library_tests/Integer1.fss:23*

Digits may be grouped by an apostrophe or by U+202F. Both are pure formatting and both compile under fortressc: `assert(123'456'789, 123456789)` and the U+202F float below are measured, exit 0. The two `_16` lines are stopped by their RADIX suffix, not by the grouping. Apostrophes appear in 6 files; the U+202F form only in NumeralTest.fss lines 46 and 47, 14 gaps in all. [fortressc]

```fortress
assert(123'456'789, 123456789)         (* grouping changes nothing about the value *)
ZZ64_MIN: ZZ64 = 8000'0000'0000'0000_16
characterMinSupplementaryCodePoint: ZZ32 = 1'0000_16  (* groups need not be equal sized *)
assert(3.14159265358979, 3.141 592 653 589 79)
(* every gap on the line above is e2 80 af, U+202F NARROW NO-BREAK SPACE, hexdumped.
   An ordinary ASCII 0x20 there would be four juxtaposed terms, not one numeral. *)
```

*Seen in: ProjectFortress/tests/NumeralTest.fss:43, ProjectFortress/tests/NumeralTest.fss:47, Library/CompilerLibrary.fss:521*

Radix is a suffix, never a prefix. There is no `0x`, `0b` or `0o` form; `wrong2 = 0x6A35` sits in NumeralTest's "Rightly rejected" block. 13 files, and `_16` is 152 of the 175 uses. [legacy]

```fortress
NN32_MAX: NN32 = FFFF'FFFF_16              (* letter digits are UPPERCASE, case must not mix *)
assert(10101101_2, strToInt("10101101", 2))
assert(XE_12, strToInt("XE", 12))          (* bases 2 through 16 all appear *)
assert(0fff_SIXTEEN, strToInt("fff", 16))  (* the base may be spelled out in capitals *)
assert(DEAD.BEEF_16, DEAD.BEEF_SIXTEEN)    (* a radix POINT is allowed in a non-decimal base *)
```

*Seen in: Library/CompilerLibrary.fss:524, ProjectFortress/tests/NumeralTest.fss:21-22, ProjectFortress/tests/NumeralTest.fss:25*

fortressc's lexer has a dedicated refusal for a DIGIT-leading radix numeral: `1100_16` gives "radix numerals are not in the M1 subset" (fortressc/crates/lexer/src/error.rs:63). A letter-leading one never reaches that rule; `FF_16` lexes as an identifier and gives "unknown name `FF_16`".  ⚠ 2026-08-23: DIGIT-LEADING RADIX NUMERALS COMPILE: `println(1100_16)` prints 4352, using the SPECIFICATION'S digit values (`X` is ten and `E` is eleven at radix twelve). The letter-leading half is still correct -- `FF_16` is still `unknown name`, and that is a NAMED DEVIATION rather than an omission.

Floats are digits, a point, digits. No e-notation exists anywhere in the corpus and there is no `f`/`d` suffix. [fortressc]

```fortress
xr: RR64 = 0.25
halve(x: RR64): RR64 = x/2
infinity : RR64 = 1.0 / 0.0   (* no INF literal; big magnitudes are a division *)
assert(DEAD.BEEF_16 + 1, 57006.745834350586)  (* or just a long digit run; the LEFT operand's
                                                 radix suffix is what makes this line [legacy] *)
```

*Seen in: fortressc/tests/exponent.fss:17, fortressc/tests/rr64literal.fss:4-6, Library/Constants.fss:17*

Strings are double-quoted with backslash escapes. There is no triple-quote form and no backslash-newline continuation; adjacent values concatenate by juxtaposition, `println("x = " x)`. [fortressc]

```fortress
y: String = "init"
println(x:Any...):() = writes(x,"\n")
"graph [concentrate=true,fontsize=14,label=\"IntMap\",style=bold];"  (* escaped quote *)
(* measured: "hi\nthere\ttab \"q\" back\\slash" compiles and prints exactly that *)
```

*Seen in: ProjectFortress/compiler_tests/Compiled1.an.fss:16, Library/File.fss:90, Library/IntMap.fss:33*

Characters are single-quoted, exactly one, with the same escapes. 57 files. [legacy]

```fortress
var pad:Char := ' '
encodeACGT(c: Char): ZZ32 = case c of 'A' => 0; 'C' => 1; 'G' => 2; 'T' => 3; end
assert(isDefined('''),true, "Failure of isDefined '''")  (* ''' is the apostrophe itself *)
```

*Seen in: Library/Format.fss:193, ProjectFortress/demos/BirdCount1u.fss:46, ProjectFortress/tests/CharacterTest.fss:191*

The apostrophe is overloaded three ways - character quotes, digit-group separator, and primed identifiers like `t'` or `i'''` - and only position separates them. fortressc refuses at parse time: "character literals are not in the M1 subset".  ⚠ 2026-08-23: CHARACTER LITERALS COMPILE. `ch: Char = 'a'` prints `a`. `Char` is ORDERED and NOT NUMERIC: `'a' + 'b'` is refused by name, `` `+` is not defined on Char; a character is ordered, not numeric ``.

Both quote characters come in more than one pair, and the closing mark must match the opening one. [legacy]

```fortress
println 'c'                (* the ordinary pair *)
println `h'                (* BACKTICK opens, apostrophe closes; the corpus holds exactly two
                              backticks, this line and its expected-fail twin *)
println ‘r’                (* U+2018 / U+2019 *)
println "Hello, World!"
println “Hello, World!”    (* U+201C / U+201D *)
```

*Seen in: ProjectFortress/tests/matchingCharacterMarks.fss:16-18, ProjectFortress/tests/matchingStringMarks.fss:16-17*

Mismatching them is an error the corpus tests for on purpose (ProjectFortress/parser_tests/XXXNotMatchingCharacterMarks.fss:16, ProjectFortress/parser_tests/XXXNotMatchingStringMarks.fss:16-17). fortressc knows the curly string well enough to name it, "curly-quote string delimiters are not in the M1 subset; use `"`", answers the backtick with "unrecognized character", and treats the U+2018/U+2019 pair as a character literal: "character literals are not in the M1 subset".  ⚠ 2026-08-23: the curly-quote STRING compiles now -- `println(“Hello”)` prints `Hello` -- and the curly CHAR is refused with a different message: `a character literal holds one character, an escape, four or more hex digits, or TAB, NEWLINE or RETURN`.

`true` and `false` are reserved words and genuine literals, not library values. [fortressc]

```fortress
consumed : Boolean := false
getter isEmpty(): Boolean = true  (* the declaration compiles; READING the getter is refused,
                                     "accessors parse but are not implemented" *)
var in_string:Boolean := false    (* [legacy] statement-level `var`; fortressc:
                                     "expected an expression, found KwVar" *)
var escape:Boolean := false
```

*Seen in: Library/FileSupport.fss:150, Library/Heap.fss:126, Library/Format.fss:369-370*

`()` is both the type with one value and that value.

```fortress
greet(): () = println("hello from a void function")
run(): () = greet()   (* return type and body: [fortressc] *)
run():() = ()
x: () = ()   (* [parses] the checker refuses: "() has no value, so it cannot be stored" *)
```

*Seen in: fortressc/tests/unitvoid.fss:4-6, Library/Avl.fss:426, ProjectFortress/compiler_tests/Compiled1.an.fss:15*

Collection literals are all [legacy]. A static-argument bracket right after the opening delimiter supplies the element type, and is what makes the empty collection writable.

```fortress
testList = <| 5, 10, 15, 20 |>            (* 837 <| against 14 Unicode uses *)
f:List[\ZZ\] = <|[\ZZ\] 2, 3, 4, 5 |>
<|[\E\] |>                                (* the empty list *)
words = ⟨"Hello ", "world, ", "it's ", "a ", "bright ", "new ", "day."⟩  (* U+27E8/U+27E9 *)
```

*Seen in: Library/incomplete/SkipTreeTest.fss:19, Library/PrefixSet.fss:129, ProjectFortress/tests/StringTests.fss:20*

```fortress
s = { 1, 2, 3 }               (* needs import Set.{...} *)
3 IN {0, 1, 2, 3, 4, 5}
{[\ZZ32\] 0,1,2}              (* element type up front; {[\T\]} alone is the empty set *)
```

*Seen in: ProjectFortress/not_passing_yet/XXXtestTuple.fss:17, SpecData/examples/basic/Expr.Set.fss:21, Library/Relation.fss:213*

```fortress
m = { "a" |-> 0, "b" |-> 1, "c" |-> 2 }   (* needs import Map.{...}; |-> 234 uses in 44 files *)
underlying = {[\ZZ32,Set[\ZZ32\]\] 0 |-> zzs(0), 1 |-> zzs(-1), 2 |-> zzs(-2) }
```

*Seen in: SpecData/examples/basic/Expr.Map.fss:22, Library/Relation.fss:191*

### Type names in annotations

`ZZ` is the unbounded integer, `NN` natural, `QQ` rational, `RR` real, with the bit width as a suffix where there is one. Bare `NN` never appears, only `NN32`/`NN64`; bare `RR` appears 14 times in 7 files, and 13 of those are the dimensioned `RR^3` or `RR^(2 BY 2)` rather than `RR` standing alone.

```fortress
xr: RR64 = 0.25
xz: ZZ64 = 4
n0 : ZZ := numerator(a)   (* the unbounded integer *)
x:Any = 5                 (* Any, the top of the hierarchy *)
f:Char = 'f'
```

*Seen in: fortressc/tests/exponent.fss:17-20, Library/FortressLibrary.fss:472, ProjectFortress/compiler_tests/Compiled5.ag.fss:17*

By annotation count: ZZ32 4843 in 713 files, String 3414/476, Boolean 1357/245, RR64 943/133, ZZ64 472/80, Any 337/67, ZZ 207/22, NN32 177/16, NN64 173/9, Char 178/40, Number 136/22, Object 95/38, IntLiteral 78/11, RR32 66/8, Character 46/5, QQ 38/2, RR 14/7, FloatLiteral 4/2. fortressc knows exactly ZZ32, ZZ64, RR64, Boolean, String, `()` and `Array[\T\]`; every other name above is [legacy].

## 4. Operators and expression syntax

Fortress has almost no built-in operator table: `+`, `IN`, `<| |>` and `|self|` are all ordinary library declarations written with `opr` (2640 uses in 228 files). The current Rust compiler implements a core set of expression forms natively but refuses every `opr` declaration [legacy] with "reserved word `opr` is not in the implemented subset", so the whole declaration half of this section is legacy; the expression forms are tagged where the distinction is real.  ⚠ 2026-08-23: `opr` DECLARATIONS COMPILE NOW, so the whole declaration half of this section is live: infix (`opr SMAX(x,y)` then `2 SMAX 3`), enclosing (`opr <| x |>`), the size bars (`opr |self|` then `|V(7)|`) and `opr BIG STAR()` all work. Sampled limits: a PREFIX USE like `NEGATE 5` still fails, and a generic operator's call site `SMAX[\ZZ32\](1,2)` does not parse. Retag this section construct by construct before relying on a `[legacy]` here.

### Juxtaposition

```fortress
sin x                  (* two expressions side by side: function application, no parens *)
n (n+1) / 2            (* numeric operands: multiplication, no * token *)
x[1] y[2] - x[2] y[1]
println("The answers are " (p + q) " and " (p - q))  (* string operand: concatenation *)
double(x: ZZ64): ZZ64 = x 2
```

*Seen in: SpecData/examples/basic/Expr.OprApp.fss:36-41, SpecData/examples/preliminaries/Overview.Juxt.sin.fss:20, fortressc/tests/juxtapply.fss:4*

A run of loose juxtapositions does NOT associate left to right; it regroups so that function names capture their arguments. [legacy]

```fortress
u = n (n+1) sin 3 n x log log y             (* the legacy parser regroups this *)
w = (n (n+1)) (sin (3 n x)) (log (log y))   (* the test asserts u = w *)
```

fortressc does not regroup. A function name anywhere but the front of a juxtaposition does not resolve at all, so the `u` line stops at "unknown name `sin`", and even the tail `log log y` on its own is refused with "a juxtaposition of 3 elements led by a function is not implemented; parenthesise the application". Of the two, only the fully parenthesised `w` line compiles. [fortressc]

*Seen in: ProjectFortress/tests/juxtTwice.fss:33-34, ProjectFortress/not_working_static_tests/LooseJuxt.fss:17*

### Whitespace is part of precedence

Tight (no space) binds above loose (spaced), and a tight operator may not meet a loose one at the same level.

```fortress
a+b DOT c   (* Reject! tight + against loose DOT *)
a+b c       (* Reject! *)
a+b - c     (* Reject! *)
a - b-c     (* Reject! *)
a+b + c     (* Reject! *)
-a b        (* Reject! tight prefix minus in a loose juxtaposition *)
c = - a b   (* Accept! spacing is uniform *)
```

The rule is [legacy]: fortressc's parser takes all six Reject lines without complaint, and only the first then fails, in the checker, on the unknown name `DOT`, which is not a spacing objection. Obey the rule anyway, because under fortressc the spacing changes the ANSWER and not just the diagnostics. With `a = 7` and `b = 3`, `a + b` is 10 but `a +b` is 21: `+b` is unary plus, the two terms then juxtapose, and juxtaposition multiplies. `a+ b` is a hard refusal, "a postfix operator followed by a juxtaposition is not in the M1 subset". Space both sides of a binary operator or neither.

*Seen in: ProjectFortress/tests/precedence.fss:19-24, ProjectFortress/tests/precedence.fss:26*

### Arithmetic

```fortress
a + b    a - b    a / b    - a      (* prefix minus differs from infix only by arity *)
2^10                                (* ^ is tight against both operands *)
2^3^2                               (* ^ is LEFT associative: 64, not 512 *)
2 3^2                               (* tight ^ beats juxtaposition: 2 (3^2) *)
2 ^ 3 ^ 2                           (* spacing does not regroup it: still 64 *)
```

All [fortressc], including the halt-with-a-diagnostic on a negative integer exponent. The legacy parser wanted the loose form parenthesised, which is why `ProjectFortress/parser_tests/XXXTwoThreeTwoLoose.fss:16` writes `x = 2 ^ (3 ^ 2)`; fortressc does not need it, and that parenthesised form means 512 rather than 64. [legacy] `**` is not written anywhere in the corpus, and `*` itself is declared only three times, across two files, because juxtaposition covers multiplication.

*Seen in: fortressc/tests/exponent.fss:12-16, ProjectFortress/parser_tests/XXXTwoThreeTwoLoose.fss:16, fortressc/tests/negexponent.fss:10*

Integer operations with no ASCII symbol are ALL-CAPS infix words. [parses] in fortressc: they lex as ordinary operator names, then the checker reports "unknown name `DIV`".

```fortress
5 DIV 2          (* = 2 *)
3 + 5 DIV 4      (* = 4; DIV binds tighter than + *)
a MOD b   a REM b   a GCD b   a LCM b   a DIVIDES b   a CHOOSE b
```

*Seen in: ProjectFortress/tests/DivPrecedence.fss:21, Library/FortressLibrary.fss:623-629*

### Comparison and chaining

```fortress
a < b     a <= b     a > b     a >= b
a = b        (* value equality; opr = is the most-declared operator in the corpus, 154 declarations in 54 files *)
a =/= b      (* not equal; 18 declarations in 7 files, of which only the Any/Any fallback is NOT (a=b) *)
a === b      (* reference identity, distinct from = *)
a CMP b      (* three-way compare, returns a Comparison; LEXICO is the lexicographic one *)
```

`< <= > >= = =/=` are [fortressc]; `CMP`, `LEXICO`, `SEQV`, `NEQV` are [parses].  ⚠ 2026-08-23: `===` LEFT THIS LIST. It used to map to the same AST node as `=`, which read `a === b` as numeric equality; it is an ORDINARY LIBRARY OPERATOR -- `Library/CompilerLibrary.fsi:30` declares `opr ===(a:Any, b:Any):Boolean` and `.fss:63` defines it as reference identity, with a separate `ZZ64` overload that IS `a = b`. So reading it as `=` got the numeric case right by luck and the reference case wrong by construction. It reaches the overload set now: with no declaration in scope `3 === 4` is `` unknown name `===` ``, and a file that declares its own gets its own. Measured before the reclassification -- ZERO corpus files that compile write one, though SEVEN of this compiler's own fixtures did and every one was caught by a gate.

⚠ 2026-08-23: `AND:` AND `OR:` ARE THE CONDITIONAL FORMS and compile [fortressc]. `basic-lib/booleans.tex:211` -- "the conditional logical AND operator `AND:` examines its first argument" -- so they SHORT CIRCUIT, where plain `AND` is an ordinary operator that evaluates both. This compiler's `AND` and `OR` already short circuit, so the colon form maps onto the same node and gets exactly what the specification asks for; the over-eager half is plain `AND`, which is pre-existing. The colon must be GLUED: `a AND : b` is not this operator. 206 corpus sites.

*Seen in: Library/FortressLibrary.fss:98, Library/CompilerLibrary.fss:63, Library/FortressLibrary.fss:224-226*

Comparisons chain, the middle operand is evaluated once, and every link must have the same sense. [fortressc]

```fortress
zero<=zero<one=one<two<=two                          (* five links, all one direction *)
0 < mid(1) < 2                                       (* mid runs exactly once *)
0 <= 0 < 1 = 1 > 2 <= 2   (* SHOULD NOT PARSE: the chain reverses direction *)
```

fortressc's refusal reads "a chain mixes `<=` with `>`; chained ordering operators must have the same sense".

*Seen in: ProjectFortress/compiler_tests/Compiled10.Chain.fss:23-24, fortressc/tests/chainonce.fss:10, ProjectFortress/parser_tests/XXXchain1.fss:19*

### Boolean operators

```fortress
a AND b    a OR b    NOT a
a XOR b    a NAND b    a NOR b
a -> b     (* implication *)
a <-> b    (* equivalence, defined as a=b *)
```

`AND OR NOT` are [fortressc] with short-circuiting; `XOR NAND NOR` are [parses], reaching the checker as "unknown name `XOR`"; neither `->` nor `<->` is a token in fortressc at all, so `a -> b` lexes as `-` then `>` and stops with "expected an expression, found Gt", and `a <-> b` stops at the `<` with "expected `)`, found Lt". [legacy]

*Seen in: Library/FortressLibrary.fss:4227-4235, ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:948-949*

```fortress
NOT true AND true    (* prefix binds above every infix: (NOT true) AND true *)
NOT x < y            (* (NOT x) < y, and the refusal is the proof: on ZZ32 operands fortressc says "NOT takes Boolean operands; this one is ZZ32", naming NOT's operand rather than the comparison *)
false AND loud()     (* short-circuits: loud() never prints *)
```

*Seen in: fortressc/tests/logical.fss:23-25, fortressc/tests/badnot.fss:11*

A colon suffix selects an overload whose right operand is a thunk, `()->Boolean`. Counting the whole family `AND: OR: LEXICO: PRINTTIME: TIMING: SYMMETRIC_PARTIAL: TESTAND:`, 258 uses across 85 files. [legacy], fortressc fails to parse at the colon with "expected an expression, found Colon".

```fortress
self.depth ≤ 46 AND: |self| ≥ fib[self.depth]   (* the subscript is only forced if the bound holds *)
(self = other OR: self > other)
opr AND(a:Boolean, b:()->Boolean):Boolean = if a then b() else false end
```

*Seen in: Library/String.fss:42, Library/FortressLibrary.fss:225, SpecData/examples/library/ConditionalOps.fss:22-23*

Colons on BOTH sides make BOTH operands thunks, which is a third overload and not another spelling of the second.

```fortress
opr OP(a:ZZ32, b:ZZ32) : ZZ32 = 1
opr OP(a:ZZ32, b:()->ZZ32) : ZZ32 = 2         (* what a OP: b selects *)
opr OP(a:()->ZZ32, b:()->ZZ32) : ZZ32 = 3     (* what a :OP: b selects *)
assert(3, 5 :OP: 6)                           (* the test pins it: the answer is 3, not 2 *)
((a SIM b) AND (b SIM a)) :IMPLIES: (a = b)   (* real use, inside a property *)
```

Two sites in the whole corpus against 258 for the trailing colon, and nothing writes the left-only `:OP`. [legacy]: fortressc stops at the leading colon with "expected `)`, found Colon".

*Seen in: ProjectFortress/tests/ConditionalOpTruncation.fss:15-20, Library/incomplete/advanced/Fortress.PartialTotalOrders.fss:15*

### Concatenation and line separators

All [legacy]: fortressc lexes `||` and `//` as tokens and the parser refuses them.  ⚠ 2026-08-23: `||` WORKS as string concatenation -- `println("a" || "b")` prints `ab`, PLAIN with no separator per the juxtaposition ruling -- and it is the one builtin a user declaration beats, because it is an ordinary library operator. `//` is still refused.

```fortress
a || b      (* string concatenation, and list append *)
a ||| b     (* like ||, but inserts a space unless one side is empty *)
a // b      (* a, one line separator, b *)
// self     (* prefix form: separator before *)
x//         (* postfix form: separator after, and it converts x to String *)
a /// b     (* two line separators *)
```

`//` is not a comment marker in Fortress; comments are `(* ... *)`.

*Seen in: Library/FortressLibrary.fss:4033-4044, Library/FortressLibrary.fss:4057-4060, Library/FortressLibrary.fss:4206*

### Bars, brackets and subscripts at use sites

```fortress
|xs|         (* enclosing bars: size for a collection, absolute value for a number *)
|-2.5|       (* 2.5 *)
|\2.5/|      (* floor -> 2, the brackets lean away at the bottom *)
|/2.5\|      (* ceiling -> 3 *)
<|1,2,3|>    (* list literal, an enclosing operator declared in List.fss *)
a[i]         (* subscript; on fortressc's own Array type this works [fortressc] *)
foo[p] := myWidget    (* subscripted assignment, a separate operator from the read *)
```

*Seen in: ProjectFortress/tests/Brackets.fss:73, ProjectFortress/tests/Brackets.fss:50-51, SpecData/examples/advanced/OprDecl.SubscriptedAssignment.fss:39*

A subscript takes a COMMA-SEPARATED index list, one entry per dimension.

```fortress
a[i,j]       (* two dimensions *)
c[i,j,k]     (* three; this is what opr[i:ZZ32, j:ZZ32, k:ZZ32] dispatches to *)
A[1,0]       (* literal indices, straight off the array literal examples *)
b[(i,j)]     (* the same read written as ONE tuple index, and a different declaration *)
```

Counting lowercase array names alone, 419 comma subscripts across 37 files, answered by 21 `opr[...,...]` declarations in 10 files. [legacy]: fortressc's `Array[\T\]` is one dimensional, and `a[1,2]` stops at "expected `]`, found Comma".  ⚠ 2026-08-23: MULTI-DIMENSIONAL ARRAYS ARE BUILT. RANK IS PART OF THE TYPE, `a: ZZ32[2,2] = [3 4; 5 6]` then `a[1,0]` prints 5, and every dimension is bounds checked separately. The type is spelled with a SHAPE SUFFIX -- `Array[\ZZ32,2\]` is not it -- and the constructor is `array(m, n)`.

*Seen in: SpecData/examples/basic/Expr.Array.a.fss:22, ProjectFortress/demos/mm.fss:18, ProjectFortress/tests/BadBounds.fss:51, Library/Generator22D.fss:265*

### Declaring operators with `opr`

This subsection reads forward: `self`, trait and object members and `[\ \]` static parameters belong to sections 6, 7 and 8. It is also [legacy] end to end, since fortressc refuses every `opr` declaration at its first token with "reserved word `opr` is not in the implemented subset", so skim it on a first pass and come back.  ⚠ 2026-08-23: NOT legacy end to end any more; `opr` declarations compile. See the note at the head of section 4.

Fixity comes from the SHAPE of the declaration. There are zero uses of `infix`, `prefix`, `postfix`, `nofix`, `multifix` or `enclosing` as keywords anywhere in the corpus.

```fortress
opr SMAX[\T extends String\](x: T, y: T):T = if x > y then x else y end  (* two params = infix, used as "2" SMAX "3" *)
opr AND(a:Boolean, b:Boolean):Boolean = if a then b else false end
opr *(x:ZZ32, y:ZZ32) = x DOT y            (* punctuation declares exactly like a word *)
opr NOT(a:Boolean):Boolean = if a then false else true end   (* one param = prefix *)
opr INV(x: Widget): Widget = x.invert()    (* applied as INV Widget, no parentheses *)
```

*Seen in: SpecData/examples/advanced/OprDecl.Infix.fss:19, Library/FortressLibrary.fss:4229, SpecData/examples/advanced/OprDecl.Prefix.fss:23*

```fortress
opr (n: ZZ32)! = PROD[i <- 1:n]i     (* param in parens BEFORE the name = postfix; used as (5!) *)
opr (x:Any)// : String = x || newline
opr @(): ImplicitRange = ImplicitRange   (* empty param list = nofix; used as a bare @ *)
opr ()OP = ()                            (* the other nofix spelling *)
opr OP(x:ZZ32, y:ZZ32..., z:ZZ32 = 0) = ()   (* varargs and defaults, same as a function *)
```

Postfix operators are applied tight against the operand: `(5!)`, `x#`, `""†`. Factorial is not built in, it is declared in 7 files; fortressc's lexer reports "unrecognized character" for `!`.

*Seen in: SpecData/examples/advanced/OprDecl.Postfix.fss:19, SpecData/examples/advanced/OprDecl.Nofix.fss:23, ProjectFortress/compiler_tests/Compiled5.aw.fss:17-21*

More than two parameters is a fixity of its own, MULTIFIX: a chain of the same infix operator collapses onto the n-ary declaration when one of matching arity and type fits, and falls back to repeated binary application when none does.

```fortress
opr +(a:Foo, b:Foo):Foo = a
opr +(a:Foo, b:Foo, c:Foo, d:Foo, e:Foo...):String = "Surprise!"  (* four fixed operands, then varargs *)
foo(f:Foo) : String = f+f+f+f   (* FOUR operands, so the multifix declaration wins: "Surprise!" *)
bar(f:Foo) : Foo    = f+f+f     (* three: no 3-ary declaration fits, so the binary one applies twice *)
quux(a:Foo,b:Gnar)  = a+a+a+b   (* the types do not fit the multifix one either, so binary again *)
```

Exactly two `opr` declarations in the corpus take more than two parameters, this one and the varargs `opr OP` above, and only this one is applied as a chain. It lives under `not_working_static_tests/`, and the keyword `multifix` is still never written; the arity of the parameter list is the whole of it.

*Seen in: ProjectFortress/not_working_static_tests/Multifix.fss:18-32*

Inside a trait or object, `self` names one of the operands, and which side it sits on decides the receiver.

```fortress
opr IN(x:E, self):Boolean                            (* self on the RIGHT: x IN aSet *)
opr NI(self, elt: T): Boolean = (elt IN self)        (* the same operator with the operands swapped *)
opr NOTIN(elt: T, self): Boolean = NOT (elt IN self)
abstract opr AND(self, rhs:TestStatus): TestStatus   (* declared with no body *)
opr |self| : ZZ32;                                   (* an abstract method may just end in a semicolon *)
```

`abstract opr` exists in 5 files, 17 declarations; `private opr` appears in no .fss file at all.

*Seen in: Library/Set.fss:38, Library/GeneratorLibrary.fss:31-32, Library/QuickCheck.fss:795*

The enclosing bar and the floor/ceiling brackets are declared the same way, as methods on a type.

```fortress
opr |self| : ZZ32 = firstUnused - firstUsed   (* list length: 142 declarations across 45 files *)
opr |self| : ZZ32                             (* abstract, in a trait *)
opr |\self/| : ZZ = floor(self)
opr |/self\| : ZZ = ceiling(self)
```

*Seen in: Library/List.fss:306, Library/Map.fss:40, Library/FortressLibrary.fss:582*

Subscripting is an operator too, and the reading and assigning forms are two different operators. The space between `opr` and `[` is optional.

```fortress
opr [n:ZZ32]: E                        (* what a[i] dispatches to; 139 declarations in 39 files *)
opr [x: BizarroIndex] = self.bizarroFetch(x)     (* the index type is arbitrary *)
opr [i:ZZ32]:E throws NotFound = if i=0 then self.get else throw NotFound end
opr[i:I]:=(v:E):() = put(offset(i),v)            (* what a[i] := v dispatches to *)
opr[i:ZZ32, j:ZZ32, k:ZZ32] := (v:T): () = do self[ (i,j,k) ] := v end
```

*Seen in: Library/List.fss:144-145, Library/FortressLibrary.fss:1212, Library/FortressLibrary.fss:1889*

An operator whose name is a bracket pair is declared by writing the opening token, the operands, then the closing token.

```fortress
opr <| x:RR64 |>:String = "<|" x "|>"
opr ||| x:RR64 |||:String = "|||" x "|||"
opr <|[\E\] xs: E... |>: List[\E\] = list(xs)    (* the vararg operand is what makes <|1,2,3|> work *)
opr {|->[\Key,Val\] xs:(Key,Val)... }:Map[\Key,Val\] = mapping[\Key,Val\](xs)
opr { ↦[\ K,V \] xs: (K,V)... }: Map[\K,V\] = Map[\K,V\]   (* the Unicode spelling of {|-> } *)
```

Pairs attested in .fss code: `<| |>`, `<<| |>>`, `{\ \}`, `{/ /}`, `[/ /]`, `[// //]`, `{*/ /*}`, `[*/ /*]`, `(.\ \.)`, `</ \>`, `<</ \>>`, `||| |||`, `{|-> }`, `{ ↦ }`, `{ }`. `| |` and `|| ||` are already taken by the library and cannot be redeclared.

*Seen in: ProjectFortress/tests/Brackets.fss:15-32, Library/List.fss:176, Library/Map.fss:186-187*

Juxtaposition itself is an overloadable operator, which is how `a b` multiplies numbers and concatenates strings.

```fortress
opr TIMES(self, other:T): T
opr juxtaposition(self, other:T): T = self TIMES other   (* on a ring, juxtaposition is TIMES *)
opr juxtaposition(self, b:String):String = self || b     (* on String it concatenates *)
opr juxtaposition(a:Any, self):String = (""||a) || self  (* other operand position, with conversion *)
```

`juxtaposition` is the ONLY lowercase operator name in the corpus; every other word operator is ALL CAPS. fortressc implements juxtaposition natively but refuses the declaration.

*Seen in: Library/FortressLibrary.fss:343-344, Library/FortressLibrary.fss:4048-4050, Library/CaseInsensitiveString.fss:42*

`opr BIG NAME` declares the reduction or comprehension form of an operator. 163 declarations across 43 files; the `BIG SUM [i <- 1:n] ...` expression belongs with generators.

```fortress
opr BIG UNION[\R extends StandardTotalOrder[\R\]\](): BigReduction[\Set[\R\],Set[\R\]\] =
    BigReduction[\Set[\R\],Set[\R\]\](SetUnionReduction[\R\])   (* nullary form: returns the reduction object *)
opr BIG <|[\T\] g:Generator[\T\]|>:List[\T\] =                  (* generator form of a bracket: list comprehension *)
opr BIG $() :BigReduction[\RR64,RR64\] = BigReduction[\RR64,RR64\](SumRR64Red)
```

fortressc refuses the declaration at its first token, "reserved word `opr` is not in the implemented subset". The `BIG` message, "reserved word `BIG` is not in the implemented subset", is what a `BIG SUM [i <- 1:n] i` EXPRESSION gets.  ⚠ 2026-08-23: `opr BIG STAR(): ZZ32 = 1` compiles as a DECLARATION. The `BIG` refusal is still right for the EXPRESSION form.  ⚠ 2026-08-23 (later): `BIG` IS A MODIFIER ON THE OPERATOR NAME AT THE USE SITE TOO, the same way the declaration side already folded it. `BIG LEXICO()` and `BIG STAR[\T\]()` are CALLS of the names `BIG LEXICO` and `BIG STAR` -- that is how `__bigOperatorSugar` is handed a reduction object -- and `BIG <op>[gens] body` is a reduction over that operator. `SUM`, `PROD`, `MAX` and `MIN` over a RANGE fold onto the accumulator; every other operator, and any generator over a COLLECTION, is refused by name and needs the generator protocol.

*Seen in: Library/Set.fss:115-116, Library/List.fss:177, ProjectFortress/BirdyLib/Bazaar.fss:42*

Static parameters go after the operator name, and for a bracket that means after the OPENING token (see `opr <|[\E\]` above).

```fortress
opr PRINTTIME[\R\](desc:String, thunk:()->R): R = do
opr UNIONCAT[\T,U\](a: Map[\T, List[\U\]\], b: Map[\T, List[\U\]\]): Map[\T, List[\U\]\] =
opr (x:I)#[\I extends AnyIntegral\] : LeftRange[\I\] = left1Range(0 asif ZZ32, x)  (* postfix: after the trailing symbol *)
```

Bounds are written inline as `[\T extends Bound\]`; no operator declaration in the corpus carries a `where` clause.

*Seen in: Library/Timing.fss:36, Library/Pairs.fss:103, Library/FortressLibrary.fss:3835*

An operator can be a static parameter of a trait or object. 109 such parameters across 20 files.

```fortress
trait IdentityOp[\T extends IdentityOp[\T,ODOT\], opr ODOT\]
  opr ODOT(self):T = self      (* the parameter is used bare inside the body *)
end
trait Compareish[\ T, opr <=, opr > \] comprises T
   opr <=(self, other:T): Boolean = NOT (other > self)
end
object Foo(x:ZZ32) extends Compareish[\ Foo, BELOWEQ, ABOVE \]   (* instantiated with bare names, no opr keyword *)
```

*Seen in: SpecData/examples/basic/StatParam.Opr.IdentityOp.fss:19-21, ProjectFortress/other_compiler_tests/OprPa1.fss:16-22, Library/FortressLibrary.fss:2823*

An operator is named in an import or except list by prefixing it with `opr`.

```fortress
import List.{opr <| |>}                      (* an enclosing operator: both halves, space between *)
import Operators.{opr OP, opr |, opr | |}    (* infix bar and the enclosing bar pair are DIFFERENT names *)
import Map.{...} except { opr BIG UNION }
import List.{Cons => CC, opr <| => ||}       (* renaming on import *)
```

*Seen in: Library/FortressLibrary.fss:22, ProjectFortress/compiler_tests/Compiled9.y.fss:12, Library/Relation.fss:14*

`opr` also appears inside a `label`, `do`, `if`, `case` or `try` body. Rare: 12 declarations across 3 files, 10 of them in one. The corpus disagrees with itself about whether it is legal: `ProjectFortress/parser_tests/XXXLocalOpr.fss:17` is a test whose own message reads "Local operator declarations are not allowed!".

```fortress
opr LOCALOP(x: ZZ32) = ()
LOCALOP(3)     (* a prefix operator is also callable with parentheses *)
```

*Seen in: ProjectFortress/compiler_tests/Compiled5.at.fss:16-17, ProjectFortress/compiler_tests/Compiled5.av.fss:15-16, ProjectFortress/parser_tests/XXXLocalOpr.fss:17*

### Operator names, symbols and synonyms

An operator name is an ALL-CAPS word, a run of punctuation, or a single Unicode symbol, and several spellings can be one operator.

```fortress
assert(2 * 3, 3 ∗ 2)             (* ASCII * and U+2217 are ONE operator *)
assert(4 DOT 3, 3 ⋅ 4)           (* DOT and U+22C5 are one operator *)
assert(7 TIMES 2, 2 CROSS 7)     (* TIMES and CROSS are synonyms *)
opr $(_:Empty,_:Empty):ZZ32 = 42                (* isolated punctuation, at any fixity *)
opr ~ (self, other:Paraffin):Boolean =
opr (s: String)‡ = "Now, double dagger (U+2021) is a valid Fortress operator."
```

Attested word names include OPLUS OTIMES ODOT BOXPLUS SQCAP SQCUP CMP LEXICO MINMAX CONVERSE INVERSE SEQV NEQV EQV SUBSET SUPSET SYMDIFF UPLUS BITAND BITOR BITXOR BITNOT LSHIFT RSHIFT DIVIDES CHOOSE PREC SUCC SIMEQ VDASH DOTPLUS DOTMINUS DOTCROSS. `**`, `&&`, `!=` and `<>` do not appear in the corpus at all. `%` appears only as the character literal `'%'` in Library/Format.fss, `?` only inside the regex literals of `ProjectFortress/syntax_abstraction_tests/RegexUse1.fss`, and `+++` only in `ProjectFortress/parser_tests/XXXInvalidOp.fss:15-16`, whose whole point is that the name is refused. fortressc's lexer reports "unrecognized character" for each of `~ ! $ % ? @`.

*Seen in: ProjectFortress/tests/operatorSynonym.fss:15-21, ProjectFortress/tests/TestCompiledEnvironments.fss:49, ProjectFortress/tests/postfixTest.fss:17*

Set-theoretic and relational operators are ALL-CAPS words, most with a negated or reversed partner. [parses] in fortressc, which reports "unknown name".

```fortress
x IN s      x NOTIN s      s NI x
s UNION t   s INTERSECTION t   s DIFFERENCE t   s SYMDIFF t   s UPLUS t
s SUBSET t  s SUPSET t   s SUBSETEQ t   s SQCAP t   s SQCUP t
```

*Seen in: Library/Set.fss:38, Library/Set.fss:53-56, Library/Set.fss:63-65, Library/FortressLibrary.fss:918, Library/FortressLibrary.fss:921, Library/Map.fss:102, Library/FortressLibrary.fss:1309, Library/FortressLibrary.fss:1335*

`::` shows the same name declared at three fixities at once; `#` and `:` are declared identically in the same block.

```fortress
opr (l:I)::[\I\] : LeftRange[\I\] = l#                      (* postfix A:: *)
opr ::[\I extends AnyIntegral\](s:I) : OpenRange[\I\] = open1Range(0 asif ZZ32, s)   (* prefix ::S *)
opr ::[\I\](l:I,s:I): LeftRange[\I\] = (l:):s               (* infix A::S *)
opr :() : TrivialOpenRange = TrivialOpenRange               (* nofix : *)
```

`A::` = `A:` = `A#`, `::S` takes every Sth element, `A::S` is open to the right. The range meanings belong with generators; only the declaration shape is operator machinery.

*Seen in: Library/FortressLibrary.fss:3882-3884, Library/FortressLibrary.fss:3891, Library/FortressLibrary.fss:3869-3870*

A caret followed by an identifier is a postfix operator NAME in its own right, the name may be more than one letter, and it may be punctuation rather than an identifier at all. Six uses in two files. [parses] in fortressc, which reads the `T` as an ordinary operand of infix `^` and reports "unknown name `T`".

```fortress
opr (x:Number)^T = x^2      (* this test deliberately defines ^T as squaring *)
run() = assert(3^T, 9)      (* applied tight against the operand *)
a = A^T     b = A^TH     c = A^TT    (* more than one letter, against an object that declares none of them *)
e = A^*                              (* punctuation; fortressc stops earlier, "expected an expression, found Star" *)
d = A^(Tx)  f = A^n                  (* these two are NOT it: ordinary exponentiation *)
```

*Seen in: ProjectFortress/tests/transposition.fss:15, ProjectFortress/tests/transposition.fss:17, ProjectFortress/not_passing_yet/HatOps.fss:19-24*

### Line continuation

A bare `&` at the end of a line, followed after spacing only by a line terminator, joins the line to the next. [fortressc]

```fortress
assert(9, 3&
x)                                  (* with x = 3 the joined expression is 3 x, a juxtaposition multiply *)
println "OK &"&(* in comment & *)   (* & inside a string or a comment is an ordinary character *)
```

*Seen in: ProjectFortress/tests/ampersand.fss:19-20, ProjectFortress/tests/ampersand.fss:16*

### Unicode spellings

```fortress
self.depth ≤ 46 AND: |self| ≥ fib[self.depth]      (* ≤ ≥ for <= >= *)
opr ∈(c:Char, self): Boolean = self.javaIndexOf(c) ≠ -1     (* ∈ as the operator name, ≠ for =/= *)
splitWithOffsets(): Generator⟦(ZZ32, String)⟧ = ⟨⟦(ZZ32, String)⟧ (0, left), (|left|, right) ⟩
rightBounds' = (r0 ≪ left.size) ∩ right.bounds     (* ≪ shifts a range down, ∩ intersection *)
baseSubrange = ((start#|str|) ∩ self.indices) ≫ range.lower   (* ≫ shifts it up, the mirror of ≪ *)
self.depth > 30 AND: (¬ self.isBalanced)           (* ¬ for NOT, one use in the whole corpus *)
BIG ∨ [(start, str) ← pieces] (do                  (* ∨ disjunction, ← for the generator arrow <- *)
```

⟦ ⟧ is `[\ \]`, ⟨ ⟩ is `<| |>`, ⇒ is `=>`, → is `->`, ≔ is `:=`, ↦ is `|->`. Only Library/String.fss, ProjectFortress/tests/StringTests.fss and Library/FlatString.fss are written this way at any scale, 166, 46 and 40 non-ASCII characters outside comments and strings; 24 other files carry a handful each, mostly parser tests, so ASCII dominates the corpus by orders of magnitude. [legacy]: fortressc rejects all of them with "non-ASCII characters are not in the M1 subset outside comments and strings".  ⚠ 2026-08-23: MOST OF THESE COMPILE NOW, through a lexer ALLOWLIST. Verified working: `⟦ ⟧` for `[\ \]`, `←` for `<-`, `≤` and `≠`, `→` for `->`, `≔` for `:=`, and curly-quoted strings. The quoted message survives only for a non-ASCII IDENTIFIER, e.g. `류: ZZ32 = 1`.

*Seen in: Library/String.fss:141, Library/String.fss:378, Library/String.fss:44, Library/FlatString.fss:113*

A non-ASCII operator can also be one you declare. `⫴` is a nofix operator, called by writing
its name alone, and it exists in a single parser test. [legacy]

```fortress
opr ⫴():ZZ32 = 17     (* empty parameter list, so the name IS the whole expression *)
a = ⫴                 (* ... and this is the call *)
c = (~) + (⫴)         (* parenthesised, so it can sit inside a larger expression *)
```

*Seen in: ProjectFortress/tests/BadEncloser.fss:20, ProjectFortress/tests/BadEncloser.fss:23-25*

Identifiers may be non-ASCII too, and that is a separate fact from the operator spellings above: a name is any run of Unicode letters, and the prime suffix has Unicode spellings as well. [legacy]

```fortress
류:ZZ32 = 1                 (* a Korean identifier, declared and used like any other *)
류:ZZ32 ≔ 3                 (* the same name with the Unicode := *)
가나 : ZZ32 = 1              (* two syllables, in a different file *)
trait Emptyא end            (* a Hebrew letter INSIDE an otherwise ASCII name *)
msg‴ = "Hello, World!"      (* U+2034 TRIPLE PRIME where ASCII writes msg''' *)
```

Five files, so this is a lexer capability the corpus barely uses, and fortressc refuses it with the same message it gives the Unicode operators. Connecting punctuation is NOT a name character: `id⁔test` is what `ProjectFortress/parser_tests/XXXforbiddenConnectingPunctuation.fss:16` exists to reject.

*Seen in: ProjectFortress/tests/ho.fss:24, ProjectFortress/tests/unicodeTest.fss:15, ProjectFortress/tests/han.fss:15, ProjectFortress/tests/TestCompiledEnvironments.fss:20, ProjectFortress/tests/primeCharacter.fss:16*

## 5. Functions

A function is a name, a parenthesised parameter list, an optional `: ReturnType`, then `=` and its body. There is no `def` or `fun` introducer and no `return` statement: a function's value is the value of its body. [fortressc]

```fortress
f(x: ZZ32): ZZ32 = x        (* the minimal complete form *)
f(x:ZZ64):ZZ64 = if x < 2 then 1 else x f(x-1) end   (* the body is one whole expression *)
factorial(n: ZZ64): ZZ64 =  (* the = may end the line, body continues indented *)
  if n = 0 then 1
  else n factorial(n-1) end (* the corpus writes the parameter untyped, `factorial(n)`, and
                               fortressc answers "expected ':', found RParen" *)
```
*Seen in: fortressc/tests/plainnamed.fss:4, fortressc/tests/fact.fss:4, SpecData/examples/preliminaries/Overview.Function.factorial.fss:19-21*

Whitespace around `:` and `=` is free. For several statements, the body is a `do ... end` block.

```fortress
foo(x:RR64) = do
  y = x
  z = 2 x
  y + z                     (* the last expression is the returned value *)
end

side(): ZZ32 = do
   println("SIDE")          (* statements first, value last *)
   7
end
```
*Seen in: SpecData/examples/basic/Expr.Do.foo.fss:19-23, fortressc/tests/builtins.fss:7-10*

2943 declarations write `= do` on the signature line, 96 put the `=` alone at the end of it and start `do` on the next. `end` closes the block whatever column it sits in, and `;` separates statements that share a line: `do a = 1; b = 2; a + b end`.

### Return types

```fortress
greet(): () = println("hello from a void function")  (* () is the void return type *)
f(x: (ZZ32)): (ZZ32) = x    (* redundant parens are the type itself, not a 1-tuple *)
foo(y:ZZ32) = y             (* return type omitted, inferred from the body *)
getSampleData(name: String): (String, ReadList, ErrorRates) = do  (* signature only: tuple
                               return type, refused by fortressc as "a tuple type is not
                               implemented in this subset" [legacy] *)
```
*Seen in: fortressc/tests/unitvoid.fss:4, fortressc/tests/parenthesised.fss:4, ProjectFortress/tests/restTest2.fss:19, ProjectFortress/demos/BirdCount2b.fss:148*

Omitting the return type is only safe when nothing calls the function before its own declaration, itself included: see Overloading and recursion below.

A nullary function has an empty parameter list, and the component entry point is the nullary `run()`, present in 1591 of the corpus's 1789 `.fss` files.

```fortress
answer(): ZZ64 = 42
run() = do
   println("the pipe exists")
end
run() = print "Hello, world!"   (* a whole Hello World, and the corpus spelling. fortressc:
                                   unknown name `print`; write print("Hello, world!") *)
```
*Seen in: fortressc/tests/juxtnullary.fss:4, fortressc/tests/skeleton.fss:4, SpecData/examples/preliminaries/HelloWorld.fss:17*

Only `widen`, `println`, `array` and `length` are recognised in juxtaposition position. `print x`, `ignore x` and `assert x` each give `unknown name`, and need the parenthesised call.

`()` in the parameter position is an EMPTY LIST, not a parameter of type `()`. An actual `()`-typed parameter is refused: `f(x: ())` gives "`()` has no value, so it cannot be stored in a parameter".  ⚠ 2026-08-23: `f(x: ()): ZZ32 = 1` COMPILES: every `()` parameter is DROPPED (DEV-16). Storing `()` in a BINDING is still refused with the quoted message.

### Parameters

```fortress
arctan(x,y) = 0             (* bare names, legacy infers them; fortressc wants
                               ": Type" on every parameter and says
                               "expected ':', found Comma" [legacy] *)
ignore(_: Any): () = ()     (* _ discards the argument, still typed, still dispatches *)
subst(_:Var, _:Term): Const = self   (* _ may repeat in one list, ordinary names cannot *)
```
*Seen in: SpecData/examples/basic/Fun.App.a.fss:18, Library/CompilerLibrary.fss:35, ProjectFortress/demos/Lambda.fss:65*

A single parameter may have a tuple type, and a final parameter may be variadic. fortressc takes neither.

```fortress
first[\T1,T2\](x:(T1,T2)): T1 = do (a,_) = x; a end  (* one tuple parameter, destructured
                                    in the body; fortressc: "a tuple type is not
                                    implemented in this subset" [parses] *)
print(x:Any...):() = writes(x)      (* variadic: no space before the dots. fortressc's
                                       lexer stops there, "expected ')', found Dot" [legacy] *)
printFirst(xs: ZZ32...) =
  if xs.reduce(SizeReduction[\ZZ32\]()) > 0 then println xs[0]  (* xs indexes like a collection *)
  else throw Error end                                          (* called printFirst(3, 2, 1) *)
```
*Seen in: Library/Tuple.fss:19-20, Library/File.fss:89-90, SpecData/examples/preliminaries/Overview.Function.printFirst.fss:32-34*

`f(a, b)` (two parameters) and `f(x: (A,B))` (one tuple parameter) read identically at the call site and are different declarations. `T...` is the last parameter everywhere but two declarations, and both of those put a DEFAULTED parameter after it.

### Calling

```fortress
sin(pi)                     (* the conventional form *)
arctan(y, x)
sin x                       (* juxtaposition applies a function *)
log log n                   (* chains right to left: log(log(n)). fortressc refuses this
                               as the 3-element case below; write log(log n) [legacy] *)
double(x: ZZ64): ZZ64 = x 2 (* x is a number, so this MULTIPLIES *)
run() = println (factorial 5)
```
*Seen in: SpecData/examples/basic/Fun.App.a.fss:24-25, SpecData/examples/basic/Fun.App.b.fss:22-23, fortressc/tests/juxtapply.fss:4*

Whether `f y` applies or multiplies is decided by `f`'s TYPE, not its name. Juxtaposition binds tighter than the infix operators, which is why `x f(x-1)` means `x * f(x-1)`. A three-element juxtaposition led by a function is refused: `println(g 1 2)` gives `a juxtaposition of 3 elements led by a function is not implemented; parenthesise the application`.

```fortress
run(): () = greet()         (* nullary call: () is an empty argument list *)
run(): () = println(answer ())   (* answer () with a space is the same call *)
```
*Seen in: fortressc/tests/unitvoid.fss:6, fortressc/tests/juxtnullary.fss:6*

Naming a nullary function without `()` does not call it. `f(7)`, `f (7)` and `f 7` all do the same thing.

### Overloading and recursion

Several declarations may share one name; the argument types pick one and the most specific wins. [fortressc]

```fortress
size(x: Nil) = 0            (* one declaration per case, in place of a match *)
size(x: Cons) = 1 + size(rest(x))

name(x: Solid): ZZ32 = 1    (* Solid extends Ink, so this one wins for a Solid *)
name(x: Ink): ZZ32 = 2

size[\T\](x: T): ZZ32 = 1   (* REFUSED: an overload set is uniformly generic
size(x: ZZ32): ZZ32 = 2        or uniformly ground, never mixed *)
```
*Seen in: SpecData/examples/preliminaries/Overview.Function.size.fss:27-28, fortressc/tests/specificity.fss:13-14, fortressc/tests/badoverload.fss:6-7*

Two declarations that are neither more specific than the other were resolved arbitrarily by the legacy language; fortressc makes it a compile error naming the tuple and both declarations. No forward DECLARATION is needed for recursion in either direction, but a call to a function whose return type is not WRITTEN is typed `()` unless that function was declared earlier, and that includes a function calling itself. Drop the `: Boolean` from the pair below and fortressc says `expected Boolean, found ()`; annotating only the later declaration is enough to fix it.

```fortress
f(x:ZZ64):ZZ64 = if x < 2 then 1 else x f(x-1) end        (* self recursion *)
iseven(x: ZZ32): Boolean = if x = 0 then true else isodd(x - 1) end (* names isodd
                                                             before it exists *)
isodd(x: ZZ32): Boolean = if x = 0 then false else iseven(x - 1) end
h() = h()                                                 (* nullary self recursion, and
                                                             the one shape where the
                                                             inferred () is the right answer *)
```
*Seen in: fortressc/tests/fact.fss:4, ProjectFortress/compiler_tests/Compiled5.bt.fss:22-23*

ProjectFortress/compiler_tests/Compiled5.bt.fss writes those two branches as `tru` and `fls`. They are ordinary component-level bindings at ProjectFortress/compiler_tests/Compiled5.bt.fss:15-16 in that one file, `fls = (0 = 1)` and `tru = (1 = 1)`, not literals, so copying the corpus spelling gives `unknown name 'tru'`.

### Local functions and closures

Inside a `do ... end` block a function is declared with exactly the top-level syntax and is visible only there. The untyped spelling reaches the checker, which refuses it: `a local function declaration is not implemented; declare it at component level`. [parses] A TYPED parameter is a parse error before that check ever runs, `expected ')', found Colon`, so the mutually recursive pair below never gets as far as the local-function message. [legacy]

```fortress
run(): () = do
    isZero(x) = x = 0       (* smallest local function; this one reaches the checker *)
    println("unreachable")
  end

blah(b:ZZ32, one:ZZ32):Boolean = do
    myOdd(x:ZZ32, one': ZZ32):Boolean = if x = 1 then true else myEven(x-one',one') end
    myEven(x:ZZ32, one':ZZ32):Boolean = if x = 0 then true else myOdd(x-one',one') end
    myOdd(b,one)            (* mutually recursive locals work only when ADJACENT *)
  end                       (* fortressc stops at the colon in myOdd's parameter list [legacy] *)
```
*Seen in: fortressc/tests/localfn.fss:4-7, ProjectFortress/compiler_tests/Compiled9.m.fss:18-22*

A local sees the enclosing function's parameters, and naming it alone as the block's last expression returns it. Not rare: 132 local declarations across 70 files, with `Library/PrefixMap.fss`, `Library/FortressLibrary.fss` and `Library/Format.fss` ahead of `Library/Reflect.fss`, which is the one below.

```fortress
wrapMethod(ty:ArrowType): (Object,Any...)->Any = do
        wrapper(selfobj:Object, args:Any...): Any = do
            checkArity(ty, args)          (* closes over the parameter ty *)
            apply(selfobj, (), tupleFromIndexed[\Any\](args))
        end
        wrapper                           (* bare name, no arguments, no & operator *)
    end                     (* fortressc stops on the signature's own `Any...`,
                               "expected ')', found Dot", well before the local [legacy] *)
```
*Seen in: Library/Reflect.fss:271-277*

### Function values

An arrow type is domain, `->`, range. 1925 uses of `->` in 201 files against 4 of the Unicode `→`. fortressc parses these and then refuses them: `an arrow type is not implemented in this subset`. [parses]  ⚠ 2026-08-23: ARROW TYPES WORK: `f(g: ZZ32 -> ZZ32): ZZ32 = g(1)` called with `fn (x: ZZ32): ZZ32 => x + 1` prints 2, and an arrow RESULT works too.

```fortress
mapReduce[\R extends Any\](body: E1->R, join: (R,R)->R, id: R): R =   (* signature excerpt *)
__cond[\E,R\](c:Condition[\E\], t:E->R, e:()->R): R = c.cond[\R\](t,e) (* ()->R is a thunk *)
compose[\T, U, V\](f: T->U, g: U->V): T->V = fn x => g(f(x))   (* an arrow as RETURN type *)
e: ZZ32->String throws Error       (* throws attaches to the arrow itself *)
a: io ZZ32->String                 (* io marks an arrow effectful; 2 files in the corpus *)
```
*Seen in: Library/GeneratorLibrary.fss:132, ProjectFortress/compiler_tests/Compiled7.ApplicationErrors.fss:20, ProjectFortress/tests/typeTests.fss:31, ProjectFortress/parser_tests/ioTests.fss:18*

`fn` with `=>` is the only anonymous-function syntax: no backslash form, no Greek lambda, no `lambda` keyword. It is legacy only, `reserved word 'fn' is not in the implemented subset`.  ⚠ 2026-08-23: `fn` WORKS, typed and untyped: `f(fn x => x)` compiles.

```fortress
fn(x: RR64) => if x < 0 then -x else x end   (* fn ( dominates, 749 uses against 158 of fn( *)
fn (a:T,b:T):Boolean => a < b                (* typed parameters and a return type *)
fn (a,b) => a UNION b                        (* both annotations are optional *)
fn x => x.foo()             (* exactly one parameter needs no parentheses *)
fn () => throw NotFound     (* nullary, used as a lazy else branch *)
fn a => do b = f(a); (b,b) end   (* block body, last expression is the value *)
```
*Seen in: SpecData/examples/basic/Expr.FnExpr.fss:20, Library/QuickSort.fss:48-49, Library/Generator2.fss:335*

1059 uses in 166 files. A `fn` needs no surrounding parentheses as an argument, but does as the left operand of an operator: `(fn x => x) * 5`. The Unicode `⇒` exists but only 3 of its 14 occurrences follow a `fn`.

```fortress
h : ZZ32 -> ZZ32 = identity[\ZZ32\]           (* an existing function named as a value *)
i : ZZ32 -> ZZ32 = fn (x:ZZ32): ZZ32 => x
f1 = fn (x) => x            (* no declared type: takes the lambda's arrow type *)
rel : (E, E) -> Boolean = p(self.seed).relation()   (* value is the RESULT of a call *)
```
*Seen in: ProjectFortress/tests/sequivTest.fss:36-37, ProjectFortress/compiler_tests/Compiled5.bo.fss:15, Library/Generator2.fss:209*

Even without a type annotation fortressc gives `unknown name 'f'` for a bare function name, so a function is not a first-class value there yet. [parses]

### Contracts and other clauses

Contracts sit between the parameter list (or the return type) and the `=`. Genuinely rare, 10 uses of `requires` in 9 files, and fortressc refuses both keywords in every position. [legacy]

```fortress
factorial(n: ZZ64) requires { n >= 0 } =    (* precondition, all on the signature line *)
  if n = 0 then 1
  else n factorial(n - 1)
  end

factorial(n)
  requires { n >= 0 }
  ensures { outcome >= 1 provided true }    (* outcome is the result, provided guards it *)
  = if n = 0 then 1
    else n factorial(n-1) end

f(n: ZZ32) requires {n >= 0,
                     n + 3,                 (* several conditions, comma separated *)
                     n-5 <= 0} = ()
```
*Seen in: SpecData/examples/basic/Fun.Contract.fss:25-28, SpecData/examples/preliminaries/Overview.Function.contract.b.fss:19-23, ProjectFortress/compiler_tests/Compiled10.c.fss:19-21*

`throws` names one exception type after the return type; never a comma list, and only 2 top-level functions in the whole corpus use it, both in `CompilerBuiltin.fss`. `where` on a function is rarer still, exactly one declaration.

```fortress
makeJavaBufferedReader(s: String): JavaBufferedReader throws FileNotFoundException =
  jJavaBufferedReaderOpen(s.asJavaString)   (* fortressc: expected '=', found Reserved("throws") *)

g[\U, V\](u:U, v:V)
   where [\nat n\]{U extends {X[\n\]}, V extends {Y[\n\]}} =  (* a fresh static parameter
                             relates two otherwise unconnected types. fortressc wants the
                             clause ON the signature line: broken across two lines as the
                             corpus writes it, "expected '=', found KwWhere", and pulled up
                             onto one line, "expected '{', found LGeneric". The brace-only
                             spelling `g[\U\](u:U) where { U extends Top } = 1` compiles *)
   do
     u.f()
     v.f()
   end
```
*Seen in: ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:1079, ProjectFortress/tests/GenericFnWithExcludes.fss:36-41*

Default parameter values on a function or an operator are rare, 6 uses in 5 files, and a keyword argument at the call site rarer still. Two of the five files sit under `ProjectFortress/not_passing_yet/`, but `parser_tests/DelimitedExprTest.fss` and `compiler_tests/Compiled5.aw.fss` are ordinary tests, so this is not a form that only ever lived in the aspirational directory. fortressc refuses both. [legacy]

```fortress
f( x : ZZ32, y : ZZ32 = x, z : ZZ32 = x + y ) : ZZ32 = x + y + z  (* defaults may name
                                                        earlier parameters *)
println f(1,y=7)            (* keyword argument at the call site *)
println f(3,z=2,y=1)        (* any order, after the positional ones *)
g( (a=1), (b=2), (c=3) )    (* the extra parens force equality TESTS, not keywords *)

f(x: ZZ32 = 0) = println x  (* DelimitedExprTest's whole body, and it is a parser test ... *)
run() = f(x = 3)            (* ... and the keyword argument that goes with it. fortressc
                               stops at the default: expected ')', found Eq *)
opr OP(x:ZZ32, y:ZZ32..., z:ZZ32 = 0) = ()   (* a default on an OPERATOR parameter, and one
                                                of the two places T... is not last *)
```
*Seen in: ProjectFortress/not_passing_yet/keywords.fss:16, ProjectFortress/not_passing_yet/keywords.fss:21-23, ProjectFortress/not_passing_yet/keywords.fss:25, ProjectFortress/parser_tests/DelimitedExprTest.fss:15,17, ProjectFortress/compiler_tests/Compiled5.aw.fss:17*


## 6. Objects

An object declaration is the only way to build a value with fields. There is no `new`, no `class`, no `this`, no `super`, no `static` and no `protected`: construction is plain application and the receiver is always `self`. The core declaration forms compile under the rewrite [fortressc]; the modifiers at the end of this section do not.

### Declaring an object

```fortress
object Leaf extends Tree          (* no parens after the name: Leaf IS the value *)
  printTree():() = println "leaf"
end

object Round extends {Face} end   (* empty body, closed on one line *)
println(draw(Solid, Round))       (* the bare name is an ordinary expression *)
```
*Seen in: SpecData/examples/basic/Object.Decl.Leaf.fss:20-23, fortressc/tests/dispatch.fss:9, fortressc/tests/dispatch.fss:38*

Passing the name is not the same as juxtaposing it. `println(Marker 2)` is refused with "juxtaposition of Marker and Marker is neither multiplication nor concatenation", which is exactly what fortressc/tests/juxtsingleton.fss:6 exists to assert [legacy].

Singletons are about half of the corpus's 1858 object-declaration lines, and are the normal spelling for enum-like alternatives. The other half take a parameter list, which declares the constructor and the fields at once.

```fortress
object Cons[\T\](first:T, rest:List[\T\]) extends List[\T\]  (* the parameters ARE the fields *)
  cons(x:T): List[\T\] = Cons(x,self)          (* application, no `new`; static args inferred *)
  append(xs:List[\T\]): List[\T\] =            (* fields are in scope unqualified *)
    Cons(first,rest.append(xs))
end
```
*Seen in: SpecData/examples/basic/Object.Decl.Cons.fss:22-26, ProjectFortress/demos/BirdCount1u.fss:58-59*

The corpus writes that header over two lines with `extends` starting the second, and leaves the parameters of `cons` and `append` untyped. Both are [legacy]. A line break before `extends` ends the header and the keyword is then read as a member name, "expected a field or method name, found KwExtends"; 182 object headers in the corpus break there, and a `&` at the end of the head line restores the layout. An untyped parameter gives "expected `:`, found RParen".

`object O` and `object O()` are different declarations. The second is a nullary constructor you must apply. It is uncommon: 43 lines in 32 files.

```fortress
object Obj()
    var tmp:ZZ32 = 0              (* the var is [parses]: "mutable fields are not implemented" *)
    x():ZZ32 = tmp
end
run():() = assert(Obj().x(), 0)   (* Obj() constructs; bare `Obj` would not *)
```
*Seen in: ProjectFortress/tests/ObjectFieldShadowing.fss:17-22, Library/Reflect.fss:233*

Drop the `var` and the rest of that is [fortressc]: `tmp:ZZ32 = 0` with the same `run` exits 0.

Two or more supertraits go in braces. One may go either way, bare or braced.

```fortress
object Dog extends Animal          (* single trait, no braces *)
  noise(): ZZ32 = 1
end

object O extends {Bar[\O, P\], Bar[\P, O\]} end                     (* brace set *)
object FlatString extends { String, DelegatedIndexed⟦Char, ZZ32⟧ }  (* [legacy] ⟦ ⟧ spells [\ \] *)
object Foo extends Any                                              (* [parses] the top type *)
```
*Seen in: fortressc/tests/dottedmethod.fss:12-14, ProjectFortress/compiler_tests/Compiled10.q.fss:16, Library/FlatString.fss:34, ProjectFortress/tests/extendObject.fss:15*

The Unicode brackets are a lexer refusal, "non-ASCII characters are not in the M1 subset outside comments and strings". `Any` is not built in either: the rewrite has no root trait, so `extends Any` gives "unknown type `Any`" until you declare `trait Any end` yourself, and then it compiles.  ⚠ 2026-08-23: `Any` AND `Object` ARE REAL ROOT TRAITS NOW, seeded in the checker. `object Foo extends Any end` compiles, `f(x: Any)` and `f(x: Object)` compile and dispatch, and a user-written `trait Any end` still compiles beside the seeded one.

1347 object declarations carry `extends`, 208 of them the brace set. ASCII `[\ \]` beats the Unicode `⟦ ⟧` 20368 to 79, so write ASCII and expect to read both.

### Fields

```fortress
object Square(side: ZZ32) extends {Face}
   mark: String = "sq"        (* immutable body field, typed *)
end

object O(x : ZZ32)
  y = 2 x     (* This fails due to rewriting in terms of self. *)
  z = y + 4   (* So does this. *)
end
```
*Seen in: fortressc/tests/dispatch.fss:10-12, ProjectFortress/tests/ObjectDefVars.fss:15-17*

Those two comments are the corpus file's own, about the legacy implementation, and they name the wrong mechanism for the rewrite: what stops `y = 2 x` here is the missing annotation, "expected `:` or `(`, found Eq". Write `y: ZZ32 = 2 x` and `z: ZZ32 = y + 4` and both compile, printing 6 and 10, so a field default computed from a constructor parameter or from an earlier field works [fortressc]. `object B(n: ZZ32) m: ZZ32 = n + 1 end` with `B(4).m` prints 5.

Three spellings make a body field mutable, and they fail differently in the rewrite: `var n: ZZ32 = 3` parses and the checker refuses it ("mutable fields are not implemented") [parses], while `n: ZZ32 := 3` and `var n: ZZ32 := 3` do not parse at all, "expected a newline or `;`, found ColonEq" [legacy].  ⚠ 2026-08-23: `var n: ZZ32 = 3` COMPILES now and the field is assignable: `o.n := 7` then `o.n` reads 7. The two `:=` spellings are still parse errors.

```fortress
object Player
  thisWon : ZZ32 := 0        (* := with no var, the commonest of the three *)
  var thisLost : ZZ32 := 0   (* var plus := *)
end

object Obj()
    var tmp:ZZ32 = 0         (* var plus plain = *)
end
```
*Seen in: ProjectFortress/tests/setterTest.fss:15-17, ProjectFortress/tests/ObjectFieldShadowing.fss:17-18*

A constructor parameter prefixed with `var` is an assignable field. 31 uses in 25 files, and the parser refuses it, "expected a parameter name, found KwVar" [legacy].

```fortress
object OneShot( var canTryIt : Boolean )
    getter canTry(): Boolean = canTryIt
    tryOnce() : Boolean =
        atomic if canTryIt then canTryIt := false; true else false end
end
value object Lazy[\T\](var s : State[\T\])   (* var composes with value object *)
```
*Seen in: Library/OneShotFlag.fss:15-18, Library/Lazy.fss:18, Library/FortressLibrary.fss:4348*

A constructor parameter may carry a default. Rare, 7 uses in 7 files, and 6 of those are the same line in sibling parser tests.

```fortress
object O(d:RR64, e:ZZ32 = 3)               (* default after a non-defaulted parameter *)
test object TestSuite(testFunctions = {})  (* no type: it comes from the default *)
```
*Seen in: ProjectFortress/parser_tests/patternMatching4.fss:22, Library/incomplete/basic/Fortress.Standard.fss:52-53*

That second line is also the corpus's only `test object` [legacy]. The default is [legacy] too: `object O(d:RR64, e:ZZ32 = 3)` gives "expected `)`, found Eq", and the same header without the `= 3` compiles.

### Methods

A member with a parameter list that does not mention `self` is a dotted method. Parentheses are mandatory on the call.

```fortress
object Point(x: ZZ32, y: ZZ32)
  sum(): ZZ32 = x + y                (* fields read unqualified *)
  scaled(k: ZZ32): ZZ32 = (x + y) k
end

println(Point(3, 4).sum())     (* call straight off a constructor application *)
myNum.add(otherNum)            (* NOT myNum.add otherNum *)
```
*Seen in: fortressc/tests/dottedmethod.fss:18-21, fortressc/tests/dottedmethod.fss:28-29, SpecData/examples/basic/Expr.MthIvk.fss:29-31*

```fortress
  printSnip() = do             (* return type omitted, do ... end body *)
    println("Snip:" name pos() "  ")
    println(" " refColorsSnip)
  end
```
*Seen in: ProjectFortress/demos/BirdCount1u.fss:62-67*

Writing `self` as one of the parameters makes the member a functional method: it joins the top-level overload set of its name and is called with the receiver in the position `self` occupied. 438 declaration lines in 99 files, not counting the `opr` declarations that take `self` the same way.

```fortress
object Square(side: ZZ32) extends Shape
   area(self): ZZ32 = side side          (* self first *)
   scaled(k: ZZ32, self): ZZ32 = k side  (* self second, and the call follows suit *)
end

println(area(Square(4)))       (* never Square(4).area() *)
println(scaled(5, Square(3)))
println(area(7))               (* a plain top-level area(n: ZZ32) is in the same set *)
```
*Seen in: fortressc/tests/functionalmethod.fss:16-19, fortressc/tests/functionalmethod.fss:28-31, SpecData/examples/basic/Trait.Method.b.fss:22-24*

### Getters and setters

`getter` is the commonest object-member modifier, 2310 uses in 233 files. Declarations typecheck in the rewrite but reading one is refused ("`depth` is a getter or setter; accessors parse but are not implemented, and `depth` is read rather than called") [parses].  ⚠ 2026-08-23: READING ONE WORKS NOW. `O(5).twice` and `self.twice` both compile and run, and a getter juxtaposed onto a string -- `"Point(" self.x.asString ")"` -- is the idiom the library uses.

```fortress
    getter depth() : ZZ32 = d       (* the empty () is ALWAYS written *)
    getter tree(): ZZ32 = t         (* never takes self, never takes [\ \] *)
    getter asDebugString(): String = t.asDebugStriing
```
*Seen in: Library/Avl.fss:61-63*

A getter is read like a field. That is the whole difference from a method.

```fortress
object Test(testField: ZZ32)
    getter testField(): ZZ32 = testField + 8
    actualTestField(): ZZ32 = testField      (* bare name reads the raw FIELD *)
    internalGetter(): ZZ32 = self.testField  (* self. invokes the GETTER *)
end

    assert(t.actualTestField(),7)   (* method: parens *)
    assert(t.testField,15)          (* getter: no parens *)
```
*Seen in: ProjectFortress/tests/AliasedGetterTest.fss:15-18, ProjectFortress/tests/AliasedGetterTest.fss:23-25*

```fortress
    println(r.asString)                      (* the universal asString getter *)
    println(Foo(17,"Hello there").asString)  (* read straight off a constructor application *)
```
*Seen in: ProjectFortress/tests/ObjectToStringTest.fss:26-27*

A setter takes one parameter and is invoked by assignment. 16 uses in 13 files. A `setter` declaration parses and typechecks with a `do` or `()` body; the expression body `= fld := x` below trips the same "expected a newline or `;`, found ColonEq" as a `:=` field [legacy]. `settable` is a parser refusal too, "reserved word `settable` is not in the implemented subset" [legacy], and the assignment is refused with "only a variable or an array element can be assigned to" [parses].  ⚠ 2026-08-23: THE ASSIGNMENT WORKS AND CALLS THE SETTER [fortressc]. `o.n := e` routes to the declared `setter` -- chosen by the written modifier, not by arity, so an ordinary `n(x: T)` does not capture it -- and the body runs, including on a trait-typed receiver with the object's override winning. Still true above: the EXPRESSION body `= fld := x` does not parse; write `= do fld := x end`. `o.n += 1` is refused by name. `settable` is still reserved.

```fortress
  settable fld: ZZ32 = 0
  setter fld(x:ZZ32):() = fld := x   (* return type may be omitted *)

  player.fld := 5                    (* invoked by assignment, never setFld(5) *)
  assert(player.fld, 5)
```
*Seen in: ProjectFortress/tests/setterTest.fss:19-20, ProjectFortress/tests/setterTest.fss:39-40*

### self, and the dot

```fortress
    getter asFlatString(): String = self   (* bare self is an ordinary value *)
    m(): String = "O(" x.asString "," y.asString ") " self.m("a","b")
    opr |self| : ZZ32 = |s|                (* self inside an operator's fixity pattern *)
```
*Seen in: Library/FlatString.fss:39-40, ProjectFortress/long_term_not_working/overriding/SimpleOverriding.fss:23, Library/CaseInsensitiveString.fss:29*

Bare `self` and `self.m(...)` compile [fortressc]. The middle line reads getters, so the rewrite refuses it [parses], and `opr` is refused at the parser, "reserved word `opr` is not in the implemented subset" [legacy].  ⚠ 2026-08-23: Reading a getter and declaring an `opr` both work now, and `self` is also an operand in a JUXTAPOSITION run -- `"n: " self.x.asString` -- which it was not at the pin.

`self.field := value` has zero hits corpus-wide. From inside a member, mutation is written with the bare field name; the dotted form only ever appears on an external receiver.

```fortress
   s:Square = Square(5)
   println(s.side)                  (* constructor-parameter field *)
   println(s.mark)                  (* body field *)
   Foo.z := "bar"                   (* write through a singleton's name *)
   player.thisWon += 18             (* compound assignment works *)
   (x, player.indices, player.arr[1]) := tuple'   (* and as a tuple-assignment target *)
```
*Seen in: fortressc/tests/dispatch.fss:40-43, ProjectFortress/compiler_tests/Compiled6.be.fss:18, ProjectFortress/tests/setterTest.fss:31,34*

Reading compiles [fortressc]. Writing does not: `player.fld := 5` is refused with "only a variable or an array element can be assigned to", whatever the field was declared with.  ⚠ 2026-08-23: WRITING WORKS when the field is declared `var`. Dotted assignment is rare anyway, 12 lines in 10 files.

### Abstract, overriding, overloading

Omitting `= body` makes a member abstract. This is far commoner than writing the keyword.

```fortress
trait Animal
  noise(): ZZ32            (* bodiless: abstract, no keyword *)
end
object Dog extends Animal
  noise(): ZZ32 = 1
end
object Rock extends Animal end   (* no winner: "no declaration of `noise` applies to (Rock)" *)
```
*Seen in: fortressc/tests/badabstract.fss:7-15*

```fortress
   noise(self): ZZ32              (* the functional-method spelling of the same thing *)
    getter left() : Avl[\K,V\]     (* bodiless getter *)
    setter s(new_s:String):()      (* bodiless setter *)
```
*Seen in: fortressc/tests/badabstractfunctional.fss:9-11, Library/Avl.fss:93-95, ProjectFortress/compiler_tests/Compiled6.bi.fss:16*

Give the trait member a body and the same shape becomes an override. Nothing marks it: an object's own method beats a trait default by the most-specific rule.

```fortress
trait Animal
  noise(): ZZ32 = 0        (* default body *)
end
object Dog extends Animal
  noise(): ZZ32 = 1        (* override by redeclaration *)
end
object Rock extends Animal end   (* inherits the default *)
```
*Seen in: fortressc/tests/dottedmethod.fss:8-16*

An explicit `abstract` keyword exists, 59 uses in 18 files, always on trait members and never on an object's own. The parser refuses it, "reserved word `abstract` is not in the implemented subset" [legacy].

```fortress
  abstract getter x():ZZ32
  abstract opr <(self, other:T): Boolean
    abstract extract(self): T throws NothingInHere
```
*Seen in: ProjectFortress/compiler_tests/Compiled16.fss:20-25, Library/CompilerAlgebra.fss:18-22, ProjectFortress/BirdyLib/Maybe.fss:29*

An explicit `override` keyword also exists, and is near extinct: 4 uses in 2 files. The parser refuses it too, "reserved word `override` is not in the implemented subset" [legacy].

```fortress
object B extends A
 override   f(self, other:Number):String = "f PASS"
 override   g(other:Number):String = "g PASS"
end
```
*Seen in: ProjectFortress/tests/disp0.fss:21-24, ProjectFortress/tests/disp1.fss:21-24*

One object may declare a name several times; the call resolves on argument types.

```fortress
object A
   internalF(t:T) = e1        (* same name, different parameter type *)
   internalF(s:S) = e2
end
f1(a:A, t:T) = a.internalF(t)
f2(s:S, a:A) = a.internalF(s)
```
*Seen in: SpecData/examples/basic/Trait.Method.d.fss:22-28*

### Anonymous objects

`object ... end` in expression position builds one unnamed object, capturing what is in scope. It never takes parentheses, because there is no anonymous constructor. 77 uses in 39 files, and the rewrite does not parse it, "expected an expression, found KwObject" [legacy].  ⚠ 2026-08-23: IT PARSES AND IT RUNS. An anonymous object is HOISTED the way a `fn` already was -- a minted top-level declaration named `obj$0`, `obj$1` and so on, whose VALUE PARAMETERS are the locals its members read, with a construction of it left behind -- so a member body reads a captured name by its own spelling and nothing in codegen changed. `extends` works, so does no clause at all, fields with initializers work, a functional method works, and each anonymous object gets a type tag of its own. TWO REFUSALS BY NAME: a captured local with no written type (a constructor parameter needs one), and a captured local declared `:=`. THE MUTABLE ONE IS THE INTERESTING ONE -- the hoist COPIES, so a later `k := 9` would not be seen inside and 1.0 captures the CELL; reading one printed the value as of construction and exited 0, which is a silent wrong answer. Refused at both hoists, measured at zero corpus files. `objectCC_mutVar1.fss` is 1.0's own test for the cell semantics and it is blocked earlier, on `object O(var v: ZZ32)`.

```fortress
    r = object end             (* the minimal anonymous object *)
    println(r.asString)

f[\T\](x: T) = object          (* a whole function body *)
              f': T = x        (* captures the parameter *)
            end
```
*Seen in: ProjectFortress/tests/ObjectToStringTest.fss:25-26, SpecData/examples/basic/Expr.Object.b.fss:19-21*

Most of them carry an `extends` clause: 69 of the 77 write `object extends ...`, over 34 files, so the clauseless form above is the rarity rather than the norm.

```fortress
   o = object extends t                    (* one supertype, bare *)
       f(self):() = println "PASS"
   end
   bar():T[\X\] = object extends T[\X\]    (* an instantiated generic supertype *)
                    x():ZZ32 = foo()      (* the body still closes over the enclosing fields *)
                  end
   o := (object extends { BetterThanZZ32 }  (* the brace set works here too *)
           foo() : BetterThanZZ32 = self + self
         end)
   ignore( object extends DontExtendMe end )  (* and the whole thing is an ordinary argument *)
```
*Seen in: ProjectFortress/tests/ObjectExprWithFunctionalMethod.fss:19-21, ProjectFortress/tests/objectCC_staticParams.fss:32-40, ProjectFortress/compiler_tests/Compiled6.bg.fss:20-22, ProjectFortress/compiler_tests/Compiled0.y.fss:21*

`extends` is the only clause an anonymous object ever takes.

Inside one, `self` rebinds to the new object, so an enclosing `self` has to be captured first.  ⚠ 2026-08-23: the rewrite agrees -- the hoist never captures the name `self`, so an inner `self` is the minted object and an outer one has to be bound to another name first, exactly as below.

```fortress
object O
  m() = do outer = self
           object
             getOuterSelf() = outer      (* outer "self" *)
             getInnerSelf() = self       (* inner "self" *)
           end
        end
end
```
*Seen in: SpecData/examples/basic/Expr.Object.a.fss:19-26*

### Modifiers the rewrite refuses

Everything in this subsection is [legacy]: the corpus uses it, the rewrite rejects it at the parser, one word at a time, "reserved word `value` is not in the implemented subset" and its like. `value` marks an immutable value type rather than a reference type, 83 `value object` headers in 41 files; combined with `private` the order is `private value object`, and the reverse has no hits at all.

```fortress
value object CaseInsensitiveString(s:String)
    extends { StandardTotalOrder[\CaseInsensitiveString\] }
    opr |self| : ZZ32 = |s|
private value object SeqListGenerator[\E\]( it: FingerTree[\E\] )
```
*Seen in: Library/CaseInsensitiveString.fss:20-29, Library/PureList.fss:248*

`private` hides a whole declaration from the component's API, or one member inside a body. 61 object declarations in 15 files and 121 member lines in 20 files, out of 287 `private` keywords in 36 files once traits and top-level functions are counted in.

```fortress
private object Empty extends Treap
  private padLength = maxReadSize + 1              (* immutable field *)
  private buffer: String := ("N")^padLength        (* mutable field *)
    private refresh(s:Vector[\N,degree\]): Vector[\N,degree\] = do   (* private method *)
```
*Seen in: Library/Treap.fss:70, ProjectFortress/demos/GenomeUtil2b.fss:45-49, Library/Random.fss:277*

The rare tail, in corpus order of rarity: `settable` 4 uses in 3 files, `hidden` and `wrapped` 2 each. Both `hidden` occurrences are on trait members, inherited into objects; `wrapped` lives in one file and is the only member modifier other than `var` ever seen inside a constructor parameter list.

```fortress
  hidden x: ZZ32                 (* suppresses the getter the field would export *)
    hidden settable my: T        (* stacks, in that order *)
  settable indices: ZZ32 = 3     (* exports a setter for an otherwise-immutable field *)
object O[\Y\](wrapped x:Y) extends Y end   (* supplies inherited behaviour by delegation *)
```
*Seen in: ProjectFortress/compiler_tests/Compiled6.b.fss:15-18, ProjectFortress/compiler_tests/VarianceTest8.fss:15-17, ProjectFortress/compiler_tests/Compiled5.ao.fss:14-17*

The closing `end` may repeat the declared name. 332 lines in 100 files, and it is a general block convention rather than an object one: labels and `do` blocks close the same way. The rewrite refuses the repeat, "expected a newline or `;`, found Ident" naming the word.

```fortress
object Snip(header: String, sequence: String, name: String, pos: ZZ32, length: ZZ32, seqend: ZZ32, 
            refColorsSnip: String, sampleColorsSnip: String, refACGTSnip: String) 
  getter asString(): String = pos || "  " || name
end Snip     (* a long parameter list wraps with continuation indent; end repeats the name *)
```
*Seen in: ProjectFortress/demos/BirdCount1u.fss:58-68, Library/FlatString.fss:34,151*

## 7. Traits

A trait is Fortress's interface plus default implementations. Declarations sit at component top level, and an `object` supplies the members. `implements` and `interface` are not keywords: every corpus hit is inside a comment, and `final` is only ever an ordinary identifier (13 uses / 6 files). `sealed` and `mixin` do not occur at all, not even in a comment. Overriding is normally just redeclaring, but `override` IS a reserved word and the corpus does write it, 4 member declarations in 2 files (ProjectFortress/tests/disp0.fss:22-23, ProjectFortress/tests/disp1.fss:22-23); fortressc refuses it with "reserved word `override` is not in the implemented subset". [legacy] Trait headers are 100% ASCII `[\ \]`: the Unicode brackets run to 79 uses in 7 files and not one of them is a trait header.

```fortress
trait Animal            (* no extends clause, so it implicitly extends Object *)
  noise(): ZZ32 = 0     (* defaulted member, inherited by every implementor *)
end

object Dog extends Animal
  noise(): ZZ32 = 1     (* the override wins by the ordinary most-specific rule *)
end
```
*Seen in: fortressc/tests/dottedmethod.fss:8-14, SpecData/examples/basic/Trait.Decl.fss:20-22, Library/Reflect.fss:94-95*

Empty traits are everywhere: they are how you get a type tag or an exclusion anchor. [fortressc]

```fortress
trait Any end                     (* the root of the whole hierarchy is an empty trait *)
trait Ink end
trait Face end                    (* markers giving dispatch two independent axes *)
trait AnyMultiplicativeRing end   (* non-parametric so `excludes` can name it without a where clause *)
```
*Seen in: ProjectFortress/LibraryBuiltin/AnyType.fss:15, fortressc/tests/dispatch.fss:4-5, Library/FortressLibrary.fss:336*

### extends, comprises, excludes

```fortress
trait Region extends Equality[\Region\]              (* one supertype, no braces *)
    isLocalTo(r: Region): Boolean = false
end

trait Enzyme extends { OrganicMolecule, Catalyst }   (* braces are multiple inheritance *)
  reactionSpeed(): Speed
  catalyze(reaction) = reaction.accelerate(reactionSpeed())
                     (* [legacy] param types come from Catalyst; fortressc wants one
                        written: "expected `:`, found RParen" *)
end
```
*Seen in: Library/FortressLibrary.fss:75-78, SpecData/examples/basic/Trait.Method.e.fss:27-30*

Both spellings are legal: braceless dominates for a single supertype, the braced set is universal once there are two, and the set is unordered with no trailing comma. Two unrelated parents defaulting the same method with neither more specific is a compile error (fortressc/tests/baddiamond.fss:6-14). [fortressc]

```fortress
trait Number
        extends { StandardPartialOrder[\Number\], StandardMinMax[\Number\],
                  AdditiveGroup[\Number\], MultiplicativeRing[\Number\] }
        comprises { RR64 }   (* library convention: one clause per line, indented 8, wraps aligned under the brace *)
```
*Seen in: Library/FortressLibrary.fss:349-352*

`comprises` seals a trait: only the listed types may extend it, which makes the hierarchy exhaustive. 238 uses / 130 files, the brace form dominating about 5 to 1 (199 braced, 39 braceless).

```fortress
trait Exception comprises { UncheckedException, CheckedException }
end

private trait FTStructure[\E\] extends Sized
    comprises { FingerTree[\E\], D04[\E\] }   (* members may be instantiated generics *)

trait Equality[\T\] comprises T   (* braceless, naming its own static parameter: sealed to its instantiating type *)
```
*Seen in: Library/FortressLibrary.fss:1447-1448, Library/PureList.fss:270-271, Library/CompilerAlgebra.fss:25*

`comprises Self` is the same self-type seal with the parameter named `Self`, 12 uses / 12 files; fortressc reserves that word and refuses it, while `comprises T` compiles.  ⚠ 2026-08-23: `comprises Self` compiles now; see the `Self` note in section 8. fortressc parses and typechecks `comprises` but does NOT enforce it: a second object extending the trait compiles clean.

A literal `...` inside the brace set says the membership list is deliberately not exhaustive, hiding the rest from anyone importing the trait. Attested only in expected-fail parser tests, so read it, do not write it; fortressc answers "expected a type name, found Dot". [legacy]  ⚠ 2026-08-23: the message moved: an open `comprises` now PARSES and is refused by the checker with `` the `comprises` clause of `T` is open (`...`), which an api may write and a component may not ``.

```fortress
trait T comprises { ... }        (* every member hidden *)
trait S comprises { T, ... }     (* one named, the rest hidden *)
```
*Seen in: ProjectFortress/parser_tests/XXXComprisesHidden.fss:15, ProjectFortress/parser_tests/XXXComprisesHidden.fss:18*

```fortress
trait Generator[\E\] extends { Contains[\E\] }
        excludes { Number }     (* no type may extend both this trait and Number *)

trait Range[\I\] extends { StandardPartialOrder[\Range[\I\]\], Contains[\I\] }
        excludes Number (* Important or the strided factories can't overload! *)

trait T excludes A end
trait S excludes A end          (* minimal setup: two functional-method overloads can now coexist *)
```
*Seen in: Library/FortressLibrary.fss:957-958, Library/FortressLibrary.fss:3615-3616, SpecData/examples/basic/Trait.Method.a.fss:18-19*

Exclusion is what keeps overloads on two traits unambiguous, and though symmetric in intent the corpus writes it on both sides anyway. 123 uses / 62 files, the spellings near-evenly split (66 braced, 57 braceless), and fortressc parses and typechecks it but does NOT enforce it either.

```fortress
trait Rank1 extends { Rank[\1\]} excludes { Rank2, Rank3, Number, String }
end   (* "Potemkin exclusion traits": hand-written stand-ins for a where-quantified rule the language cannot express *)
```
*Seen in: Library/FortressLibrary.fss:1599-1600*

Clause order is fixed in practice. [legacy] parser tests reorder and repeat clauses, but nothing else does and fortressc refuses it.

```fortress
trait T excludes W extends S comprises {U, V} end            (* excludes BEFORE extends *)
trait T extends Any excludes ZZ32 comprises { O } excludes String
end                                                          (* two separate excludes clauses *)
```
*Seen in: ProjectFortress/parser_tests/XXXtraitClauses.fss:15-17, ProjectFortress/parser_tests/XXXMultipleTraitClauses.fss:15-16*

Write extends, then comprises, then excludes, then `where`. That order compiles; a reordering gives "expected a field or method name, found KwExtends".

### Members

A member with a signature and no `= body` is abstract. This is the dominant spelling; the whole of PureList, Treap and IntMap use it and never write `abstract`. [fortressc]

```fortress
trait Shape
   getter size(): ZZ32          (* abstract getter *)
   area(self, k: ZZ32): ZZ32    (* abstract functional method *)
end

trait StandardMin[\T extends StandardMin[\T\]\]
    opr MIN(self, other:T): T   (* [legacy] `opr` declarations are refused:
                                   "reserved word `opr` is not in the implemented subset" *)
end
```
*Seen in: fortressc/tests/selfgetter.fss:13-16, Library/FortressLibrary.fss:235-237*

An unimplemented abstract member is not a separate rule: a bodiless declaration is never a dispatch target, so it surfaces as the exactly-one-winner dispatch check failing (fortressc/tests/badabstract.fss:7-15).

The explicit `abstract` modifier is redundant with omitting the body, and strip the keyword and the same declaration compiles. 59 uses / 18 files, concentrated in Random, QuickCheck, BirdyLib's Comparison, the Compiled12.inherit tests, CompilerAlgebra and Stream. [legacy]

```fortress
trait TestStatus extends Equality[\TestStatus\] comprises { TestPass, TestFail, TestSkip }
    abstract getter asString(): String
    opr =(self, _:TestStatus): Boolean = false      (* abstract and defaulted mixed in one body *)
    abstract opr AND(self, rhs:TestStatus): TestStatus
```
*Seen in: Library/QuickCheck.fss:792-795, ProjectFortress/BirdyLib/Comparison.fss:23-29, Library/Random.fss:32-42*

`abstract trait` does not exist anywhere in the corpus; the modifier only ever sits on a member.

A member with an `= body` is defaulted and inherited by anything that does not override it. Library convention is to state a "minimal complete definition" in a comment and default everything else in terms of it. [fortressc]

```fortress
trait Condition[\E\] extends { ZeroIndexed[\E\], SequentialGenerator[\E\] }
    getter isEmpty(): Boolean = NOT self.holds
    getter size(): ZZ32 = if self.holds then 1 else 0 end
```
*Seen in: Library/FortressLibrary.fss:1200-1204, Library/FortressLibrary.fss:955*

Whether a member takes `self` decides how it is called, and `self` may sit in any parameter position. [fortressc]

```fortress
trait A
   f(self, t:T) = e1        (* functional method: called f(a, t) *)
   f(s:S, self) = e2        (* self second; these two only coexist because T and S exclude A *)
   internalF(t:T) = e1      (* no self, so dotted only: a.internalF(t) *)
end

object Square(side: ZZ32) extends Shape
   area(self): ZZ32 = side side
   scaled(k: ZZ32, self): ZZ32 = k side   (* scaled(5, Square(3)), never .scaled(5) *)
end
```
*Seen in: SpecData/examples/basic/Trait.Method.a.fss:23-26, SpecData/examples/basic/Trait.Method.c.fss:23-28, fortressc/tests/functionalmethod.fss:12-19*

A functional method is lifted into the top-level overload set, and the receiver keeps its source position because dispatch treats the call as an ordinary tuple.

Getters are read as `x.name` with no parentheses at the use site, setters are written on assignment. getter is one of the most common things in the corpus (2310 uses / 233 files); setter is rare (16 uses / 13 files).

```fortress
trait Treap comprises { NonEmpty, Empty }
    getter isEmpty(): Boolean       (* abstract getter *)
    private getter w(): ZZ32        (* [legacy] `private` is not in fortressc's subset *)
end

setter name(x: T): ()               (* the setter form *)
```
*Seen in: Library/Treap.fss:23-25, Library/FortressLibrary.fss:1200-1202*

fortressc lexes `getter`/`setter` and the declaration typechecks, but the use site fails: accessors parse and are not implemented, so the name is read rather than called. [parses]

A `name: Type` member with no parentheses declares a field slot the implementor must provide.

```fortress
trait T
  coerce(x: ZZ32, y: String): T
  zero: T           (* abstract field *)
end
object O extends T
  zero: T = O       (* concrete field *)
end
```
*Seen in: ProjectFortress/compiler_tests/Compiled6.n.fss:15-22*

Three modifiers exist on field members, all one-off compiler tests (settable 4 uses / 3 files, hidden 2 uses / 2 files, wrapped 2 uses / 1 file). Bare `zero: ZZ32` compiles; all three modifiers are refused as reserved words. [legacy]

```fortress
trait A
  hidden x: ZZ32              (* hidden suppresses the implicit getter *)
end
trait Test[\contravariant T extends Any\]
    hidden settable my: T     (* settable also gives the field a setter; modifiers stack in this order *)
end
trait T[\X\] extends X
  wrapped x:X                 (* the corpus's only trait-side `wrapped` *)
end
```
*Seen in: ProjectFortress/compiler_tests/Compiled6.b.fss:15-17, ProjectFortress/compiler_tests/VarianceTest8.fss:15-17, ProjectFortress/compiler_tests/Compiled5.ao.fss:14-16*

Private members are visible only inside the declaring component. 287 uses / 36 files for `private` overall, covering both members and whole declarations. [legacy]

```fortress
    private appendRC[\T\](other:List[\T\]): List[\E\]   (* private abstract helpers backing the public || *)
    private mkString(withParens: Boolean): String
```
*Seen in: Library/List.fss:130-131, Library/Treap.fss:25-45*

### Declaration modifiers

Both are [legacy]: fortressc refuses `private` and `value` as reserved words.

```fortress
private trait Sized              (* not exported from its component; 32 occurrences in 4 files, mostly PureList *)
    leafSize(self):ZZ32
end

value trait Maybe[\T\]           (* instances are values: no identity, no mutable fields; 26 occurrences in 20 files *)
        extends { AnyMaybe, Condition[\T\], UniqueItem[\T\] }
        comprises { Nothing[\T\], Just[\T\] }
    opr SQCAP(self, o: Maybe[\T\]): Maybe[\T\] = Nothing[\T\]
end
```
*Seen in: Library/PureList.fss:266-271, Library/FortressLibrary.fss:1306-1310, Library/FortressLibrary.fss:1369*

Objects extending a value trait are themselves written `value object`.

A long declaration may repeat its own name on the closing `end`. Purely documentation, 215 lines corpus-wide in 57 files, and fortressc rejects it with "expected a newline or `;`, found Ident". [legacy]

```fortress
trait Object extends Any
   getter asString(): String = jAsString(self)
end Object
```
*Seen in: ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:334-365, Library/FortressLibrary.fss:1195*

### Any, Object and Self

```fortress
trait Any end                      (* the top, an empty trait in its own native component *)
trait Object extends Any           (* everything below Object gets asString for free *)
   getter asString(): String = jAsString(self)
   getter asDebugString(): String = self.asString
end Object
cast[\T extends Any\](x:Any):T =   (* Any as a static-parameter bound and as a parameter type *)
```
*Seen in: ProjectFortress/LibraryBuiltin/AnyType.fss:12-15, ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:332-365, Library/FortressLibrary.fss:33*

Writing `extends Object` explicitly is what the spec examples do and the library does not. fortressc parses it but the checker refuses with "unknown type `Object`": its primitive set is ZZ32, ZZ64, RR64, Boolean, String, `()` and `Array[\T\]`, with no root trait. [parses]

`Self` is NOT a keyword and there is no built-in self type. It is an ordinary static parameter the trait declares itself, usually F-bounded, and the shipped library spells the identical idiom `T`.

```fortress
trait Equality[\Self extends Equality[\Self\]\]   (* 97 uses / 16 files, mostly compiler tests and aStar *)
    opr =(self, other:Self): Boolean = self SEQV other
end

trait Equality[\T extends Equality[\T\]\]         (* the library's spelling, and the one that compiles *)
    opr =(self, other:T): Boolean = self SEQV other
end
```
*Seen in: ProjectFortress/compiler_tests/Compiled17ee.fss:21-23, Library/FortressLibrary.fss:100-102, ProjectFortress/demos/aStar.fss:38-40*

fortressc reserves the word `Self` and refuses it. [legacy]  ⚠ 2026-08-23: NO LONGER. `Self` is an ordinary type VARIABLE -- 1.0 has no self-type -- and it is accepted as a static parameter name and in type position. `trait Eq[\Self extends Eq[\Self\]\]` compiles. It is still reserved everywhere else: `Self: ZZ32 = 5`, `object Self end`, `Self(): ZZ32 = 5` and `f(Self: ZZ32)` are all still refused by name.

### where clauses

The plain brace form states constraints on the trait's static parameters and always follows extends/comprises/excludes. 13 uses / 8 files: ten on a trait header, two attached to a supertype inside an `extends` set, one on an object header, and never on a function. fortressc parses it and throws it away, so a bound written only here is never enforced; the same bound moved into the bracket list refuses the violation with "Plain does not satisfy `T extends Ink`". [parses]

```fortress
trait Monoidish[\ T, opr OPLUS \]   (* [legacy] an `opr` static parameter is a second, separate
                                        refusal: "`opr` static parameters are not implemented;
                                        M3d is type parameters only" *)
  where { T extends Monoidish[\ T, OPLUS \] }   (* an F-bound that cannot be written in the bracket list *)
    ident() : T
    opr OPLUS(self, other:T):T
end

trait HasMaximalElement[\T extends HasMaximalElement[\T,PRECEQ\], opr PRECEQ\]
    extends { PartialOrder[\T,PRECEQ\] }
    where { T coerces MaximalElement[\PRECEQ\] }   (* [legacy] `coerces`: 2 uses in the whole corpus *)
```
*Seen in: ProjectFortress/tests/XXXextendOprParam2.fss:15-19, Library/incomplete/advanced/Fortress.PartialTotalOrders.fss:103-105*

The full form takes its own bracket list of fresh static parameters. 10 uses / 10 files, and whereTest.fss is the only one carrying `widens` (1 use corpus-wide) or a `type <alias> = <type>` binding inside the constraint set. fortressc rejects the bracket list with "expected a field or method name, found KwWhere". [legacy]

```fortress
trait T[\S, int i, unit U, bool b\]
  where [\bool b', nat n\]
        { S extends Number, type IntList = List[\ZZ64\],
          S widens String, NOT b, b IMPLIES b',
          n = i, U = dimensionless, 2 n + i < 2^8 }
end
```
*Seen in: ProjectFortress/tests/whereTest.fss:17-22*

An individual supertype inside an `extends { ... }` set may carry its own where clause, so the trait extends it only when the constraint holds. Two files, both of them named conditionalExtension.fss, and parse-only even in the legacy tree. [legacy]

```fortress
    extends {
      RationalQuantity[\U, ninf', lt', eq', gt', pinf', nan'\]
              where [\bool ninf', bool lt', bool eq', bool gt', bool pinf',
                      bool nan' \]
                    { ninf IMPLIES ninf', lt IMPLIES lt' },
              PartialOrderAndBoundedLattice[\...\]
              where { ninf AND pinf AND NOT nan }
            }
    where { ninf OR lt OR eq OR gt OR pinf OR nan }   (* trait-level where, after the set *)
```
*Seen in: ProjectFortress/tests/conditionalExtension.fss:21-32*

### coerce and asif

A `coerce` member declares an implicit conversion INTO the declaring type, naming the source in its parameter. 48 uses / 21 files, the largest single cluster being CompilerBuiltin's numeric tower at 15 of them. [legacy]

```fortress
trait A
  coerce(x: D) = C        (* read as "a D can become an A" *)
  coerce(t: (D, D)) = C   (* composite coercion, from a tuple type *)
end

trait ZZ extends { Number, Equality[\ZZ\] } excludes { RR64, ZZ64, ZZ32 }
    coerce(x: IntLiteral) = x.asZZ    (* one coerce per source type *)
    coerce(x: ZZ32) = x.asZZ
```
*Seen in: ProjectFortress/compiler_tests/Compiled270.fss:21-24, ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:494-497, ProjectFortress/compiler_tests/CoercionsApi.fss:14-20*

`asif` forces an expression to be seen at a supertype so dispatch and coercion pick that declaration instead of the most specific one. 196 uses / 36 files. [legacy]

```fortress
(* the dominant use: call the SUPERTYPE's version of the method you are overriding.
   Parenthesised because asif binds tighter than juxtaposition *)
getter asString(): String = "seq(" (self asif Generator[\E\]).asString ")"

(self asif String) CMP (other asif String)   (* two ascriptions in one expression *)

makeMultiJuxt():OpRef = do
    name = makeMultiFix(makeOp("juxtaposition"))
    OpRef(0, name, singleton(name asif IdOrOp), <|[\StaticArg\] |>)
end asif OpRef                               (* applied to a whole do ... end block *)
```
*Seen in: Library/FortressLibrary.fss:1176-1177, Library/String.fss:382, Library/FortressAstUtil.fss:29-32*

### Trait value parameters

A parenthesised value-parameter list on a trait is the pattern-matching feature, not a constructor. 21 uses / 16 files, every one of them named patternMatching* or Compiled7.PatternMatching.*. [legacy]

```fortress
trait Tree(item: ZZ32) comprises { Node, Leaf }   (* every member must carry this field *)
  getter depth(): ZZ32
end

object Node(left: Tree, item: ZZ32, right: Tree)
       extends Tree      (* so `typecase t of Node(l, i, r) => ...` can destructure it *)
```
*Seen in: ProjectFortress/compiler_tests/patternMatching1.fss:24-29, ProjectFortress/parser_tests/patternMatching5.fss:17-20*

### Satisfying a trait

Objects share part of the trait clause vocabulary: extends, the braced set and where. `comprises` and `excludes` are trait-only in fortressc, whose object header stops at the member list and reports "expected a field or method name, found KwComprises" or "... found KwExcludes". 1189 object headers carry an extends clause on the header line. [fortressc]

```fortress
object Solid extends {Ink} end                 (* braced form works on objects too *)
object Dotted(width: ZZ32) extends {Ink} end

object Global extends Region
    getter asString(): String = "Global"
    isLocalTo(_: Region): Boolean = true       (* supplies the trait's abstract members *)
end

object Nothing extends Maybe[\T\] excludes Just[\T\] where {T extends Object}
  isNothing = true
end   (* [legacy] two refusals in three lines: `excludes` on an object header gives
         "expected a field or method name, found KwExcludes", and the untyped field
         gives "expected `:` or `(`, found Eq" *)
```
*Seen in: fortressc/tests/dispatch.fss:7-12, Library/FortressLibrary.fss:80-83, Library/incomplete/basic/Fortress.Convenience.fss:44-46*

## 8. Generics and static parameters

A declaration becomes generic by following its name with a static parameter list between the two-character tokens `[\` and `\]`. [fortressc]

```fortress
object Cell[\T\](held: T)      (* generic object; T is a type static parameter used as a field type *)
   twice: ZZ32 = 2
end

pick[\T\](a: T, b: T, first: Boolean): T = if first then a else b end   (* generic function *)

trait List[\T\]                (* the type kind is written BARE - there is no `type` keyword *)
  first(): T
  rest(): List[\T\]
end
```
*Seen in: fortressc/tests/generics.fss:6-8, fortressc/tests/generics.fss:10, SpecData/examples/basic/StatParam.Type.fss:19-23*

`[\` and `\]` are single lexer tokens, so nothing may sit between the `[` and the `\`.

```fortress
object Rows[\ E, nat b0, nat s0, nat b1, nat s1 \](...)   (* space INSIDE the brackets is legal, tight spelling wins 32:1 *)
concat[\E\](x:List[\List[\E\]\]):List[\E\] = x.concatMap[\E\](identity[\E\])   (* nested lists close with adjacent \]\], no separator *)
run[ \]():()      (* MALFORMED: a space after [ breaks the token; the corpus has exactly one such line, as a test of it *)
```
*Seen in: Library/Generator22D.fss:62, Library/PureList.fss:142, ProjectFortress/compiler_tests/Compiled0.j.fss:14*

Several parameters are comma-separated in one bracket pair, and static arguments are strictly positional, never named.

```fortress
swap[\A, B\](x: A, y: B): B = y      (* two type parameters; the call site writes both, in order *)

trait Map[\Key,Val\]                 (* no space after the comma is the Library house style *)
      extends { Generator[\(Key,Val)\], Equality[\Map[\Key,Val\]\] }

trait PrefixMap[\E extends StandardTotalOrder[\E\], F extends List[\E\], V\]   (* three params, two of them bounded *)
```
*Seen in: fortressc/tests/generics.fss:12, Library/Map.fss:31-32, Library/PrefixMap.fss:37*

A parameter is constrained from above with `extends`, inside the bracket list. [fortressc]

```fortress
object Pen[\T extends Ink\](tip: T) end     (* instantiating at a type that does not extend Ink is a compile error *)

trait Set[\E extends StandardTotalOrder[\E\]\]    (* F-bound: the bound instantiates the very parameter it constrains *)
        extends { ZeroIndexed[\E\], ContainmentGenerator[\E,Set[\E\]\] }
```
*Seen in: fortressc/tests/badbound.fss:9, Library/Set.fss:25-26*

```fortress
[\T extends { Object, StandardTotalOrder[\T\] },U\]      (* several upper bounds, as a brace-delimited type set *)

private app[\T,E extends T,F extends T\](a:List[\E\], b:List[\F\]): List[\T\] =   (* a bound may name an EARLIER parameter *)
```
*Seen in: ProjectFortress/BirdyLib/LPairs.fss:84, Library/List.fss:93*

1144 static parameters in 207 files carry an `extends` bound. The bound is recorded by monomorphization and discharged by the checker, so a violation is a compile error naming the instantiation.

Naming a generic in a type slot requires its static arguments, and each instantiation is a distinct concrete type. [fortressc]

```fortress
i: Cell[\ZZ64\] = Cell[\ZZ64\](7)          (* declared type and constructor each carry their own list *)
s: Cell[\String\] = Cell[\String\]("hi")

comprises {NodeMap[\Key,Val\], EmptyMap[\Key,Val\]}   (* instantiation inside extends and comprises clauses *)

area(s: Box[\ZZ64\]): ZZ32 = 3     (* two instantiations of one generic object are DIFFERENT types *)
area(s: Box[\String\]): ZZ32 = 4   (* so they dispatch separately; each gets its own layout and tag *)
```
*Seen in: fortressc/tests/generics.fss:15-16, Library/Map.fss:31-33, fortressc/tests/genericdispatch.fss:15-16*

At a call site the static arguments go between the callee name and the parenthesised arguments. [fortressc]

```fortress
println(pick[\ZZ64\](3, 9, true))
println(swap[\ZZ32, String\](1, "second"))
println(O.f[\ZZ32\]())      (* dotted method: the brackets go on the method name, after the dot *)
concatMap[\G\](fn (e:E):List[\G\] => singleton[\G\](f(e)))   (* undotted self-method call, the Library's dominant shape *)
```
*Seen in: fortressc/tests/generics.fss:20-22, fortressc/tests/genericmethod.fss:38,42, Library/List.fss:169*

Legacy code often omits the static arguments and lets them be inferred. That parses and is then refused. [parses]

```fortress
x: Test[\Foo\] = Impl(Bar())   (* T inferred from Bar(); widespread in the interpreter-era tests *)
y: Foo = x.f(Bar())            (* same elision on a generic method call *)
(* fortressc: `Cell` is generic; write its static arguments, as in `Cell[\ZZ64\]`. They are never inferred *)
```
*Seen in: ProjectFortress/compiler_tests/VarianceTest1.fss:28, ProjectFortress/compiler_tests/VarianceTest4.fss:27-28*

Written static arguments are what let monomorphization run as an AST pass before anything is typed.

A method carries its own static parameters, independent of its owner's. [fortressc]

```fortress
trait T
   f[\S\](): ZZ32          (* abstract generic methods, on a NON-generic trait *)
   g[\S, U\](): ZZ32
end

concatMap[\G\](f: E->List[\G\]): List[\G\] =    (* G is the method's, E is the enclosing trait's, both in scope *)
    generate[\List[\G\]\](Concat[\G\],f)

abstract min[\T extends Integral[\T\]\](gen:RandomGen[\T\]): Maybe[\U\]   (* abstract, with an F-bounded parameter *)
```
*Seen in: fortressc/tests/genericmethod.fss:12-15, Library/List.fss:166-167, Library/Random.fss:143*

```fortress
object Cell[\T\](held: T)
   get(): T = held               (* a GROUND method on a generic owner: substituted with the owner's arguments *)
   echoed(): T = same[\T\](held)
end

foo[\T\](self, x: T): T = x      (* FUNCTIONAL method (self in the parameter list): parses, but is not lifted [parses] *)
```
*Seen in: fortressc/tests/genericowner.fss:12-15, fortressc/tests/genericfunctional.fss:12*

An overload set is uniformly generic or uniformly ground, never mixed. [fortressc]

```fortress
tag[\T\](x: Red): ZZ32 = 1        (* two uniformly generic members; instantiating yields a ground set of two *)
tag[\T\](x: Blue): ZZ32 = 2

mapping[\Key,Val\](): Map[\Key,Val\] = EmptyMap[\Key,Val\]     (* the Library shape: same-named generic factories *)
mapping[\Key,Val\](g: Generator[\(Key,Val)\]): Map[\Key,Val\] =

f(x : Any) : String = "general"                  (* [legacy] a ground member against a generic one *)
f[\X extends B\](x : X) : String = "specific"    (* each line alone compiles; the MIX breaks the uniformity rule *)
```
*Seen in: fortressc/tests/genericoverload.fss:10-11, Library/Map.fss:147-148, ProjectFortress/other_compiler_tests/SimpleBounds1.fss:21-22*

A `where` clause carries constraints the bracket list cannot express, and may introduce parameters of its own. Rare: 24 uses in 17 files.

```fortress
trait Monoidish[\ T, opr OPLUS \]
  where { T extends Monoidish[\ T, OPLUS \] }    (* the commonest form: an F-bound moved out of the bracket list *)

trait HasMaximalElement[\T extends HasMaximalElement[\T,PRECEQ\], opr PRECEQ\]
    extends { PartialOrder[\T,PRECEQ\] }
    where { T coerces MaximalElement[\PRECEQ\] }   (* position: after the extends clause, before the members *)

trait T[\S, int i, unit U, bool b\]
  where [\bool b', nat n\]                         (* [legacy] the bracket form INTRODUCES extra static parameters *)
        { S extends Number, type IntList = List[\ZZ64\],
          S widens String, NOT b, b IMPLIES b',
          n = i, U = dimensionless, 2 n + i < 2^8 }   (* type, coercion, Boolean and nat-arithmetic constraints *)
```
*Seen in: ProjectFortress/tests/XXXextendOprParam2.fss:15-16, Library/incomplete/advanced/Fortress.PartialTotalOrders.fss:103-105, ProjectFortress/tests/whereTest.fss:17-21*

fortressc ACCEPTS `where { ... }` on the declaration's own line and silently DISCARDS it, so a bound stated only in a where clause is never enforced, while the same bound written in the bracket list is. [fortressc] One pair measures it: `object Holder[\A extends Top\](it: A)` refuses `Holder[\Plain\]` at compile time and exits 1, and the same object with that bound moved into `where { A extends Top }` compiles and runs at `Holder[\Plain\]`. A where clause on a CONTINUATION line, which is how the corpus writes it, never reaches the checker at all: the parser answers `expected a field or method name, found KwWhere`.  ⚠ 2026-08-23: THIS MEASUREMENT IS INVERTED. A `where` bound IS ENFORCED now -- the same pair gives `Plain does not satisfy `A extends Top`` and exits 1 -- and `where`, `extends`, `comprises` and `excludes` all parse on a CONTINUATION line. Clause reordering works and an object header may carry `excludes`.

A generic function that instantiates itself at a strictly larger type is legal legacy Fortress that the rewrite refuses outright, exit 1 with the instantiation limit named. [legacy]

```fortress
Deep[\E\](left,arrayToFingerTree[\D23[\E\]\](arr),right)    (* the one real corpus occurrence: the finger-tree spine *)
deeper[\T\](x: T): ZZ32 = deeper[\Wrap[\T\]\](Wrap[\T\](x))  (* the same shape reduced to one line *)
```
*Seen in: Library/PureList.fss:137, fortressc/tests/polyrec.fss:10*

### Static parameter kinds beyond types

Everything from here to the end of the section is [legacy]: it exists in the corpus, and fortressc refuses it. `nat`, `int`, `bool`, `unit` and `opr` are rejected by name at the parser, which answers that they are not implemented and M3d is type parameters only.  ⚠ 2026-08-23: `nat`, `int` and `bool` ARE VALUE PARAMETERS NOW and work: `O[\nat n\]` instantiated at `O[\3\]` reads back 3. A static ARGUMENT must be statically evaluable, and a BOUND on a value parameter is refused by name. `unit` and `dim` PARSE and are refused at the INSTANTIATION instead. Only `opr` is still refused at the parser.

```fortress
makeVector[\T extends Number, nat s0\]():Vector[\T,s0\] = vector[\T,s0\]   (* nat: the argument is a natural-number VALUE *)

object O[\ nat n, bool b, int i\]()
  getN():ZZ32 = n                               (* a value parameter is usable as an ordinary expression *)
  getB():String = if b then "T" else "F" end
end
o = O[\1, true, 2\]()      (* value static arguments are bare literals, positional, with no marker syntax *)
```
*Seen in: SpecData/examples/basic/StatParam.Nat.fss:19, ProjectFortress/tests/tparams2.fss:15-17, ProjectFortress/tests/tparams2.fss:24*

`nat` carries sizes, `int` carries bases (which may be negative), and arithmetic on them is itself a static argument. 514 nat parameters in 50 files, 14 int ones in 10 files.

```fortress
array3[\T, nat s0, nat s1, nat s2\](f:(ZZ32,ZZ32)->T):Array3[\T,0,s0,0,s1,0,s2\] =
  array3[\T,s0,s1,s2\]().fill(f)          (* literal 0 bases and nat names mix freely as static arguments *)

opr BSD[\T, nat s0, nat s1, nat s2\](x : Array2[\T, 0, s0, 0, s1\], y : Array2[\T, 0, s0, 0, s2\])
      : Array2[\T, 0, s0, 0, s1 + s2\] =  (* ARITHMETIC as a static argument: s1 + s2 *)

opr <|[\T, int b0, int b1, nat s\]x:Array1[\T, b0, s\], y:Array1[\T, b1, s\]|>:T = do   (* int base, nat size *)

A = mkMatrix[\50\]()      (* a nat literal sizing a matrix at compile time *)
```
*Seen in: Library/FortressLibrary.fss:2818-2819, Library/Generator22D.fss:314, ProjectFortress/tests/setMakerTest0.fss:53, ProjectFortress/demos/conjGrad.fss:106*

`bool` and `unit` are the rare tail: 38 uses in 12 files and 6 uses in 6 files. There is no `dim` static parameter anywhere in the corpus, though the parser refuses that keyword by name too.

```fortress
trait BooleanLiteral[\bool b\] end       (* the argument is `true` or `false` *)

trait RationalQuantity[\unit U absorbs unit, bool ninf, bool lt, bool eq,
                        bool gt, bool pinf, bool nan\]   (* six bools encoding a lattice of numeric-tower facts *)

trait Float1[\unit U absorbs unit, nat e, nat s\] end    (* `absorbs unit` is the only modifier of its kind, 3 uses *)
object O[\S, int i, unit U, bool b\]() end               (* a plain unit parameter, no modifier *)
```
*Seen in: SpecData/examples/basic/StatParam.Bool.fss:19, ProjectFortress/tests/conditionalExtension.fss:19-20, ProjectFortress/tests/dimensionUnitDecl.fss:24, ProjectFortress/not_passing_yet/staticArg.fss:17*

`opr` declares a parameter ranging over OPERATORS, which the body may then declare and use as an operator. 117 uses in 26 files.

```fortress
trait IdentityOp[\T extends IdentityOp[\T,ODOT\], opr ODOT\]
  opr ODOT(self):T = self       (* ODOT is a static parameter and then a DEFINED operator method *)
end

trait TotalOrder[\T extends TotalOrder[\T,<,<=,=,>=,>\],opr <,opr <=,opr =,opr >=,opr >\]
                                (* symbol-style parameters; name-style like OPLUS or PRECEQ dominates, 95 to 22 *)
```
*Seen in: SpecData/examples/basic/StatParam.Opr.IdentityOp.fss:19-21, ProjectFortress/not_passing_yet/monoidalPolymorphism.fss:29*

An operator declaration takes its static parameters between the operator name and the parameter list.

```fortress
opr UNIONCAT[\T,U\](a: Map[\T, List[\U\]\], b: Map[\T, List[\U\]\]): Map[\T, List[\U\]\] =

opr BIG UNION[\Key extends StandardTotalOrder[\Key\],Val\]()   (* on a BIG reduction operator the brackets go after the NAME, not after BIG *)

opr BIG {/|->[\E extends StandardTotalOrder[\E\], F extends List[\E\],V\] g:Generator[\(F,V)\] /}:PrefixMap[\E,F,V\]
      = prefixMap[\E,F,V\](g)   (* an enclosing operator takes them right after the opening delimiter *)
```
*Seen in: Library/Pairs.fss:103, ProjectFortress/BirdyLib/Map.fss:181, Library/PrefixMap.fss:411*

### Variance, lower bounds and the rest of the legacy tail

Variance is annotated only on TRAIT parameters: 7 uses in 7 files, VarianceTest1, VarianceTest2 and VarianceTest4 through VarianceTest8. There is no `invariant` variance annotation.

```fortress
trait Test[\covariant T\]
  f(): T
end
object Impl[\T\](x: T) extends Test[\T\]   (* the implementing object is declared with a PLAIN parameter *)

trait Test[\contravariant T extends Any\]  (* annotation first, bound after the name *)
  setter x(y: T): ()
end
x: Test[\Foo\] = Impl[\Any\]()             (* the point of it: a Test[\Any\] serves where a Test[\Foo\] is wanted *)
```
*Seen in: ProjectFortress/compiler_tests/VarianceTest1.fss:16-20, ProjectFortress/compiler_tests/VarianceTest6.fss:15-17, ProjectFortress/compiler_tests/VarianceTest6.fss:26*

`dominates` is the only lower bound in the language, 2 uses in 1 file, and it exists for covariance.

```fortress
trait Test[\covariant T\]
  f[\U dominates T\](x: U) : U    (* U must be a SUPERtype of T, which keeps f sound under covariance *)
end
```
*Seen in: ProjectFortress/compiler_tests/VarianceTest4.fss:15-17*

A recurring convention names the F-bounded parameter `Self` so a trait can talk about its implementing type. Nothing about the name is special to the grammar, and fortressc reserves the word.

```fortress
trait Equality[\Self extends Equality[\Self\]\]
    opr =(self, other:Self): Boolean = self SEQV other   (* Self is then an ordinary type in signatures *)

trait StandardTotalOrder[\Self\]           (* the same name used with NO bound *)
        extends { StandardMinMax[\Self\] }
(* fortressc: reserved word `Self` is not in the implemented subset *)
```
*Seen in: ProjectFortress/compiler_tests/Compiled17ee.fss:21-22, ProjectFortress/not_working_library_tests/MatchErrorBug.fss:33-34*

U+27E6 and U+27E7 are the Unicode spelling of `[\` and `\]`, usable in every position. 79 occurrences in 7 files against 20368 ASCII `[\`, roughly 260 to 1, and fortressc refuses them in the LEXER, not the parser: the core grammar is ASCII on purpose.

```fortress
object ConcatGenerator⟦T⟧(first:Generator⟦T⟧, second:Generator⟦T⟧)
      extends Generator⟦T⟧
    generate⟦R⟧(r: Reduction⟦R⟧, body:T→R):R =        (* declared and then called with Unicode arguments *)
        r.join(first.generate⟦R⟧(r, body), second.generate⟦R⟧(r, body))

a: Array1⟦ZZ32, 0, 47⟧ = array1⟦ZZ32, 47⟧()    (* mixed type and numeric static arguments *)
(* fortressc: non-ASCII characters are not in the M1 subset outside comments and strings *)
```
*Seen in: Library/String.fss:260-263, Library/String.fss:22*

List, set and map aggregates take a static argument list butted straight against the opening bracket, with the elements one space later.

```fortress
getter ranges() = <|[\CompactFullRange[\ZZ32\]\] |>      (* an EMPTY aggregate has no other source for its element type *)
gmp = geometricMean <|[\RR64\] e.errorProb | e <- events |>    (* a comprehension carrying its element type *)
opr {[\E extends StandardTotalOrder[\E\]\] es: E... }: Set[\E\] =   (* the set-aggregate operator's own declaration *)
```
*Seen in: Library/Pairs.fss:84, ProjectFortress/BirdyLib/BirdCount2c.fss:90, Library/Set.fss:106*

`FORALL` quantifies a law over a generic trait's parameters. It binds ordinary VALUE variables, not static parameters, and only ever heads a `property` declaration. 16 uses in 3 files.

```fortress
trait TotalOrder[\T extends TotalOrder[\T,PRECEQ\], opr PRECEQ\]
    extends { PartialOrder[\T,PRECEQ\] }
  property FORALL (a: T, b: T) (a PRECEQ b) OR (b PRECEQ a)   (* totality, over the type and operator parameters *)
end

property fIsMonotonic = FORALL(x: ZZ, y: ZZ) (x < y) IMPLIES (f(x) < f(y))   (* the NAMED form *)
```
*Seen in: Library/incomplete/advanced/Fortress.PartialTotalOrders.fss:26-29, ProjectFortress/parser_tests/DeclTest.fss:19*

## 9. Control flow

Every control construct is an expression, closes with `end`, and yields the value of the branch taken. There is no `return`, no `break`, no `continue` and no `goto`.

### if

```fortress
if x > 0 then
  println(x "is positive")
else
  println(x "is nonpositive")
end                                       (* no `end if`, a bare end closes it *)

f(x:ZZ64):ZZ64 = if x < 2 then 1 else x f(x-1) end  (* one line, no do wrapper needed *)
if even(n) then n := n / 2 else n := 3 n + 1 end    (* both branches for effect *)
```

The condition is not parenthesised by convention, though parens are legal. [fortressc]

*Seen in: Documentation/Specification/Code/If2.fss:21-25, fortressc/tests/fact.fss:4, fortressc/tests/parallelcollatz.fss:13*

```fortress
if x > 0 then println x end               (* else is optional; the value is then () *)
if r0.isEmpty then exit method with EmptyString end   (* guard-clause idiom; the exit is *)
                (* [legacy]: "reserved word `exit` is not in the implemented subset" *)
```

An else-less if cannot go where a value is wanted, and its `end` is never optional. Around 370 of the 2253 ifs: there are 2086 elses and 203 of those are the `else =>` arms of case and typecase. [fortressc]

*Seen in: Documentation/Specification/Code/If1.fss:21, Library/String.fss:138*

```fortress
if   (z = val) then self
elif (z < val) then balancedAdd(val,left.add(z),right)   (* any number of elif arms *)
else balancedAdd(val,left,right.add(z))
end                                       (* ONE end closes the whole chain *)

z = if x < 0 then 0                       (* if is an expression: bind it, pass it, juxtapose it *)
    elif x IN {1, 2, 3} then 3            (* the brace set literal is [legacy]: fortressc says *)
    elif x IN {4, 5, 6} then 6            (*   "expected `then`, found LBrace" *)
    else 9 end
```

488 elif uses in 102 files, and 1883 of the 2253 ifs carry an else, which is what lets an if sit in value position. [fortressc]

*Seen in: Library/Set.fss:262-265, Documentation/Specification/Code/If4.fss:21-24*

```fortress
if right > left
then pivotIndex = (left+right) DIV 2      (* a branch is BlockElems, not one expression: *)
     pivotNewIndex = mypartition(lt, arr, left, right, pivotIndex)  (* locals live here *)
     do
       quicksort(lt, arr, left, pivotNewIndex-1)
     end
end                                       (* DIV itself is [parses], "unknown name `DIV`" *)
```

Elements are separated by a newline or `;`, and names bound in a branch are scoped to it. [fortressc]

*Seen in: Library/QuickSort.fss:37-45*

```fortress
println(x "is" (if v > 0 then "positive" else "nonpositive"))
                          (* inside parens and WITH an else, the end may be dropped *)
(if |s| = 0 AND: r.eof() then Nothing[\String\] else Just s)

if (min_elt, del_min) <- r.extractMinimum() then   (* generator condition: the binders are *)
    balancedAdd(min_elt, self, del_min)            (*   in scope only in the then-branch *)
else
    self                                           (* taken when the generator produces nothing *)
end
```

Both are [legacy]. fortressc rejects the end-less form with `expected a newline or ';', found RParen`, and does not even lex `<-` here (`expected 'then', found Lt`). Dropping the end is a choice, not a rule: Library/IntMap.fss:337 keeps it inside parens.

*Seen in: Documentation/Specification/Code/If3.fss:21, Library/FileSupport.fss:105, Library/Set.fss:274-278*

```fortress
if (z = val) then self
else if (z < val) then balancedAdd(val,left.add(z),right)   (* pre-elif spelling, *)
   else balancedAdd(val,left,right.add(z))                  (*   one end PER LEVEL *)
   end
end
```

41 uses in 26 files, counting the ones that put the `if` on the line below the `else`; `elif` collapses the same chain to one end. The three deepest examples, the BirdyLib CMP methods, are commented out in their own files. [fortressc]

*Seen in: ProjectFortress/tests/treeTest.fss:137-141*

### while

```fortress
while x < 10 do                (* `do` is mandatory; there is no do/while and no repeat/until *)
  println x
  x += 1                       (* [legacy] compound assignment, both of them; write x := x + 1 *)
end
while (k < 10) do              (* parens round the condition are optional, 38 of the 157 uses *)
  k += 1
end
while x >= retrybound do x := gen.random() end   (* one-liner, do and end still required *)
```

157 loops in 90 files; a while's own value is (). A loop that must produce a value is written `while true do ... end` inside a `label` and escaped with `exit ... with`. [fortressc]

*Seen in: SpecData/examples/preliminaries/Overview.Expression.whileE.fss:21-24, Documentation/Specification/Code/WhileLoop1.fss:22-25, Library/Random.fss:381*

```fortress
while (next <- cursor) do      (* generator condition: binds a fresh name each iteration, *)
  println(next.item)           (*   stops when the generator is empty. 7 uses in 7 files. *)
  cursor := next.link
end
```

[legacy]; fortressc stops at the arrow, `expected ')', found Lt` with the parens and `expected 'do', found Lt` without them.

*Seen in: Documentation/Specification/Code/WhileLoop2.fss:22-25*

```fortress
while do foo := foo - 1        (* the CONDITION is itself a do block, last element is the test *)
         (foo > 0)
      end do                   (* `end do` is NOT a decoration: end closes the condition, *)
  println(foo)                 (*   the following do opens the body. 1 use in 1789 files. *)
end
```

This one compiles, and it is the corpus's only do-while equivalent. The cited file still does not build, because its counter is declared `var foo : ZZ32 = 7`, which fortressc refuses at the parser. [fortressc]

*Seen in: ProjectFortress/other_compiler_tests/whiledo.fss:17-21*

### Blocks

```fortress
do
  f(w: ZZ32) = w+1     (*) [parses] local function: "expected `)`, found Colon" at the typed
                       (*)   parameter, then "declare it at component level" once it parses
  y = x+1              (*) Local variable declaration (immutable)
  var z: RR64 = 0      (*) [parses] var: "expected an expression, found KwVar"; write z: RR64 := 0.0
  z += f(y)            (*) [legacy] compound assignment: "expected an expression, found Eq"
  |z|                  (*) [legacy] size bars: "expected an expression, found Bar"
end
```

A `do` block opens a new scope, nests freely, and evaluates to its LAST element. The block itself is [fortressc]; four of the five elements above are not, each flagged with the diagnostic it draws. The `steps` block below is one that compiles as written.

*Seen in: Documentation/Specification/Code/Block1.fss:21-27*

```fortress
steps(start: ZZ64): ZZ64 = do
   n: ZZ64 := start
   count: ZZ64 := 0
   while n > 1 do
      if even(n) then n := n / 2 else n := 3 n + 1 end
      count := count + 1
   end
   count                       (* no return: the trailing expression IS the result *)
end
writeOn(stream: WriteStream): () = do end   (* empty block, value (). 21 uses in 19 files *)
```

A block whose last element is a declaration or an else-less if has value (). The same rule governs if branches, case arms, typecase arms and label bodies. [fortressc]

*Seen in: fortressc/tests/parallelcollatz.fss:9-17, Library/String.fss:315*

```fortress
first[\T1,T2\](x:(T1,T2)): T1 = do (a,_) = x; a end   (* `;` separates elements like a newline *)
        (* [parses] tuple type and binder: this passes only because nothing instantiates it, *)
        (*   and a call draws "a tuple type is not implemented in this subset" *)
run():()=do
   test1();
   test2();          (* a trailing `;` before end/else/elif/also/catch/finally is legal *)
end
```

A semicolon is not a statement terminator and most of the corpus omits it. [fortressc]

*Seen in: Library/Tuple.fss:19, ProjectFortress/tests/caseTest.fss:92-95*

```fortress
(factorial(10), factorial(5), factorial(2))  (* tuple elements run as separate implicit threads *)
(a, b, c, d, e, f, g, _) = h()               (* tuple binder; `_` discards a component *)
```

The comma never sequences statements. fortressc parses a tuple then refuses it: `a tuple expression is not implemented in this subset`. [parses]

*Seen in: SpecData/examples/preliminaries/Overview.Expression.tuple.factorial.fss:20, SpecData/examples/preliminaries/Overview.Expression.tuple.fss:24*

```fortress
do
  accum += treeSum(t.left)
also do                        (* sibling implicit threads; only the LAST block gets an end *)
  accum += treeSum(t.right)
also do
  accum += t.datum
end
run() = do 3 also do 5 end     (* smallest legal group in the corpus *)
```

39 uses in 22 files, and the group is one expression, so it can be assigned. Each arm may carry its own `atomic` or `at` prefix, as in `also atomic do`. [legacy]

*Seen in: SpecData/examples/basic/Expr.Do.treeSum.fss:26-32, Library/CovariantCollection.fss:52-55, ProjectFortress/compiler_tests/Compiled10.a.fss:14*

### label and exit

`label` is the only early-escape mechanism in the language, and the whole family is [legacy].  ⚠ 2026-08-23: THE FAMILY WORKS: `label out ... exit out with 5 ... end out` runs and yields 5. `exit` still cannot leave a `for` body, because every loop body is OUTLINED.

```fortress
label out
    for g<-seq(0#|string|) do
        if string[g] = find then
            exit out with Just[\ZZ32\](g)   (* jump out; that value becomes the block's value *)
        end
    end
    exit out with Nothing[\ZZ32\]           (* fall-through value *)
end out                                     (* the end REPEATS the label name, always *)
```

111 label introductions in 50 files, and 108 close with `end <name>`; the name is not optional, and the three exceptions are all XXX-prefixed parser tests, one of them named XXXWrongLabel.fss. Labels nest, and the inner name is out of scope after its own end.

*Seen in: Library/Format.fss:127-134*

```fortress
lookup(kk:K): Maybe[\V\] = label done   (* a label can BE the whole method body *)
exit done with Just[\V\](tt'.val())     (* labelled + valued: 77 of the 109 exits *)
exit with just candidate                (* no label: targets the innermost label. 12 uses *)
exit gobble                             (* labelled, no value: leaves the block with (). 18 uses *)
exit                                    (* bare, the nearest thing to break. 2 uses, 1 file *)
```

There must always BE an enclosing label; an exit with no target in scope is a static error.

*Seen in: Library/Avl.fss:27,35, Library/QuickCheck.fss:1037, ProjectFortress/demos/GenomeUtil2b.fss:116*

```fortress
object Meth(return: Type, name: String, params: List[\Decl\],
            body: Expr)              (* `return` used as a FIELD NAME, so it is not reserved *)
end
if list[i] = key then
    index := i
    exit                             (* what stands in for break: the answer is left in a mutable *)
end
```

Zero keyword uses of `return`, `break`, `continue` or `goto` in 1789 files. `return` occurs once, as the field name above; `break` three times, all the method at Library/SkipList.fss:240 and its two calls; `continue` and `goto` never.

*Seen in: ProjectFortress/demos/FeatherweightJava.fss:166-168, Library/incomplete/SkipTree.fss:417-419*

### case

All case forms are [legacy]; 93 uses in 51 files.

```fortress
case 2 + 2 of
    4 => ()                              (* guard, `=>`, block; first guard comparing equal wins *)
    5#2 => fail("caseTest test1 failed") (* guards are ordinary expressions, here a range *)
end
case front of
    "w" => widthLeft := Just[\ZZ32\](readInt())    (* string guards, arms run for effect *)
    "ww" => widthRight := Just[\ZZ32\](readInt())
end
```

Arms are separated by a newline or `;`, one expression per guard, and the default comparison is `=`. There are no comma-separated multi-guard arms.

*Seen in: ProjectFortress/tests/caseTest.fss:17-20, Library/Format.fss:213-216,221*

```fortress
case 2+2  of
    3 => fail("caseTest test6 failed")
    else => ()                (* `else =>` is the catch-all and must be the LAST arm *)
end
case planet IN of             (* an operator before `of` replaces the implicit `=` ... *)
  {"Mercury", "Venus", "Earth", "Mars"} => "inner"   (* ... so this tests `planet IN guard` *)
  else => "remote"
end
case most > of                (* no subject: the arm with the most EXTREME guard wins *)
  1 mile => "miles are larger"
  1 kilometer => "we were wrong again"
end
```

The operator form is 14 uses in 6 files. `most` is 6 uses in 2 files, the operator after it is required, and no corpus example gives it an else.

*Seen in: ProjectFortress/tests/caseTest.fss:56-60, SpecData/examples/basic/Expr.Case.a.fss:21-25, SpecData/examples/basic/Expr.Extremum.fss:21-24*

```fortress
encodeACGT(c: Char): ZZ32 = case c of 'A' => 0; 'C' => 1; 'G' => 2; 'T' => 3; end
                                (* whole case on one line, trailing `;` before end allowed *)
chars => do                     (* an arm body is BlockElems: one expression, an explicit *)
    result := result string[i]  (*   do block, or an indented multi-line block *)
    exit out with result
end
else => result := result string[i]
```

*Seen in: ProjectFortress/demos/BirdCount1m.fss:77, Library/Format.fss:382-386*

### typecase

Dispatch on the RUN-TIME type. Arms are tried in order, so more specific types go first. 213 uses in 78 files, all [legacy].

```fortress
typecase myLoser.myField of
  x:String => x "foo"           (* `name:Type` rebinds the name at the narrowed type for that arm *)
  x:Number => x + 3
  Object => yogiBerraAutograph  (* a bare type arm matches but does not rebind *)
end

typecase x of
  String => x
  else => x.asString            (* `else` is the default arm *)
end
```

Without an `else` the arms must cover the subject's type.

*Seen in: SpecData/examples/basic/Expr.Typecase.fss:25-29, Library/incomplete/basic/Fortress.Convenience.fss:16-19*

```fortress
cast[\T extends Any\](x:Any):T =
  typecase x of
    x':T => x'               (* the prime-suffixed name is the corpus convention *)
    else => throw CastError
  end
typecase e CMP val of        (* the subject is any expression; arms here are singleton types *)
    LessThan => left.indexOfI(i,e)
    EqualTo => Just[\ZZ32\](i + |left|)
    GreaterThan => right.indexOfI(i + |left| + 1,e)
end
```

*Seen in: Library/FortressLibrary.fss:33-37, Library/Set.fss:226-229*

```fortress
typecase (1, 2, 3) of
  (String, ZZ32) => println "Fail"      (* tuple subject, tuple-type arms *)
  (ZZ32, String) => println "OK"
  else => println "Fail"
end
typecase child = children[childIndex] of   (* binds the subject so the arms can reuse it *)
    Just[\Node[\Key,Value\]\] => child.get().find(querykey)
    else => Nothing[\Value\]
end
```

An arm may also give each tuple component its own binder, `(gg':Generator2[\E\], ...) =>`. The binding-subject form is 11 uses in 3 files, 8 of them in SkipTree.

*Seen in: ProjectFortress/compiler_tests/Compiled5.s.fss:15-19, Library/incomplete/SkipTree.fss:177-180, Library/FortressLibrary.fss:1152-1153*

### end

```fortress
end                          (* closes exactly ONE construct: do, if, while, for, case, *)
                             (*   typecase, label, try, atomic, object, trait, component *)
end method                   (* a LABEL's end MUST repeat its name; a component, object or *)
                             (*   trait end MAY repeat its own, and fortressc takes only the *)
                             (*   component one, because it stops parsing at that end *)
end typed String             (* `typed T` is the general postfix ascription on the block *)
end outer typed IntLiteral   (* both: label name, then ascription *)
```

11546 uses in 1759 files, the most common reserved word in the corpus (`self` is next, at 5566). There is no `end if`, `end while` or `end case`. The bare `end` is [fortressc]; the label-naming ends are [legacy] with the rest of `label`, and `end typed T` draws `expected a newline or ';', found Reserved("typed")`.

*Seen in: Library/String.fss:149, ProjectFortress/compiler_tests/Compiled280.fss:32, ProjectFortress/compiler_tests/Compiled9.u.fss:25-27*

## 10. Loops, generators and comprehensions

Fortress has exactly one generator-driven iteration statement and a generator is the only thing it can iterate: there is no C-style `for(init;test;step)` anywhere in the corpus. `while` is the other loop, 157 uses in 90 files, and it belongs to the control-flow section. The body runs in parallel by default. The rewrite compiles the core loop, the `#` and `:` ranges, `seq` in a header and the bare `_` binder; everything else in this section is [legacy], present in the corpus but not yet accepted by fortressc.

```fortress
for i <- 0#n do          (* bind i to each element the generator produces *)
   a[i] := steps(i + 1)
end                      (* `do` is mandatory, `end` closes the loop *)

for x <- l4 do println x end   (* do/body/end on one line for short bodies *)
```
*Seen in: fortressc/tests/parallelcollatz.fss:22-24, SpecData/examples/preliminaries/Overview.Expression.forLoop.fss:20-22, ProjectFortress/BirdyLib/TestLPairs.fss:17*

Indentation is not significant [fortressc]. 718 loop headers across 200 files.

```fortress
for i <- seq(0#n) do arr[i] := random() end               (* the dominant ASCII arrow, 2384 uses *)
for i ← seq(sequence.indices.flip) do                     (* ← is the same token, not a different one *)
for w ← seq(words) do testString := CatString(testString, w) end
```
*Seen in: Library/Random.fss:48, Library/String.fss:196, ProjectFortress/tests/StringTests.fss:46*

32 Unicode uses in 6 files against ASCII's 2384 in 292 files, and fortressc refuses the Unicode form outright: "non-ASCII characters are not in the M1 subset outside comments and strings" [legacy].  ⚠ 2026-08-23: `for i ← 0#3 do ... end` COMPILES; `←` is on the lexer allowlist. Spacing round the arrow is free, `x <- g` and `x<-g` both occur.

### Ranges

```fortress
for i <- 0#n do            (* a#n: n consecutive values starting at a, so 0 .. n-1 *)
   a[i] := i i + 7
end

acc:ZZ64 := 0
for j <- 0:i do            (* a:b is INCLUSIVE at both ends, so this runs i+1 times *)
    acc := acc + j
end
```
*Seen in: fortressc/tests/parallelfill.fss:14-16, SpecData/examples/basic/Expr.Do.mySum.fss:20-23*

604 generator clauses use `#`, 205 use a `:` range. Spacing is free (`1 # 4`, `0 # |x|`) and negative bounds are fine (`seq(-10:10)`). Both are ordinary infix operators declared in the library, not lexer magic, and both have overloads taking tuples for 2D and for 3D.

```fortress
opr #[\I extends AnyIntegral\](lo:I, ex:I): CompactFullParScalarRange[\I\] = sized1Range(0 asif ZZ32,lo,ex)
opr :[\I extends AnyIntegral\](lo:I, hi:I): CompactFullParScalarRange[\I\] =
        bounded1Range(0 asif ZZ32,lo,hi)
```
*Seen in: Library/FortressLibrary.fss:3813, Library/FortressLibrary.fss:3823-3824*

fortressc parses `#` and `:` ranges ONLY inside a for header; `r = 0#3` as a free binding is a parse error.

```fortress
for i <- seq((|a| - 1):-1:-1) do    (* a:b:c adds a stride; negative counts down *)
for  n <- seq(min:max:stride) do    (* header only: all three components may be variables *)
```
*Seen in: Library/Shuffle.fss:23, ProjectFortress/demos/trips.fss:305*

A stride is [legacy]: fortressc refuses it at parse with "expected `do`, found Colon". Descending iteration is a negative stride; there is no `downto` or `step` keyword. Eight sites in five files is the whole corpus.

```fortress
eqToRange(a[#], 0:10)      (* partial ranges: the missing end comes from the collection *)
eqToRange(a[3:], 3:10)
eqToRange(a[3#], 3:10)
eqToRange(a[#3], 0#3)      (* first three *)
eqToRange(a[:3], 0:3)
assert(a[4#],"o ")         (* the same forms on a String *)
```
*Seen in: ProjectFortress/tests/subArray.fss:52-56, ProjectFortress/tests/stringJuxt.fss:61-63*

Partial ranges are almost entirely array and string slices [legacy]; `for i <- 0# do` is a parse error in fortressc. `::` is the strided-with-missing-info factory: `A::` == `A:` == `A#`, and `::S` takes every Sth element. A long comment enumerates exactly which combinations of `:`, `#` and `::` the parser accepts and which it rejects, worth reading before inventing a form.

*Seen in: Library/FortressLibrary.fss:3900-3938*

### seq

```fortress
for i <- seq(0#5000) do              (* seq(): natural order, one thread *)
   println(i)
end
for i0 <- sequential(0 # |mem|) do   (* header only: `sequential` is the same call *)
```
*Seen in: fortressc/tests/parallelseq.fss:9-11, Library/Sparse.fss:55, Library/FortressLibrary.fss:1242*

`seq` is the only way to make a Fortress loop sequential [fortressc], and it dominates `sequential` 70 to 1 (630 uses in 123 files against 9 in 7). fortressc special-cases the token `seq` followed by `(` inside a for header; `sequential(0#3)` [legacy] and a free `g = seq(0#3)` are both parse errors there.

### Clause lists: more generators, guards, binders

```fortress
for i <- 0:7, j <- 0:7 do        (* comma: cross product, leftmost clause outermost *)
    testIntAverage(i, j)
end

for i <- seq(a.indices), i > 0, a[i-1]=/=a[i] do   (* a bare Boolean clause is a guard *)
    a[j] := a[i]
    j += 1
end
```
*Seen in: ProjectFortress/library_tests/AverageTest.fss:106-107, Library/Set.fss:88-91*

115 multi-generator headers in 39 files, 25 headers carrying at least one guard in 11 files. A later clause may depend on an earlier binder, and fortressc stops at the comma for both: "expected `do`, found Comma" [legacy]. The guard desugaring is stated in the library itself: `BIG OP [ xs <- g, p(xs) ] expr = BIG OP [ xs <- g.filter(p) ] expr`.

*Seen in: Library/FortressLibrary.fss:1131*

```fortress
for (i,ix) <- xs.indexValuePairs do    (* a parenthesised binder destructures the tuples *)
  a[i] := ix
end
for (_, entries) <- seq(database), entry <- seq(entries) do   (* `_` in any tuple position *)
for _ <- 0 : |elts| do                 (* bare `_`: repeat, element discarded *)
    k = random |elts|
```
*Seen in: Library/SetClosure.fss:39-41, ProjectFortress/demos/BirdCount1w.fss:203, Library/Avl.fss:360-361*

482 tuple-binder sites in 86 files, against only 18 bare `_` sites in 16. fortressc accepts a bare `_` [fortressc] and refuses tuple binders entirely, "expected a loop variable, found LParen", so it accepts no `_` inside a tuple either.

```fortress
for n <- <|3,5,7|> do                (* nesting is plain lexical containment *)
    for t <- 0#(2 n) do              (* the inner bound may use the outer binder *)
        simpleInsDel( <|[\RR64\] random(n) | i <- 0#n |> )
    end
end
```
*Seen in: Library/Avl.fss:420-424*

Nesting fixes the traversal order of the outer loop; a single comma-separated header does not, which is the only difference between them.

### What can be a generator

```fortress
(* header shapes; the generator is everything after the arrow *)
for i <- elts do                            (* a List value is a generator directly *)
for y<-{x,-x} do                            (* a set literal *)
for (k,v) <- seq(children()) do             (* a Map yields (key,value) pairs *)
for i <- a.bounds do                        (* .bounds: an Indexed's own index range *)
for (position,t) <- mem.indexValuePairs do  (* .indexValuePairs: index/element pairs *)
for (i,j) <- b.indices do                   (* .indices of a 2D array yields tuples *)
```
*Seen in: Library/Avl.fss:355-356, Library/PrefixSet.fss:134, ProjectFortress/tests/BadBounds.fss:29-31*

No `.iterator()` call, no method: lists, sets, arrays, maps, streams and literals are themselves generators, and a String yields Chars. Maps have NO `.keys` or `.values` generator anywhere in the corpus, they generate pairs and you destructure. `.indices` appears in 50 generator clauses across 23 files, `.indexValuePairs` in 60 uses across 20. All [legacy]: fortressc rejects any generator that is not a literal `lo:hi` or `lo#n` range, "expected `:` or `#` to close the generator range".

```fortress
for l <- self.left, r <- self.right do   (* a Maybe generates 0 or 1 elements, so this is a guard too *)
    if (l-r) MOD str =/= 0 AND NOT self.isEmpty then

for tfl <- tf.left, ofl <- otf.left, tfl > ofl, NOT tf.isEmpty, NOT otf.isEmpty do
    errorPrintln(other " left outside bounds " this)
    throw IndexOutOfBounds[\I\](this,ofl)
end
```
*Seen in: Library/RangeInternals.fss:157-158, Library/RangeInternals.fss:131-134*

The same arrow binds in a condition, which reads as "take this branch if the generator produced something" [legacy]. 281 sites in 63 files, so it is a staple and not a corner; `if` itself belongs to the control-flow section.

```fortress
(* header shapes, from three different files *)
if m <- mv then                                 (* then-branch only when mv holds; m names the contents *)
elif width <- params.widthRight then            (* the same form on an elif *)
if (k,v,delmin) <- left.extractMinimum() then   (* tuple binder in condition position *)
```
*Seen in: Library/Map.fss:379, Library/Format.fss:246-248, Library/Map.fss:319*

### Getting a value out

```fortress
mySum(i:ZZ64):ZZ64 = do
  acc:ZZ64 := 0
  for j <- 0:i do
    acc := acc + j
  end
  acc            (* the loop is a statement; the block's last expression is the value *)
end
```
*Seen in: SpecData/examples/basic/Expr.Do.mySum.fss:19-25, fortressc/tests/badparallelescape.fss:9-13*

A for loop evaluates to `()`, and fortressc says so directly: "`()` has no value, so it cannot be stored in a binding". Every value-producing loop in the corpus uses this accumulator shape, and badparallelescape.fss is that same shape as a NEGATIVE test: the write escapes the loop scope. If you want a value out of an iteration itself, use a comprehension or a BIG reduction, which is the whole reason they exist. Early exit is `label`/`exit with`, owned by the control-flow section.

```fortress
a.init0(i,g.get(i)), i <- a.indices          (* expression, comma, clauses: a loop with no for/do/end *)
s += x + y, x <- seq(1#10), y <- seq(1#x)    (* two clauses; the `<-` is what marks it a loop *)
result.init(i, r DOT other), (i,r) <- rows.indexValuePairs
```
*Seen in: Library/Set.fss:81, ProjectFortress/tests/simpleBig.fss:62, Library/Sparse.fss:141*

44 sites in 22 files, not a curiosity, and heavily used for array initialisation where the parallel semantics are the point. Easy to misread as a tuple. fortressc: "expected a newline or `;`, found Comma" [legacy].

### Comprehensions

All [legacy]: fortressc has no set type at all and rejects both enclosers, "expected an expression, found LeftBar" and "expected an expression, found LBrace".  ⚠ 2026-08-23: still refused, but the messages moved -- both enclosers PARSE now (`opr` declarations landed) and come back as `` unknown name `<|_|>` `` and `` unknown name `{_}` ``, which names the missing declaration rather than the bracket.  ⚠ 2026-08-23 (later): AND A COMPREHENSION PARSES in every bracket. `<| e | x <- g, p |>` and `{ e | x <- g }` are ONE production in 1.0 (`DelimitedExpr.rats:290-314`), so the bracket pair is carried as the operator name and nothing is list-specific. Static arguments go INSIDE the opener -- `<|[\E\] e | ... |>`. A guard is a generator clause with NO binder, which is 1.0's own representation. The separator is a bare `|` with WHITESPACE ON BOTH SIDES (`wr bar wr`); `<|x|x<-s|>` does not parse in 1.0 either. The LOWERING is refused by name: a comprehension accumulates an unknown number of elements and nothing in this backend grows storage.

```fortress
<| x^2 | x <- {0, 1, 2, 3, 4, 5}, x MOD 2 = 0|>   (* List: element expr, bar, the clause list a for header takes *)
p8 : List[\ZZ32\] = <|[\ZZ32\] x + y | x <- 1#10, y<-1#x |>   (* [\T\] right after the bracket fixes the element type *)
l = <|0, 1, 2, 3, 4|>                            (* no bar: a plain list literal *)
getter id(): List[\T\] = <| |>                   (* the empty list, here as a reduction identity *)
```
*Seen in: SpecData/examples/basic/Expr.ListComp.fss:22, ProjectFortress/tests/simpleBig.fss:25, ProjectFortress/BirdyLib/BigOpTests.fss:24*

341 comprehensions in 77 files. The static argument's placement is fixed: immediately after the opening bracket, before the element expression. Most Library comprehensions carry one, demo code usually omits it.

```fortress
{ x^2 | x <- {0, 1, 2, 3, 4, 5}, x MOD 2 = 0}            (* Set *)
tripsTo(trips, max:ZZ32) = { t | t<-trips, t.a <= max, t.b <= max, t.c <= max }
{ x^2 |-> x^3 | x <- {0, 1, 2, 3, 4, 5}, x MOD 2 = 0}    (* Map: the element is a k |-> v maplet *)
{[\Val\] k |-> v | (k,v) <- kvs }                        (* typed: two static args, key then value *)
```
*Seen in: SpecData/examples/basic/Expr.SetComp.fss:21, ProjectFortress/demos/trips.fss:234, Library/IntMap.fss:680*

88 brace comprehensions in 48 files, 51 of them the map form. `{ 1, 2, 3 }` with no bar is a set literal.

```fortress
<|[\List[\ZZ32\]\] <| x, y |> | x <- l1, y <- l1 |>   (* comprehensions nest *)
{x^2 | x <- {i | i <- 0#100}}
```
*Seen in: ProjectFortress/BirdyLib/BigOpTests.fss:58, ProjectFortress/not_passing_yet/desugarBug0.fss:34*

```fortress
words = ⟨"The", "quick", "brown", "fox"⟩              (* ⟨ ⟩ is the Unicode <| |> *)
for i ← ⟨ 0, 5, 7, 8, 9, 36, 23, 35 ⟩ do             (* a literal used straight as a generator *)
split(): Generator⟦String⟧ =  ⟨⟦String⟧ piece | (_, piece) ← self.splitWithOffsets() ⟩
```
*Seen in: ProjectFortress/tests/StringTests.fss:44, ProjectFortress/tests/StringTests.fss:34, Library/String.fss:157*

⟦ ⟧ is the Unicode spelling of `[\ \]`. 79 occurrences in 7 files, ASCII wins about 260 to 1, and a file that reaches for the Unicode form uses it for nearly everything. None of it compiles.

There is a fourth encloser: square brackets build an ARRAY comprehension, and its element expression is an `index |-> value` maplet rather than a bare value, because an array has to be told where each element goes.

```fortress
a : ZZ32[17] = [ i |-> i | i <- 0#17 ]   (* index expression, |->, value, then the clause list *)

array_comp_big = [ (x,y,1) |-> 0.0 | x <- 1#xSize, y <- 1#ySize
                   (1,y,z) |-> 0.0 | y <- 1#ySize, z <- 2#zSize
                   (x,1,z) |-> 0.0 | x <- 2#xSize, z <- 2#zSize
                   (x,y,z) |-> x + y z | x <- 2#xSize,
                                       y <- 2#ySize,
                                       z <- 2#zSize ]
(* several maplet-and-clause GROUPS stack, one per line and no separator between them, each
   filling a different slab of the same array; a TUPLE index is what makes it multi dimensional *)
```
*Seen in: ProjectFortress/not_passing_yet/arrayComp.fss:16, ProjectFortress/not_passing_yet/comprehensions.fss:30-35*

Two sites in the whole corpus, both under not_passing_yet, so the legacy compiler never took it either [legacy]. fortressc stops at the maplet, "expected `]`, found Bar". It is the only comprehension whose brackets are also the array literal's brackets, which is exactly why it needs the `|->`.

### Reductions

```fortress
p1 = BIG STAR [x<-0#10] 2 x + 1        (* BIG OP [clauses] body: reduce the body over every binding *)
factorial(n) = PROD[i <- 1:n] i        (* the common reductions have bare names and need no BIG *)
e_a = SUM [(i,n) <- a.indexValuePairs, a.isSet(i)] chki("bi",i,n,123)   (* guards, same as a for header *)
BIG SQCUP[\Type\] [t' <- gent.typeExtends] findGenType(t')   (* static type arg before the clause bracket *)
```
*Seen in: ProjectFortress/tests/simpleBig.fss:45, SpecData/examples/preliminaries/Overview.Expression.big.fss:18, Library/ChunkedSparseArray.fss:163*

603 bracketed-clause sites in 100 files, 215 of them the bare SUM/PROD/MAX/MIN spellings. Whitespace between the operator and `[` is optional. `BIG` is one of the 66 words fortressc lexes as Reserved and then rejects [legacy].

```fortress
p2 = BIG STAR (0#10)          (* no clause bracket: reduce the generator's own elements *)
mx = BIG MAX histogram        (* BIG Op Expr === BIG Op [x <- Expr] x *)
geometricMean(xs: List[\RR64\]): RR64 = (PROD xs)^(1.0/(|xs|))   (* juxtaposition, so the parens matter *)
```
*Seen in: ProjectFortress/tests/simpleBig.fss:49-51, ProjectFortress/tests/zeno.fss:94, Library/Pairs.fss:101*

```fortress
p6 : List[\ZZ32\] = BIG <|[\ZZ32\] x + y | x <- 1#10, y<-1#x |>   (* BIG on the comprehension's own encloser *)
readReferenceFile(name: String): String = BIG || <| line | line<-FileReadStream(name).lines(), line[0] =/= '>' |>
"[" self.bounds "] = [" (BIG ||[(i,v) <- self.indexValuePairs] " " i "|->" v) " ]"
```
*Seen in: ProjectFortress/tests/simpleBig.fss:29, ProjectFortress/demos/BirdCount1u.fss:51, Library/ChunkedSparseArray.fss:73*

`BIG ||`, concatenation, is the most-used BIG operator in real code, 133 sites against UNIONCAT's 89. The full declared vocabulary is `||` `|||` `//` AND OR MAX MIN MAXN MINN MINMAX MINMAXN MAX_MAX MAX_MIN MIN_MAX MIN_MIN MAXNUM MINNUM BITXOR CONCAT LEXICO UNION INTERSECTION SYMDIFF UPLUS SQCAP SQCUP BOXPLUS UNIONCAT UNIONPLUS RELATION, plus the encloser forms `<| |>`, `{ }`, `{|-> }`, `{/ /}` and `{/|-> /}`. One Unicode spelling occurs in the corpus, `BIG ∨` for BIG OR, a single use.

*Seen in: Library/String.fss:414*

### Declaring your own

```fortress
opr BIG STAR[\T extends Number\](): Comprehension[\T,Number,Number,Number\] =
    Comprehension[\T,Number,Number,Number\](fn x => x, SumReduction, cast[\Number\])

opr BIG STAR[\T extends Number\](g: Generator[\T\]): T =
    __bigOperatorSugar[\T,Number,Number,Number\](BIG STAR[\T\](), g)
```
*Seen in: ProjectFortress/tests/simpleBig.fss:16-20*

A fixed pair: the nullary declaration gives the reduction object, the unary one takes the generator. 35 distinct `opr BIG` names across Library/, and the comprehension enclosers are declared the same way, which is why the syntax is open-ended.

```fortress
opr BIG <|[\T\] g:Generator[\T\]|>:List[\T\] =                        (* the list encloser itself *)
    __bigOperatorSugar[\T,List[\T\],AnyCovColl,AnyCovColl\](BIG <|[\T\]|>(), g)
opr BIG {|->[\Key,Val\] g: Generator[\(Key,Val)\] }:Map[\Key,Val\] =  (* the map encloser *)
    __bigOperatorSugar[\(Key,Val),Map[\Key,Val\],AnyCovColl,AnyCovColl\](BIG {|->[\Key,Val\]}(), g)
opr <|[\E\] xs: E... |>: List[\E\] = list(xs)                         (* the literal, a varargs bracketing operator *)
```
*Seen in: Library/List.fss:177-180, Library/Map.fss:194-195, Library/List.fss:176*

`{/ /}` for PrefixSet and `{/|-> /}` for PrefixMap exist only because someone declared them. Ten sites in four files, and only two are live use: a comprehension in ProjectFortress/tests/parametricManiaCompr.fss:30 and a call in ProjectFortress/tests/Brackets.fss:59, which declares its own `{/ /}`. `{/|-> /}` is declarations only.

```fortress
l4 = <|[\ZZ32\] x + x | x <- l1.filter(fn y => y =/= 2) |>       (* .filter replaces a guard clause *)
l8 = <|[\ZZ32\] y | y <- l1.nest(f) |>                           (* .nest is `x <- l1, y <- f(x)` *)
l10 = <|[\ZZ32\] x + y + z | (x,y) <- l1.cross(l1) , z <- l1 |>  (* .cross yields pairs *)
```
*Seen in: ProjectFortress/BirdyLib/BigOpTests.fss:55, ProjectFortress/BirdyLib/BigOpTests.fss:67, ProjectFortress/BirdyLib/BigOpTests.fss:73*

Every generator carries `.map`, `.filter`, `.nest`, `.cross`, `.zip`, `.seq` and `.reverse`, and the clause-list desugaring is literally defined in terms of them: `BIG OP [gs_1, gs_2] e = BIG OP [gs_1] (BIG OP [gs_2] e)` plus filter-squeezing. `.cross` deliberately does not promise left-to-right order unless one side is `seq`.

```fortress
object BlockedRange(lo: ZZ64, hi: ZZ64, b: ZZ64) extends Generator[\ZZ64\]
  size : ZZ64 = hi - lo + 1
  seq(self): SequentialGenerator[\ZZ64\] = seq(lo:hi)
  generate[\R\](reduction: Reduction[\R\], body: ZZ64->R): R =

f() = <| 2 x | x <- BlockedRange(1,10,3) |>   (* used exactly like a built-in generator *)
```
*Seen in: SpecData/examples/advanced/Generators.GeneratorDefn.fss:19-22, SpecData/examples/advanced/Generators.GeneratorDefn.fss:41*

`generate` is the minimal complete definition, and a for loop is a `generate` over the void reduction.

```fortress
loop(f:E->()): () = generate[\()\](VoidReduction, f)   (* what a `for` desugars to *)
```
*Seen in: Library/FortressLibrary.fss:1052*

## 11. Parallelism and atomicity

Fortress is parallel by default: unless a generator says otherwise, every call of a `for` body may happen in parallel, and there is no keyword to ask for it. Almost everything in this section is [legacy]; the Rust rewrite implements exactly one parallel loop form and refuses the rest by reserved word, so only the exceptions are tagged.

### Loops are already parallel

```fortress
for i <- 1#1000 do                    (* every iteration may run in parallel; that is the default *)
                                      (* `←` U+2190 also spells `<-`, 32 uses in the corpus against 2384 *)
   atomic do count := count + 1 end   (* so a shared read-modify-write has to be a transaction *)
end

for i <- 0#8, j <- 0#8 do end         (* comma-separated generators: a cross product, parallel in both *)
```

*Seen in: ProjectFortress/other_compiler_tests/atomic3.fss:19-21, ProjectFortress/demos/mm.fss:17, Library/GeneratorLibrary.fss:47-49*

The Rust compiler implements one parallel shape: a ZZ64 binder over a `#`- or `:`-range with a void body. [fortressc]

```fortress
a: Array[\ZZ64\] = array(n)
for i <- 0#n do        (* body is outlined to $loopN(i64 index, ptr env); captures go in one *)
   a[i] := i i + 7     (* scanned environment struct allocated once before the loop *)
end
                       (* pool is min(nproc,16), built on first use, and the calling thread takes chunk 0 *)
                       (* ranges under FORTRESS_PARALLEL_MIN = 4096 run inline and never touch the pool *)
```

*Seen in: fortressc/tests/parallelfill.fss:13-16, fortressc/tests/parallelalloc.fss:10-12, fortressc/runtime/shims.c:81*

### Opting out with seq

```fortress
for i <- seq(0#n) do arr[i] := random() end   (* seq forces one thread, in the generator's own order *)

for k <- seq(0#n-1) do                        (* the usual mixed idiom: outer sequential ... *)
  rest = k+1 # n-k-1
  t:(RR64,ZZ32) := (|A[k,k]|,k)
  for ii <- rest do                           (* ... inner left parallel, with an atomic reduction *)
    atomic t MAXMIN= (|A[ii,k]|,ii)
  end
end

for (_, entries) <- seq(database), entry <- seq(entries) do entry.printEvent() end  (* wrap each generator *)
```

*Seen in: Library/Random.fss:48, ProjectFortress/demos/lutx.fss:63-68, ProjectFortress/demos/BirdCount1u.fss:240*

In fortressc `seq` is not a function: the parser recognises the literal word before `(` in generator position only, so `g = seq(0#10)` gives ``expected `)`, found Hash``. [fortressc]

```fortress
for i <- seq(0#5000) do    (* recognised and marked sequential; 5000 is deliberately over the threshold *)
   println(i)
end
                           (* FIXED 2026-08-23. This block used to record a reproducible BUG: *)
                           (* a seq loop assigning to a mutable declared OUTSIDE it passed the *)
                           (* checker and died in codegen with `internal error: `total` was  *)
                           (* assigned to but has no storage`. The same program prints 45 now. *)
```

*Seen in: fortressc/tests/parallelseq.fss:9-11, fortressc/crates/parser/src/lib.rs:1069-1073, fortressc/crates/types/src/lib.rs:90-92*

Every library generator also carries `seq` as a method, declared two ways: the functional `seq(self)` in 18 files and the dotted `seq()` in 12. [parses]

```fortress
seq(self): SequentialGenerator[\E\] = NaiveSeqGenerator[\E\](self)  (* functional method, so the call is seq(g) *)
seq(): SequentialGenerator[\E1\]                                    (* dotted method, so the call is g.seq() *)

for (_, entries) <- database.seq(), entry <- entries.seq() do println(entry.asDetailedString) end
                                    (* seq(g) beats g.seq() about 11 to 1, 484 call sites against 44 *)
```

*Seen in: Library/FortressLibrary.fss:990, Library/GeneratorLibrary.fss:74, ProjectFortress/BirdyLib/TestGU1.fss:268*

```fortress
sequential[\T\](g:Generator[\T\]):SequentialGenerator[\T\] = seq(g)  (* the long name for seq, a plain *)
                                                                     (* function and not a keyword *)
args_v = <|[\Expr\] arg.eval(CT_, theta) | arg <- sequential(args) |>  (* one of 8 call sites in 6 files *)
```

*Seen in: Library/FortressLibrary.fss:1242, ProjectFortress/demos/FeatherweightJava.fss:128*

### atomic

```fortress
atomic do count := count + 1 end                  (* one transaction: no thread sees an intermediate state, *)
                                                  (* and the block may be rolled back and retried *)
atomic do count1+= 1; count2+=2; count3+=3 end    (* several updates made atomic together, split on `;` *)

atomic true                                       (* no `do` needed: atomic takes any expression, *)
                                                  (* and the result has that expression's type *)
atomic if canTryIt then canTryIt := false; true else false end   (* an atomic test-and-set *)
```

*Seen in: ProjectFortress/other_compiler_tests/atomic3.fss:19-21, ProjectFortress/other_compiler_tests/atomic5.fss:22, Library/OneShotFlag.fss:18*

Fortress has no declared reduction-variable syntax. Accumulating from a parallel loop is this idiom or a BIG operator.

```fortress
sum: ZZ32 := 0
accumArray[\N extends Number, nat x\](a: Array1[\N,0,x\]): () =
  for i <- a.indices do
    atomic sum += a[i]        (* an atomic update of a mutable declared outside the loop *)
  end

for i <- 1:n do atomic result := result i end   (* the same shape with plain assignment *)
```

*Seen in: SpecData/examples/basic/Expr.Atomic.fss:19-23, SpecData/examples/preliminaries/Overview.Expression.alsodo.fss:31*

```fortress
atomic do
        count := count + 1
        atomic do
           count := count + 1    (* nesting joins the outer transaction rather than starting a second one; *)
        end                      (* the test asserts count=2, so it is neither skipped nor double-counted *)
 end
```

*Seen in: ProjectFortress/other_compiler_tests/nestedTransactions0.fss:18-23, ProjectFortress/other_compiler_tests/nestedTransactions1.fss:20-26*

Two declaration modifiers, both rare enough to be curiosities rather than style: `io` runs to 7 uses in 2 files, both of them parser tests.

```fortress
trait T
  io atomic io f():()   (* atomic in modifier position makes every call a transaction. ONE use in the *)
end                     (* whole corpus, and it is a parser test repeating `io` on both sides *)

a: io ZZ32->String              (* io marks a declaration or arrow type as doing I/O, which stops the *)
e: io ZZ32->String throws Error (* implementation moving or duplicating it *)
g: ZZ32 -> (io String -> ())    (* it binds to the arrow, including a nested one *)
```

*Seen in: ProjectFortress/parser_tests/XXXMultipleModifiers.fss:15-17, ProjectFortress/parser_tests/ioTests.fss:18-21*

### tryatomic and abort

```fortress
for i ← 0#iters do
    try
        tryatomic do successes+= 1 end    (* like atomic, but a conflict or abort raises TryAtomicFailure *)
    catch e                               (* instead of retrying silently, so the caller decides *)
        TryAtomicFailure ⇒ atomic do failures+= 1 end
    end
end

a:= tryatomic foo        (* bare expression form, no `do` *)
tryatomic (spawn ())     (* negative test: like atomic, a tryatomic may not contain a spawn *)
```

*Seen in: ProjectFortress/tests/tryatomicTest.fss:19-25, ProjectFortress/compiler_tests/Compiled6.aw.fss:16, ProjectFortress/compiler_tests/Compiled1.ap.fss:15*

```fortress
abort():() = builtinPrimitive("com.sun.fortress.interpreter.glue.prim.Thread$abort")
                                (* a library FUNCTION, not a keyword: it aborts the innermost *)
atomic do                       (* enclosing transaction. Under atomic the block retries; *)
    old = s                     (* under tryatomic it surfaces as TryAtomicFailure *)
    if NOT old.isDone() then
        if old.isPending() then abort() end
    end
end
```

*Seen in: ProjectFortress/LibraryBuiltin/FortressBuiltin.fss:699, Library/Lazy.fss:60-63, ProjectFortress/tests/abortBlock.fss:20-23*

### Parallel blocks: do ... also do ... end

```fortress
do
  quicksort(lt, arr, left, pivotNewIndex-1)
also do                                        (* the clauses run in parallel and the group finishes *)
  quicksort(lt, arr, pivotNewIndex+1, right)   (* before execution continues *)
end                                            (* exactly ONE end for the whole group, not one per clause *)

do
  accum += treeSum(t.left)
also do
  accum += treeSum(t.right)
also do
  accum += t.datum      (* also chains rather than nests *)
end

run() = do 3 also do 5 end   (* the group is an expression and can be typed *)
```

*Seen in: Library/QuickSort.fss:40-44, SpecData/examples/basic/Expr.Do.treeSum.fss:26-32, ProjectFortress/compiler_tests/Compiled10.a.fss:14*

The modifier attaches to the clause, not to the group, and clauses may differ.

```fortress
atomic do
  x += 1
  y += 2
also atomic do    (* each clause repeats its own modifier; both of these are transactions *)
  b += 1
  y += 3
end

atomic do
    x += 1
    x += 1
also do
    z := x        (* mixed: this clause is not atomic, and the test asserts it can never *)
end               (* observe the half-done state *)
```

*Seen in: Documentation/Specification/Code/DoAbbrev1.fss:25-31, ProjectFortress/tests/AlsoDo.fss:35-41, SpecData/examples/preliminaries/Overview.Expression.Also.b.fss:24-29*

### spawn and Thread

```fortress
var x:ZZ32 = 0
pt:Thread[\Any\] = spawn do x:=1 end     (* runs the body in a new thread, returns a handle at once *)
pt.wait()                                (* block until it completes *)
pt.stop()                                (* terminate it *)

ft:Thread[\Any\] = spawn(makeWork(10))   (* spawn applied to a call *)
ft.val()                                 (* blocks and yields the result *)
ft.ready()                               (* the non-blocking poll *)

ft:Thread[\Any\] = Thread[\Any\](fn()=>do x:=1 end)   (* spawn e is sugar for this *)
```

Those four methods are the whole API: no `join`, no `start`, no priority.

*Seen in: ProjectFortress/tests/Spawn1.fss:16-20, ProjectFortress/tests/Spawn5.fss:24-25, ProjectFortress/LibraryBuiltin/FortressBuiltin.fss:688-696*

```fortress
pt:Thread[\Any\] = spawn atomic do  x:=1; y := 1 end   (* the new thread's whole body is one transaction, *)
                                                       (* so both writes become visible together *)
                                                       (* the asymmetry is deliberate: spawn atomic is legal, *)
                                                       (* atomic (spawn ...) is not *)
s = spawn
      for i <- a do        (* spawn <expr> where the expression is a whole loop, with no `do` of its own *)
        prod1 TIMES= i
      end
```

*Seen in: ProjectFortress/tests/Spawn2.fss:18-21, ProjectFortress/compiler_tests/Compiled160.fss:22-25, ProjectFortress/compiler_tests/Compiled9.i.fss:18-20*

### at, regions and sharing

```fortress
do
  v := a[i]
  at a.region(j) do    (* evaluate the body in region j's region instead of here *)
    w := a[j]
  end
  x = v + w
end

at region(v) do
    println(v " at " region(v) "; here = " here())   (* here() reports the current region *)
end

at (throw NotFound) do                 (* the region expression is evaluated first: *)
    fail("Running block of flaky region computation!")   (* if it throws, the body never runs *)
end
```

*Seen in: SpecData/examples/advanced/Parallel.At.d.fss:24-30, ProjectFortress/tests/Region.fss:31-33, ProjectFortress/tests/Region.fss:35-41*

Regions are library values, not keywords, and in this implementation they are a stub. [parses]

```fortress
region(a:Any): Region = Global   (* every region query answers Global, so `at` placement is a no-op here: *)
here(): Region = Global          (* the syntax is real, the locality is not *)

shared[\T extends Any\](x:T): T = x   (* shared / isShared / localize are identity stubs. `shared` is NOT *)
isShared(x:Any): Boolean = true       (* reserved, so `y := shared Cons(x, xs)` is ordinary juxtaposition *)
localize[\T extends Any\](x:T): T = x
```

*Seen in: Library/FortressLibrary.fss:85-90, Library/FortressLibrary.fss:65-69, SpecData/examples/advanced/Parallel.Shared.a.fss:26-27*

```fortress
s = spawn at a.region(i) do   (* place the new thread as well as start it *)
          a[i]
    end
t = spawn at region(s) do     (* region(s) on a thread handle: a spawned thread is itself a located value *)
          a[j]
    end

do
  v := a[i]
also at a.region(j) do        (* an also clause can carry a placement instead of atomic *)
  w := a[j]
end
```

*Seen in: SpecData/examples/advanced/Parallel.At.c.fss:22-27, SpecData/examples/advanced/Parallel.At.b.fss:24-28*

A `for` header takes the same modifiers between the generator list and `do`, with no punctuation.

```fortress
for a <- b at Global do
    (println "Testing at")
end
for c ← chars atomic do result := result.add(c) end  (* each ITERATION is a transaction, which is not the *)
                                                     (* same as wrapping the whole loop in one atomic *)

run() = at GlobalRegion atomic println "first"       (* the do-front with no do/end at all, and it still *)
run() = at GlobalRegion println "first" also atomic println "second" end   (* takes `also`. 3 parser tests in all, *)
                                                     (* not idiomatic: everything else uses do ... end *)
```

*Seen in: ProjectFortress/syntax_abstraction_tests/ForUse.fss:44-50, ProjectFortress/tests/StringTests.fss:245, ProjectFortress/parser_tests/XXXPreparser.ak.fss:15, ProjectFortress/parser_tests/XXXPreparser.aj.fss:15*

```fortress
(v, w) := (a[i],
           at a.region(j) do   (* tuple components are evaluated in parallel, so one component can sit *)
              a[j]             (* in another region while the other runs *)
           end)
```

*Seen in: SpecData/examples/advanced/Parallel.At.a.fss:24-27*

### Reductions: generate, Reduction objects and BIG

```fortress
object SumZZ32 extends CommutativeMonoidReduction[\ZZ32\]
    empty(): ZZ32 = 0
    join(a: ZZ32, b: ZZ32): ZZ32 = a+b   (* MonoidReduction needs associativity; the Commutative one also *)
end                                      (* lets the implementation reorder *)

z = (1#100).generate[\ZZ32\](SumZZ32, fn (x) => 3 x + 2)   (* the explicit protocol: a generator is free *)
y = SUM [x <- 1#100] 3 x + 2                               (* to split any way it likes. Same value. *)
```

*Seen in: SpecData/examples/advanced/Generators.ReductionClass.fss:18-24, SpecData/examples/advanced/Generators.ReductionClass.fss:28*

```fortress
loop(body :E1->()): () = generate[\()\](VoidReduction, body)  (* `for` desugars to this: reducing many *)
                                                              (* voids to one is what synchronises the loop *)
var res:dna  = BIG OPLUS [l<-rs.lines()] processLine(l)       (* a user-declared BIG operator, in parallel *)
fixedsize:ZZ32 = (if v then 1 else 0 end) + (SUM[(k,a) <- c] a.size)
                                          (* tuple binder in the clause; `SUM[` with no space is written too *)
```

fortressc has no Generator library, so a hand-written reduction parses and then the checker cannot resolve the trait names; `SUM [i <- 0#10] i` does not parse at all.

*Seen in: Library/GeneratorLibrary.fss:143, ProjectFortress/tests/FileConversion.fss:109, Library/PrefixSet.fss:374*

### What fortressc refuses

`atomic`, `tryatomic`, `spawn`, `at`, `also`, `io` and `BIG` are all reserved but rejected: "reserved word `X` is not in the implemented subset".  ⚠ 2026-08-23: `atomic`, `spawn` and `also` ALL WORK now, and so does an ARRAY GENERATOR -- `for x <- a` runs, so the `expected `:` or `#`` message below is gone; `for ch <- "abc"` answers `expected an array, found String`. A parallel loop body still may not assign to an outer mutable, and the message now adds `` Write `for ... <- seq(...)` for a sequential loop ``; the two hatches beside it are a REDUCTION VARIABLE (`s += i`) and an `atomic` block. `tryatomic`, `at`, `io` and `BIG` are still refused. `region`, `shared`, `abort` and `Thread` are library names, so they parse and come back as `unknown name` or `unknown type`. Also out of the M4 subset, and not one of them is named by its own diagnostic: a tuple binder gives `expected a loop variable, found LParen`, an array generator (`for x <- a`) gives ``expected `:` or `#` to close the generator range, found KwDo``, and a loop body with a value comes back as a plain mismatch against `()`, `expected (), found ZZ64` or `an integer literal cannot be used where () is required`. The compiler does carry a form string for the valued body, but the Void expectation is pushed into the body first and fails there, so nothing reaches the check that would print it.

```fortress
total: ZZ64 := 0
for i <- 0#1000 do
   total := total + i   (* REFUSED: "`total` is declared outside this loop, and a parallel loop body may *)
end                     (* not assign to it; iterations run in any order and on any thread." *)
                        (* It is one scope comparison, not dataflow analysis: a mutable the body *)
                        (* declares itself is fine, so per-iteration scratch is the workaround *)

a: Array[\ZZ64\] = array(1000)
for i <- 0#999 do
   a[i + 1] := i        (* REFUSED: "a parallel loop may only assign to `a[i]`, the element its own *)
end                     (* iteration owns". An array the body CREATED itself is fresh per iteration, *)
                        (* so any index into that is allowed *)
```

*Seen in: fortressc/tests/badparallelescape.fss:9-12, fortressc/tests/badparallelindex.fss:8-11, fortressc/crates/types/src/lib.rs:1826-1844*

## 12. Exceptions

The whole control-flow half of this family is [legacy]: `throw`, `try`, `catch`, `finally`, `forbid`, `throws`, `tryatomic` and `Zilch` are all reserved words.  ⚠ 2026-08-23: `throw` COMPILES AND RUNS [fortressc]. An uncaught throw HALTS -- there is no `catch` yet, so every throw is uncaught -- printing `fortress: uncaught exception <Name>` and exiting 1. No unwinding, no landing pad, and no cost on the path that does not throw. Two rules: 1.0's own, that the thrown value must be an `Exception` (`XXX9aa.test` records "`throw` can only throw objects of Exception type"), and that a throw takes the type its context wants, so `if c then x else throw E end` types. `try` PARSES as of later the same day -- body, `catch` binder and arms, `forbid` list and `finally` block, exactly `DelimitedExpr.rats:141-142` -- and is refused by name at the checker: its lowering is not built. `catch`, `finally` and `forbid` have no meaning outside a `try`. `tryatomic` and `Zilch` are still reserved and refused. In expression, statement or type position fortressc refuses one with `reserved word ... is not in the implemented subset`, and that covers seven of the eight, `Zilch` included. `throws` is the odd one out, because the corpus only ever writes it as a clause hanging off a type and the parser rejects the token before that check runs: `getMinimum(): ZZ32 throws NotFound = 1` gives ``expected `=`, found Reserved("throws")``, while a trait member or an arrow type gives ``expected a newline or `;`, found Reserved("throws")``, exactly the way `requires` behaves in section 14.  ⚠ 2026-08-23: A `throws` CLAUSE PARSES NOW: `f(): ZZ32 throws E = 1` compiles. `throw`, `try`, `catch` and the rest of the family are still refused by name. The declaration shapes below compile today.

### Declaring exceptions

```fortress
trait Exception comprises { UncheckedException, CheckedException }
end
trait UncheckedException extends Exception excludes CheckedException
end
trait CheckedException extends Exception excludes UncheckedException
end
```

The three-trait root. The shape compiles [fortressc], but the names themselves do not exist yet: `extends UncheckedException` gives `unknown type`.

*Seen in: Library/FortressLibrary.fss:1447-1453, Library/FortressLibrary.fss:1530-1531, Library/incomplete/basic/Fortress.Standard.fss:31-34*

```fortress
object Scooby extends CheckedException
    getter asString(): String = "ScoobyDoobyDoo"  (* the message; read back as x.asString *)
end
object BrokenInvariant extends UncheckedException
  getter asString() = "Broken Invariant"          (* return type inferred *)
end
object EmptyIntersection extends UncheckedException end   (* no message getter at all *)
object SizeMismatchException end     (* thrown with NO Exception ancestry; rare, but real library code *)
```

An exception is an ordinary `object`. There is no `exception` declaration keyword. The declaration and its getter compile [fortressc]; reading the getter back is [parses] only (`accessors parse but are not implemented`).

*Seen in: ProjectFortress/tests/Exception.fss:15-17, ProjectFortress/BirdyLib/PureList.fss:19-21, Library/Generator22D.fss:262*

```fortress
object TestFailCalled(s:String) extends UncheckedException
    getter asString(): String = s                (* constructor field as the payload *)
end
object ForbiddenException(chain : Exception) extends UncheckedException   (* what `forbid` produces *)
    getter asString(): String = "Forbidden exception"
end
object IndexOutOfBounds[\I\](range:Range[\I\],index:I) extends UncheckedException  (* static params *)
    getter asString(): String = index " is outside the range " range
end
object AtomicSpawnSynchronization extends {UncheckedException}   (* braced set; same meaning, 1 site *)
```

*Seen in: ProjectFortress/other_compiler_tests/ThrowTest1.fss:15-17, Library/FortressLibrary.fss:1504-1506, Library/FortressLibrary.fss:1571-1573*

### Throwing

```fortress
throw Scooby                                   (* bare singleton object - the dominant form *)
throw FailCalled(s)                            (* constructor call, exception carries a payload *)
throw IndexOutOfBounds[\ZZ32\](self.bounds,i)  (* generic: static args, then value args *)
```

The operand is always juxtaposed after the keyword. There is no bare `throw` re-throw and no parenthesised `throw (...)` anywhere in the corpus.

*Seen in: ProjectFortress/tests/Exception.fss:26-28, Library/CompilerLibrary.fss:73-76, Library/FortressLibrary.fss:3986*

```fortress
var a : ZZ32 = throw FooExn        (* You can assign throw to any type *)
else => throw CastError            (* fits a typecase arm; bottom agrees with every other arm *)
fn () => throw NotFound            (* first-class failure continuation, the library's usual idiom *)
```

`throw` has the bottom type, so it stands wherever any type is expected.

*Seen in: ProjectFortress/compiler_tests/Compiled9.ab.fss:18-21, Library/FortressLibrary.fss:33-37, Library/FortressLibrary.fss:1205*

### try / catch

```fortress
try
    throw Scooby
    x := 5              (* never runs *)
catch e                 (* ONE binder for the whole clause; e is the thrown value *)
    Shaggy => x := 7    (* arms are bare TYPES, not patterns; first match wins *)
    Scooby => x := 9    (* so x ends as 9 *)
end
```

The binder is mandatory: all 89 catch sites name one. There is no typed arm binder (`e: T =>`) and no `else =>` default arm in any catch in the corpus.

*Seen in: ProjectFortress/tests/Exception.fss:26-32, ProjectFortress/other_compiler_tests/Exception.fss:24-30*

```fortress
catch e IndexOutOfBounds[\ZZ32\] =>    (* arm on the catch line; arm type takes static args *)
    assert(shouldThrow, true, "Spurious IndexOutOfBounds on ",i)
catch x EmptyReduction => ()           (* whole clause compressed onto one line *)
catch x
    KeyOverlap[\String,ZZ32\] => ()    (* multi-argument generic as the arm label *)
```

Whitespace and indentation inside a try are free; only the keyword order is fixed.

*Seen in: ProjectFortress/tests/RangePrototype.fss:626-628, ProjectFortress/tests/simpleSum.fss:59, ProjectFortress/tests/MapTest.fss:77-78*

```fortress
IllegalMove => do                      (* do ... end body; its `end` is not the try's *)
    println "You cannot move there. Choose again."
    play(board)
end
TryAtomicFailure => printThreadInfo("Caught") ; atomic do failures+= 1 end  (* ; sequences two exprs *)
FailCalled =>
    fail(x.s " generated by " elts[ # |r_new| ])   (* hanging indent; x.s reads a field off the binder *)
```

*Seen in: ProjectFortress/demos/tictactoe.fss:246-251, ProjectFortress/tests/abortTest.fss:26, Library/Avl.fss:346-348*

```fortress
TryAtomicFailure ⇒ atomic do failures+= 1 end   (* U+21D2 for `=>`; the only catch arm in the corpus written this way *)
```

*Seen in: ProjectFortress/tests/tryatomicTest.fss:20-24*

### finally and forbid

```fortress
try
    throw Scooby
catch f
    Scooby => x := 7
finally x := 9      (* runs after the body and any handler, matched or not; x ends at 9 *)
end
```

`finally` is always the last clause before `end`, and a try may carry it with no catch at all. 10 uses / 6 files.

*Seen in: ProjectFortress/tests/Exception.fss:75-80, ProjectFortress/tests/ExceptionScoping.fss:30-36, ProjectFortress/compiler_tests/Compiled5.at.fss:43-52*

```fortress
bar():ZZ32 = do
    try
        throw Scooby
        1
    catch e
        Scooby  => 5    (* this arm supplies the try's value *)
    finally 7           (* finally's value is DISCARDED: bar() = 5, not 7 *)
    end
end
```

*Seen in: ProjectFortress/tests/Exception.fss:85-94*

```fortress
try
  inp = read(file)
  write(inp, newFile)
forbid IOFailure      (* an IOFailure escaping the body becomes ForbiddenException wrapping it *)
end

catch e
    Scooby => x := 3               (* arms are tried BEFORE forbid, so this wins *)
forbid { Something, SomethingElse }  (* braced set: the corpus's only multi-type forbid *)
```

Rare: 5 code sites in 4 files. `forbid` always follows `catch` when both are present, and never coexists with `finally`.

*Seen in: SpecData/examples/basic/Expr.Try.a.fss:25-29, ProjectFortress/tests/Exception.fss:116-121, ProjectFortress/compiler_tests/Compiled9.ag.fss:22-26*

```fortress
try <body> [catch b <arms>] [forbid <set>] [finally <expr>] end
(* Fixed order. Every clause but the body is optional.
   No corpus try has two catch clauses, a finally before a catch, or forbid and finally together. *)
```

*Seen in: ProjectFortress/compiler_tests/Compiled9.af.fss:19-26, SpecData/examples/basic/Expr.Try.c.fss:27-35*

### try as an expression

```fortress
z := try
  throw FooExn
catch e
  FooExn => 5                     (* the try's value is the body's, or the matching arm's *)
  Exception => "This is a string"
end

run() = try                       (* a try can be the whole function body *)
    testFail("Fooey!")
  catch e
    TestFailCalled => println "PASS"
  end
```

*Seen in: ProjectFortress/compiler_tests/Compiled9.ad.fss:19-25, ProjectFortress/other_compiler_tests/ThrowTest1.fss:19-26, ProjectFortress/tests/objectCC_label.fss:182-185*

```fortress
try
    try
        Nothing[\ZZ32\].get     (* raises NotFound *)
    catch x
        FailCalled => fail("Caught FailCalled!!")   (* no arm matches *)
    finally
        finallyCount += 1       (* inner finally runs, then it propagates to the outer catch *)
    end
```

The binder may be reused at each nesting level; sibling `catch x` clauses do not collide.

*Seen in: ProjectFortress/tests/ExceptionScoping.fss:29-36, ProjectFortress/tests/Exception.fss:62-68*

### throws clauses

```fortress
getMinimum():F throws NotFound = do ... end        (* method: after the return type, before = *)
getter get(): E throws NotFound = ...              (* getter *)
opr[k:Key]: Val throws NotFound = mem(k).getVal()  (* operator / subscript; `opr [` also occurs *)
getVal():Val throws NotFound                       (* abstract trait method, no body *)
g: ZZ32 -> (ZZ32, ZZ32) throws IOFailure           (* arrow type: after the range *)
```

Every corpus `throws` names exactly one type. There is no comma-separated list and no `throws { A, B }` set form. 56 uses / 19 files.

*Seen in: Library/PrefixSet.fss:200-207, Library/Map.fss:41-43, SpecData/examples/basic/Types.Arrow.fss:23-24*

```fortress
(fn (arg:String):String throws Foo =>arg)("MyString")  (* on a fn expression: 1 site, a rejected test *)
object U throws Exn ensures { true } extends T end     (* object header: 1 parser test, not idiomatic *)
```

*Seen in: ProjectFortress/not_working_static_tests/DXXFnExpr2.fss:19, ProjectFortress/parser_tests/XXXobjectClauses.fss:15-17*

```fortress
fail(s: String): Zilch = do                (* Zilch = never returns; every path throws *)
    errorPrintln("FAIL: " s)
    throw FailCalled(s)
end
testFail(s:String): Zilch = throw TestFailCalled(s)
extract(self):Zilch throws NothingInHere = throw NothingInHere  (* Zilch and throws together *)
```

Rare, 4 sites in 4 files; the `ProjectFortress/BirdyLib/Maybe.fss` line above is the fourth, and it is live code: the block comment opened at `ProjectFortress/BirdyLib/Maybe.fss:29` closes on line 33, because the `(*)` inside it is inert. The same never-returning `fail` is also written `fail[\T\](s:String):T` with an inferred static parameter.

*Seen in: Library/CompilerLibrary.fss:73-76, ProjectFortress/other_compiler_tests/ThrowTest1.fss:19, ProjectFortress/BirdyLib/Maybe.fss:46-49*

### The standard names

```fortress
(* Unchecked *) NotFound  BrokenInvariant  DivisionByZero  IndexOutOfBounds[\I\]  EmptyReduction
                ForbiddenException(Exception)  FailCalled(String)  IntegerOverflow  InvalidRange
                NegativeLength  LabelException  CallerViolation  CalleeViolation  UnpastingError
(* Checked *)   CastError  IOFailure  MatchFailure  DisjointUnionError  TryAtomicFailure  APIMissing
```

Most-thrown of these: `BrokenInvariant` 51 (all of them in `ProjectFortress/BirdyLib/PureList.fss`, which is also the only file that declares it), `NotFound` 37, `ForbiddenException` 10, `FailCalled` 8. None of these names exist in fortressc. `Throwable` and `RuntimeException` have zero uses in the corpus; `Error` is not a root either, just a local `object Error` in 3 files.

*Seen in: Library/FortressLibrary.fss:1455-1568, ProjectFortress/BirdyLib/PureList.fss:19, ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:1515-1522*

### Transactions and testing

```fortress
try
    tryatomic do successes+= 1 end          (* an abort() inside raises the CHECKED TryAtomicFailure *)
catch e
    TryAtomicFailure => atomic do failures+= 1 end   (* handler body may itself be atomic *)
end
```

`tryatomic` is 6 uses in 6 files. Four of the six sit inside a try catching exactly `TryAtomicFailure`; the other two stand outside any try. Nothing in the corpus throws out of an `atomic` block, so it shows nothing about how a transaction unwinds.

*Seen in: ProjectFortress/tests/tryatomicTest.fss:19-25, ProjectFortress/tests/abortTest.fss:20-27, ProjectFortress/tests/nestedTransactions3.fss:24-28*

```fortress
shouldRaise⟦Ex extends Exception⟧ (expr: ()→()): () = do
    try
        expr()
        throw ForbiddenException   (* reached only if expr did NOT raise *)
    catch x
        Ex => assert(true)         (* the corpus's only catch arm labelled by a TYPE VARIABLE *)
    forbid Exception               (* any other exception is a failure *)
    end

shouldRaise⟦IndexOutOfBounds⟦ZZ32⟧⟧ (fn() => testString[i])
(* ASCII spelling of the call: shouldRaise[\IndexOutOfBounds[\ZZ32\]\] *)
```

The only exception construct written with U+27E6/U+27E7 double square brackets.

*Seen in: Library/FortressLibrary.fss:312-319, ProjectFortress/tests/StringTests.fss:37-39*

## 13. Types, tuples, dimensions and units

`:` is the universal type ascription punctuation, 33814 uses over 1526 files. [fortressc]

```fortress
halve(x: RR64): RR64 = x/2         (* parameter and return type on a top-level function *)
ff(x: RR64, y: RR64) = x + y       (* the return type may be left off *)
squares:Array[\ZZ64\] = array(n)   (* no space round the colon, the dominant Library style *)
rel : (E, E) -> Boolean = p(self.seed).relation()   (* spaces on both sides are equally legal; REJECTED, an arrow type is not implemented in this subset *)
f: (RR64, RR64) -> RR64            (* REJECTED: an arrow type is not implemented in this subset. A type with no `= expr` is a forward/abstract declaration only as a TRAIT member, where fortressc accepts it; at component level fortressc calls it a component-level value declaration and refuses it whatever the type *)
```

*Seen in: SpecData/examples/preliminaries/Overview.Types.fss:19, fortressc/tests/arraysum.fss:6, SpecData/examples/basic/Types.Arrow.fss:23*

Empty parens are both the void type and its sole value; which one falls out of position. `: ()` runs to 2009 annotations over 1058 files, more than `: Boolean` (1357) or `: List` (775). [fortressc]

```fortress
greet(): () = println("hello from a void function")   (* void as a return type *)
run():() = ()                       (* the standard no-op entry point: type, then value *)
yogiBerraAutograph = ()             (* REJECTED: `()` has no value, so it cannot be stored in a binding *)
loop(f:E->()): () = generate[\()\](VoidReduction, f)  (* range, return type and static argument; REJECTED, an arrow type is not implemented in this subset *)
f(x: ()): ZZ32 = 1                  (* REJECTED: `()` has no value, so it cannot be stored in a parameter *)
```

*Seen in: fortressc/tests/unitvoid.fss:4, Library/ChunkedSparseArray.fss:168, SpecData/examples/basic/Expr.Typecase.fss:23, Library/FortressLibrary.fss:1052, fortressc/tests/badvoidparam.fss:4*

`Any` tops the whole hierarchy, `Object` tops the object types. [parses]

```fortress
trait Any end                       (* a native component holding one empty trait *)
trait Object extends Any            (* Object sits directly under Any *)
debugString(x: Object): String = x.asString
cast[\T extends Any\](x: Any): T =  (* Any as a static-parameter bound and as a parameter type *)
```

`Any` (851 uses) beats `Object` (252) on static-parameter bounds, not on varargs: 302 of the 851 are `extends Any` and only 29 are the vararg type `Any...`. fortressc has only ZZ32, ZZ64, RR64, Boolean, String, `()` and `Array[\T\]`, so `Object` parses as an identifier and then dies with `unknown type `Object``.

*Seen in: ProjectFortress/LibraryBuiltin/AnyType.fss:15, ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:332, Library/CompilerLibrary.fss:112*

### Tuples

A tuple type is written in exactly the shape of the value that inhabits it. [parses]

```fortress
("this", "is", "a", "tuple", "of", "mostly", "strings", 0)   (* heterogeneous tuple value *)
getPair():(Key, Val) throws NotFound         (* tuple return type *)
b3: ((A, B), B) = b33                        (* tuple types nest *)
cross[\G\](g: Generator[\G\]): Generator[\(E,G)\] =   (* a tuple type as a static argument *)
indexAndMask(i:ZZ32) : (ZZ32,ZZ64) = (i RSHIFT 6, widen(1) LSHIFT (i BITAND 63))
```

`(T)` with one element is just a parenthesised type, not a one-tuple, and `()` is void rather than a zero-tuple. fortressc answers `a tuple type is not implemented in this subset`.  ⚠ 2026-08-23: TUPLE TYPES AND TUPLE DESTRUCTURING WORK -- `(a, b) = (1, 2)` then `a + b` prints 3 -- and an api may NAME a tuple freely. What is still refused is a tuple VALUE in a position needing a representation, and the message changed: `a tuple value is not implemented in this subset, so it cannot be the result of a function with a body`.

*Seen in: SpecData/examples/preliminaries/Overview.Expression.tuple.fss:20, Library/Map.fss:41, Library/ChunkedSparseArray.fss:75*

Parenthesised binders on the left of `=` unpack a tuple. [parses]

```fortress
(a, b, c, d, e, f, g, _) = h()      (* eight-way unpack, `_` discards a component *)
(old,eval) = attempt()              (* the compact no-space Library spelling *)
(y_L, y_H) = (y1 MIN y2, y1 MAX y2) (* both sides tuples, so this binds simultaneously *)
unwrap(a:(R,R)) = do (r1, r2) = a; r1 end   (* destructuring inside a do block *)
(x: ZZ64, y: ZZ64, z: ZZ64) = (0, 1, 2)     (* a binder MAY carry a type annotation *)
(c:ZZ32,m:List[\ZZ32\],newinv) = (1, <|[\ZZ32\]1, 2|>, "test")   (* annotated and bare mixed in one tuple *)
```

Annotated binders are legal but rare: 15 hits over 9 files, two of them an `opr` parameter list on a continuation line rather than a binder.

*Seen in: SpecData/examples/preliminaries/Overview.Expression.tuple.fss:24, Library/Lazy.fss:22, Fortify/example/buffons.fss:34, SpecData/examples/basic/Var.Top.e.fss:19, ProjectFortress/tests/mixedTypeAnnotation.fss:17*

### Arrow and rest types

The domain of `->` is a single type, so a multi-argument function's domain is a tuple type. [parses]

```fortress
f: (RR64, RR64) -> RR64                   (* tuple domain *)
g: ZZ32 -> (ZZ32, ZZ32) throws IOFailure  (* `throws E` follows the range *)
opr COMPOSE[\A,B,C\](f: B->C, g: A->B): A->C = fn (a:A): C => f(g(a))
mapReduce[\R\](body: E->R, join:(R,R)->R, id:R): R =
opr SYMMETRIC_PARTIAL(self, other:()->Comparison): Comparison = self SYMMETRIC_PARTIAL other()
```

`E->R` and `E -> R` mean the same thing; the tight spelling dominates Library/. `->` binds looser than juxtaposition, which is why a juxtaposed dimensioned range has to be parenthesised. Fortress has no written union or intersection type at all. Comment-stripped, the whole corpus holds 4 `&` in 3 files and none of them is a type operator: a character literal in Library/Format.fss, a regex in RegexUse1.fss, and two line continuations of the kind the operator section documents. `|` is aggregate brackets, size bars and map arrows.

*Seen in: SpecData/examples/basic/Types.Arrow.fss:23-24, Library/FortressLibrary.fss:52, Library/FortressLibrary.fss:144*

A parameter type followed by `...` takes zero or more arguments, 78 uses over 42 files. [legacy]

```fortress
format(string:String,args:Any...): String = do   (* rest parameter after a fixed one *)
print(x:Any...):() = writes(x)                   (* the printf idiom of the I/O library *)
foo(y:ZZ32, z:ZZ32, x:ZZ32...) =                 (* two fixed parameters, then the rest *)
private object BoundedProperty(props:GenericProperties, p:(Object,Any...)->Any)  (* rest inside an arrow domain *)
```

fortressc's parser stops at the first dot: `expected `)`, found Dot`.  ⚠ 2026-08-23: VARARGS PARSE NOW: `f(x: ZZ32...): ZZ32 = 1` compiles.

*Seen in: Library/Format.fss:437, Library/File.fss:89, Library/ReflectiveQuickCheck.fss:252*

Three Library files and four tests are hand-Unicode-ified. Read them, do not copy them. [legacy]

```fortress
generate⟦R⟧(r: Reduction⟦R⟧, body: Char→R): R =   (* U+2192 is `->`, U+27E6/U+27E7 are `[\` and `\]` *)
fib:Array1⟦ZZ32, 0, 47⟧ = do
shouldRaise⟦Ex extends Exception⟧ (expr: ()→()): () = do   (* void-to-void thunk, Unicode spelling *)
```

4 Unicode arrows in 2 files against 1925 ASCII `->`, and 79 Unicode `⟦` in 7 files against 20368 ASCII `[\`. fortressc refuses both at the lexer: `non-ASCII characters are not in the M1 subset outside comments and strings`.  ⚠ 2026-08-23: BOTH COMPILE now: `f(g: ZZ32 → ZZ32)` and `object Cell⟦T⟧(held: T)` are on the allowlist.

*Seen in: Library/String.fss:86, Library/String.fss:20, Library/FortressLibrary.fss:312*

### Ascription and type dispatch

`asif` upcasts, 196 uses over 36 files. It is a view, not a check. [legacy]

```fortress
samples : List[\ZZ32\] = <|0 asif ZZ32,1,2,7,11|>   (* fix an aggregate's element type via its first element *)
getter max(): Just[\N\] = just (((1 asif N) LSHIFT wordsize) - 1)
arb = c.arbitrary asif ReflectiveArbitrary   (* view a result at a trait to reach that trait's methods *)
end asif OpRef                               (* the rare block form: ascribes the whole preceding block *)
```

*Seen in: ProjectFortress/tests/CovariantTest.fss:17, Library/Random.fss:270, Library/FortressAstUtil.fss:31-32*

`typed` is a plain ascription of an expression's static type, 43 uses over 21 files. [legacy]

```fortress
run() = println (3 typed Number)   (* the minimal form *)
y:ZZ32 = x typed ZZ32              (* ascription feeding an annotated binding *)
fn (x:Any):T => x typed T          (* ascribing to a static parameter *)
end typed Boolean                  (* block form: after `end`, ascribes the block's value *)
end inner typed Boolean            (* label first, then the ascription *)
```

*Seen in: ProjectFortress/tests/AsExprSimple.fss:16, ProjectFortress/compiler_tests/Compiled5.ag.fss:18, ProjectFortress/compiler_tests/Compiled9.t.fss:24-26*

`typecase ... of` is the third ascription form and the only one that inspects the run-time type.
It closes with `end` like the other control constructs, so its arms and its `else` are in
section 9. `of` hosts exactly two constructs in the whole corpus, `typecase` and value
`case ... of`; it is never a type operator. [legacy]

### Arrays, vectors and matrices

```fortress
squares:Array[\ZZ64\] = array(n)   (* one dimensional, homogeneous, ZZ64 subscripts *)
held:Array[\String\] = array(64)   (* a reference element type forces the scanned allocator *)
trait Array[\E,I\] extends { ReadableArray[\E,I\], MutableIndexed[\E,I\] }   (* legacy: E element, I index *)
toArray[\E\](g:Indexed[\E,ZZ32\]): Array[\E,ZZ32\] = do
```

The one-argument `Array[\T\]` is what the Rust compiler implements [fortressc]; the two-argument index-type form is legacy only.

*Seen in: fortressc/tests/arraysum.fss:6, fortressc/tests/gcarray.fss:11, Library/FortressLibrary.fss:1886*

`T[n,m]` is by far the corpus's most common sized-array type, 376 uses over 76 files. [legacy]

```fortress
A: ZZ32[3,3] = [1 2 3; 4 5 6; 7 8 9]   (* sizes comma-separated, `;` separates rows *)
MA8(a:ZZ32[8,8],b:ZZ32[8,8]):ZZ32[8,8] = do    (* in parameter and return position *)
a : ZZ32[1077] = chunkedSparseArray[\ZZ32\](1077,-1)
A: ZZ32[3,3,3] =                       (* three dimensions *)
a: Any[3] = [5 mg, 3 m, 4 s]           (* over the top type, three differently-dimensioned values *)
```

`ZZ32[3](Length)` attaches a dimension after the size. Chaining postfixes is illegal: `ZZ32^3[5]`, `ZZ32[3][5]` and `ZZ32[3]^5` appear only inside typeTests.fss's `(* Rightly rejected *)` comment blocks.

*Seen in: SpecData/examples/basic/Expr.Array.a.fss:20, ProjectFortress/demos/mm.fss:15, ProjectFortress/tests/typeTests.fss:46*

The value on the right of one of those types is an array element literal, and its separators are the whole syntax: elements are JUXTAPOSED, a `;` or a newline ends a row, `; ;` ends a plane, and an existing array pastes in as a slab. Expr.Array.f.fss comments out a `; ; ;` for a fourth axis. SpecData ships six files that vary nothing but the separators. 144 uses over 72 files. [legacy]

```fortress
a: ZZ32[3]     = [1 2 3]           (* one dimension: juxtaposition is the only separator, no commas *)
A: ZZ32[2,2]   = [3 4 ; 5 6]       (* `;` ends a row *)
A: ZZ32[2,2]   = [3 4
                  5 6]             (* a NEWLINE ends a row just as well *)
A: ZZ32[2,2]   = [ 3 4

                 ; 5 6 ]           (* blank lines are free and the `;` may lead the next row *)
A: ZZ32[3,3,3] = [1 0 0
                  0 1 0
                  0 0 1 ; ; 0 1 0
                            1 0 1
                            0 1 0 ; ; 1 0 1
                                      0 1 0
                                      1 0 1]   (* `; ;`, space allowed, ends a PLANE *)
B: ZZ32[2,3,2] = [A ;; 3 4 5; 6 7 8 ]   (* PASTING: an existing array spliced in as a slab *)
a : RR64[3] = [1.1 2.2 3.3]        (* arrayArgs.fss marks this one "This works" *)
f([1.1 2.2 3.3])                   (* and the bare literal as an argument "This breaks" *)
```

The trap is that part of it compiles. fortressc has its own COMMA-separated array literal, so `a = [1, 2, 3]` compiles and `a[2]` is 3, while the Fortress spelling `a = [1 2 3]` also compiles and gives a ONE-element array, `length(a)` 1, holding the juxtaposition product 6. Only the multi-row forms are refused outright, `expected `]`, found Semi`, and the sized-array type never even gets read: `a: ZZ32[3] = [1, 2, 3]` dies on the colon with `expected a newline or `;`, found Colon`.  ⚠ 2026-08-23: EVERY CLAUSE OF THIS PARAGRAPH IS NOW FALSE. `a = [1 2 3]` gives a THREE-element array; `a: ZZ32[3] = [1 2 3]` and the comma spelling both compile; and the matrix aggregate `[3 4; 5 6]` builds a rank-two array. The separator LEVEL decides the shape and the mapping is not the identity: `;` steps dimension 0, whitespace steps dimension 1, `;;` steps dimension 2.

*Seen in: ProjectFortress/parser_tests/XXXarrayTest.fss:61, SpecData/examples/basic/Expr.Array.e.fss:20, SpecData/examples/basic/Expr.Array.b.fss:20-21, SpecData/examples/basic/Expr.Array.d.fss:20-22, SpecData/examples/basic/Expr.Array.f.fss:21-27, ProjectFortress/not_passing_yet/Expr.Array.Pasting.fss:21, ProjectFortress/not_passing_yet/arrayArgs.fss:19, ProjectFortress/not_passing_yet/arrayArgs.fss:22*

`^` is the other sized-array spelling: `T^n` for a vector and `T^(n BY m)` for a matrix, with `BY` an ordinary infix operator rather than a keyword. 13 vector uses over 6 files, 5 matrix uses over 3. [legacy]

```fortress
b': ZZ32^3                                 (* ACCEPTED; only the chained ZZ32^3^5 and ZZ32^3[5] are not *)
position(): RR^3                           (* the spec's own Overview walkthrough uses RR^3 throughout *)
pos: RR^3 = [3 2 5]                        (* a vector type taking an array element literal *)
f(x: ZZ32[3], y: ZZ32^(2 BY 4)): () = ()   (* the two spellings side by side in one signature *)
map[\nat n,T\](f:T -> ZZ32,x:T^(n BY 1)):ZZ32^(n BY 1) = do   (* a nat static parameter as a dimension *)
f(first: RR^(2 BY 2)): () = do             (* an unsized numeric type carries it too *)
assert(3 BY 3 = 9)                         (* outside a type, BY is just a multiplying operator *)
```

*Seen in: ProjectFortress/tests/typeTests.fss:20, SpecData/examples/preliminaries/Overview.Sol.fss:22, SpecData/examples/preliminaries/Overview.p1.fss:27, ProjectFortress/tests/AfterTypeChecking.fss:15, ProjectFortress/tests/ho.fss:16, ProjectFortress/compiler_tests/Compiled1.ar.fss:15, ProjectFortress/compiler_tests/IntegerLiteralsFolding.fss:19*

Rank-specific array traits take a (base, size) pair of naturals per dimension. [legacy]

```fortress
trait Array1[\T, nat b0, nat s0\]                    (* lowest index b0, size s0 *)
trait Array2[\T, nat b0, nat s0, nat b1, nat s1\]    (* base and size per dimension *)
trait Array3[\T, nat b0, nat s0, nat b1, nat s1, nat b2, nat s2\]   (* the corpus stops here *)
trait Vector[\T extends Number, nat s0\]             (* Vector is a zero-based Array1 *)
trait Matrix[\T extends Number, nat s0, nat s1\]
emptyArray : Array1[\T,0,0\] = array1[\T,0\]()       (* lowercase array1/vector/matrix are the constructors *)
```

All of them need `nat` static parameters, which fortressc refuses at the parser.

*Seen in: Library/FortressLibrary.fss:2093, Library/FortressLibrary.fss:2189-2190, Library/ChunkedSparseArray.fss:48-49*

### Type aliases

```fortress
type IntList = List[\ZZ32\]                        (* an abbreviation for an existing type *)
{ S extends Number, type IntList = List[\ZZ64\],   (* an alias as a where-clause constraint *)
```

3 real declarations in 3 files, so this is not idiomatic. fortressc: `reserved word `type` is not in the implemented subset`. [legacy]

*Seen in: ProjectFortress/parser_tests/DeclTest.fss:15, ProjectFortress/tests/whereTest.fss:19*

### Dimensions and units

Everything under this heading is [legacy]: the four unit libraries live under Library/incomplete/, the feature was never finished, and fortressc accepts none of it. 75 `dim` uses in exactly 3 files.

```fortress
dim Length  SI_unit meter meters m_    (* base dimension, declaring its SI unit inline *)
dim Time  SI_unit second seconds s_
dim Mass default kilogram; SI_unit gram grams g_: Mass   (* `default` names the representation unit *)
dim Information  unit bit bits         (* `unit` instead of `SI_unit` for a non-SI unit *)
```

The Mass line is the interesting one: SI base mass is the kilogram but the prefixable unit is the gram, and `default` is how that gets said.

*Seen in: Library/incomplete/basic/Fortress.SIUnits.fss:18-20, Library/incomplete/basic/Fortress.SIUnits.fss:19, Library/incomplete/basic/Fortress.InformationUnits.fss:14*

Derived dimensions compose by power, quotient and juxtaposition.

```fortress
dim Area = Length^2                    (* ^ raises a dimension to a natural power *)
dim Volume = Length^3
dim Velocity = Length / Time           (* / is quotient *)
dim Momentum = Mass Velocity           (* juxtaposition is product *)
dim MomentOfInertia = Mass Length^2    (* ^ binds tighter than juxtaposition *)
dim Angle = Unity  SI_unit radian radians rad_ = 1 meter per meter   (* dimensionless, still named *)
```

*Seen in: Library/incomplete/basic/Fortress.SIUnits.fss:63-68, Library/incomplete/basic/Fortress.SIUnits.fss:80, Library/incomplete/basic/Fortress.SIUnits.fss:28*

`SI_unit` acquires the whole SI prefix set; `unit` does not. Both take one or more names, an optional `: Dim` and an optional defining expression.

```fortress
dim Length SI_unit meter meters m                  (* singular, plural, abbreviation *)
SI_unit metricTon metricTons tonne tonnes t_: Mass = 1000 kilograms   (* four names, then the dimension *)
unit inch inches: Length = 25.4 millimeters
unit inch inches: Length                           (* the definition is optional *)
unit byte bytes = 8 bits                           (* dimension inferred from the definition *)
```

Names ending in `_` (m_, s_, Hz_, t_) are the abbreviations; the trailing underscore keeps them off ordinary identifiers. Units are ordinary importable names, and the two libraries that import them use a form that appears nowhere else in the corpus: `import { Length, Area, Volume, Time, Mass, millimeters, liters, grams }` and then `from Fortress.SIUnits` on the next line. Potrzebie.fss writes the same form with the `from` on the closing-brace line. 2 files, both under `Library/incomplete/`; the import section has the forms everything else uses. [legacy]

*Seen in: ProjectFortress/tests/dimensionUnitDecl.fss:16, Library/incomplete/basic/Fortress.SIUnits.fss:117-118, Library/incomplete/basic/Fortress.EnglishUnits.fss:16-19, Library/incomplete/basic/Fortress.EnglishUnits.fss:13-14*

Five ordinary identifiers do the arithmetic inside unit and dimension expressions.

```fortress
unit radian = 1 meter per meter        (* `per` is division *)
unit meter_squared = square meter      (* `square` and `cubic` are prefix powers *)
unit gallon gallons: Volume = 231 cubic inches
dim SolidAngle = Unity  SI_unit steradian steradians sr_ = radian squared   (* `squared` is postfix *)
dim Conductance = 1 / Resistance  SI_unit siemens S_ = inverse ohms  (* `inverse` is prefix reciprocal *)
unit blintzal blintzals b_al_:  Force = blintz potrzebie per kovac squared  (* product, per and squared at once *)
3 grams per cubic centimeter           (* the same words work in expression position *)
(5 meter per second)squared
```

None of the five is a reserved word.  ⚠ 2026-08-23: ALL FIVE ARE RESERVED NOW -- `per`, `square`, `squared`, `cubic`, `inverse`, plus `cubed` -- and so is `in`. The reclassification FIXED A LIVE WRONG ANSWER: with `in` an ordinary identifier, `println(x in nm)` was a three-way juxtaposition PRODUCT and printed a number at exit 0 with no diagnostic. Placement is fussy: `(Time)squared` and `(Length Time)squared` are accepted, while unparenthesised `ZZ32 Length Time squared` and `per` inside a parenthesised dimension are recorded as "Rightly rejected".

*Seen in: ProjectFortress/tests/dimensionUnitDecl.fss:21-22, Library/incomplete/basic/Fortress.SIUnits.fss:45, Library/incomplete/basic/Fortress.Potrzebie.fss:75*

A numeric type juxtaposed with a dimension is a dimensioned type. 3 files in the whole corpus write one.

```fortress
x: RR64 Length = 1.3 m         (* unit-attached literal on the right *)
v: RR64 Velocity = x/t
b: ZZ32 Length/Time            (* quotient dimension *)
b'': ZZ32 Length^3             (* power dimension *)
n : ZZ32 (Length/Time)Mass     (* parenthesised dimension expression *)
o: ZZ32(Length)^3(Meter)       (* the parens may abut the numeric type *)
h: ZZ32 Length in meter        (* `in` pins the type to a unit of measure *)
y: (RR64 Velocity in furlongs) per fortnight = (v in furlongs) per fortnight   (* `in` works in expressions too *)
```

`in` is not in fortressc's lexer at all, so it would lex as an identifier; every other `in` in the corpus is English prose in a comment.  ⚠ 2026-08-23: `in` IS RESERVED now; see the note above. ProjectFortress/tests/typeTests.fss is the grammar's own boundary map, and its "Rightly rejected" comment blocks are the negative spec.

*Seen in: ProjectFortress/not_passing_yet/dimensionUnit.fss:17-19, ProjectFortress/tests/typeTests.fss:55-59, ProjectFortress/tests/typeTests.fss:38*

`unit` also classifies a static parameter, so a type can be parameterised by a unit of measure.

```fortress
object O[\S, int i, unit U, bool b\]() end        (* unit kind alongside type, int and bool *)
O[\P, 2 + 0, dimensionless, true AND false\]()    (* `dimensionless` is the unit static argument *)
trait T where [\ nat n, int i, bool b, unit u \]  (* the same kinds via a where clause *)
trait Float1[\unit U absorbs unit, nat e, nat s\] end   (* absorbs: the type swallows an applied unit *)
```

All three corpus uses of `absorbs` are literally `absorbs unit`, and no .fss file writes `[\dim D\]` even though `dim` is reserved as a kind. fortressc refuses nat/int/bool/opr/unit/dim at the parser.

*Seen in: ProjectFortress/not_passing_yet/staticArg.fss:17, ProjectFortress/not_passing_yet/WhereConstraints.fss:15, ProjectFortress/tests/dimensionUnitDecl.fss:24*

### Types as values

`dominates`, the language's only lower bound on a type, is a static-parameter bound and is in
section 8 with `extends` and the variance annotations. [legacy]

Library/Reflect.fss models types as values, and its six mutually excluding kinds are the corpus's own taxonomy of what a type can be. These are ordinary library traits, not grammar: there is no `Bottom` keyword and no value is ever annotated with a bottom type. [parses]

```fortress
trait Type extends StandardTotalOrder[\Type\]
trait ArrowType extends Type
trait TupleType extends {Type, ZeroIndexed[\Type\]}   (* a tuple type is itself indexable by position *)
trait BottomType extends Type
excludes {ObjectOrTraitType, ArrowType, TupleType, RestType, BottomType}   (* the six kinds are disjoint *)
voidType: Type = ReflectTuple[\()\]()                 (* void reflected as a zero-element tuple type *)
```

*Seen in: Library/Reflect.fss:222, Library/Reflect.fss:63, Library/Reflect.fss:375*

## 14. Contracts, tests and modifiers

Everything in this section is [legacy] except `assert`, which fortressc compiles, and `getter`/`setter`/`var` in member position, which it [parses] and then refuses. fortressc rejects `requires` at the parser with ``expected `=`, found Reserved("requires")``; `ensures` and `invariant` give the same message with their own name in it.

### Contracts

A `requires` clause is a precondition, written between the header and the `=`.

```fortress
factorial(n: ZZ64) requires { n >= 0 } =   (* precondition, always before the = *)
  if n = 0 then 1
  else n factorial(n - 1)
  end

factorial(n)
    requires {n >= 0}                      (* same thing, clause on its own line *)
    = if n = 0 then 1 else n factorial (n-1) end

fib13[\I\](n:I): I requires { n >= 0 } = do   (* after static params, params AND return type *)
    |\ phi^n / SQRT 5 + 1/2 /|
  end
```

*Seen in: SpecData/examples/basic/Fun.Contract.fss:25-28, ProjectFortress/tests/contracts1.fss:14-18, ProjectFortress/tests/fib13.fss:17-19*

Both `requires {n >= 0}` and `requires { n >= 0 }` occur and neither dominates: 6 of the 10 uses write it tight, 4 spaced. One brace can hold a comma list, and object headers and methods take the same clause.

```fortress
f(n: ZZ32) requires {n >= 0,
                     n + 3,                (* the parser accepts non-Boolean-looking entries *)
                     n-5 <= 0} = ()        (* only comma-list `requires` in the whole corpus *)

object O[\nat n\] requires {n >= 0} end    (* on an object header, constraining a static param *)

  field(v: New, n: String) requires {v.isVal()} = do   (* method form; predicates are arbitrary exprs *)
```

*Seen in: ProjectFortress/compiler_tests/Compiled10.c.fss:19-21, ProjectFortress/compiler_tests/Compiled5.z.fss:15, ProjectFortress/demos/FeatherweightJava.fss:39-40*

`ensures` is the postcondition, and inside it `outcome` names the returned value. `outcome` is not reserved, just an identifier the contract machinery binds.

```fortress
mangle(input: List) ensures { sorted(outcome) provided sorted(input) } =
  if input =/= Empty                       (* `provided g` makes the predicate conditional on g *)
  then mangle(first(input))
       mangle(rest(input))
  end

  eval(CT_: ClassTable, theta: Map[\String, Expr\]): Expr
    ensures { outcome.isVal() }            (* bodiless trait method: clause after the return type, no = *)

g(n: ZZ32) ensures  {n >= 0,
                     n + 3,
                     n-5 <= 0 provided 8} = ()  (* `provided` binds the LAST predicate, not the brace *)
```

*Seen in: SpecData/examples/basic/Fun.Contract.fss:30-34, ProjectFortress/demos/FeatherweightJava.fss:88-89, ProjectFortress/compiler_tests/Compiled10.c.fss:22-24*

All 8 uses of `provided` are inside `ensures` braces; it never appears in a `requires` and never outside a contract.

```fortress
factorial(n)
  requires { n >= 0 }                      (* stacked clauses: requires first, then ensures, then = *)
  ensures { outcome >= 1 provided true }
  = if n = 0 then 1 else n factorial(n-1) end

blah(n) invariant {n > 0} =  atom_sum(n)   (* the third clause, and the single use in the corpus *)

object U throws Exn ensures { true } extends T end   (* object header order: throws, ensures, extends *)
```

*Seen in: SpecData/examples/preliminaries/Overview.Function.contract.b.fss:19-23, ProjectFortress/tests/contracts1.fss:78, ProjectFortress/parser_tests/XXXobjectClauses.fss:17*

Nothing in the corpus carries all three clauses, and `invariant` never appears beside another one. A contract brace holds any expression, including a block that declares its own contracted function.

```fortress
f() ensures { do (outcome = 5)
                 g() ensures { (outcome = 3) provided true } = 3   (* inner `outcome` is g's *)
                 g()
                 true
              end provided true } = 5
```

*Seen in: ProjectFortress/tests/nestedOutcome.fss:15-19*

Contract failure is observed as an exception: a broken `requires` is the caller's fault, a broken `ensures` is the callee's.

```fortress
try
    factorial(-1)
catch e
        CallerViolation => worked := true   (* the requires {n >= 0} was broken at the call site *)
end

try
    baz(10)
catch e
        CalleeViolation => worked := true   (* the ensures was broken inside the body *)
end
```

*Seen in: ProjectFortress/tests/contracts1.fss:24-28, ProjectFortress/tests/contracts1.fss:70-74*

### Properties

A `property` is an algebraic law, usually an anonymous trait member. 14 of the 16 in the corpus are in one file.

```fortress
trait TotalOrder[\T extends TotalOrder[\T,PRECEQ\], opr PRECEQ\]
    extends { PartialOrder[\T,PRECEQ\] }
  property FORALL (a: T, b: T) (a PRECEQ b) OR (b PRECEQ a)   (* no name, no =, just the law *)
end

  property FORALL (a: T, b: T) ((a CMP b) === LessThan) IFF ((b CMP a) === GreaterThan)
  property FORALL (a: T) a PRECEQ MaximalElement[\PRECEQ\]    (* one binder; follows a where clause *)

property fIsMonotonic = FORALL(x: ZZ, y: ZZ) (x < y) IMPLIES (f(x) < f(y))   (* named form, component level *)
```

*Seen in: Library/incomplete/advanced/Fortress.PartialTotalOrders.fss:26-29, Library/incomplete/advanced/Fortress.PartialTotalOrders.fss:46, Library/incomplete/advanced/Fortress.PartialTotalOrders.fss:103-106, ProjectFortress/parser_tests/DeclTest.fss:19*

`FORALL` never appears outside a `property`, there is no `EXISTS`, no Unicode `∀`, and every binder in the corpus carries `: Type`. The named form writes `FORALL(x: ZZ, ...)` and the trait-member form `FORALL (a: T, ...)`; both parse. Laws the shipping library cannot check are written commented out, in `(* *)` or `(*)`: `property` has 44 raw occurrences against the 16 live ones.

### Tests

`test` is a modifier, not a declaration form of its own, and it only ever appears at component level. 66 uses over 22 files.

```fortress
test testFactorial1() = do          (* the common shape: test, name, (), =, do block *)
   assert(fact(0) = 1)
   assert(fact(5) = 120)
end

test testRelationUtilities():() = do    (* explicit ():() signature, the shipping-library habit *)
test leftShouldBeFirst = do             (* no parameter list at all *)
test testRadicalConstruction () = do    (* a space before the () also parses *)
```

*Seen in: ProjectFortress/tests/testTest2.fss:19-22, Library/Relation.fss:189, Sandbox/PureListBehavior.fss:22-25, ProjectFortress/demos/turnersParaffins0.fss:438*

The same modifier attaches to bindings, objects and plain helpers, which is what makes it a build/visibility modifier rather than a case marker.

```fortress
test y: ZZ32 = 1                    (* a test-only binding, beside `private x: ZZ32 = 0` *)
test testData[ ] = { True, False, Uncertain, Impossible }   (* test-only array binding *)
test fxLessThnFy[x <- E, y <- F] = assert(f(x) < f(y))      (* generator-driven: [ ] and <-, not ( ) *)
```

*Seen in: ProjectFortress/parser_tests/VarNYITest.fss:15-19, Library/incomplete/basic/Fortress.Standard.fss:20, ProjectFortress/parser_tests/DeclTest.fss:17*

```fortress
test object TestSuite(testFunctions = {})   (* the modifier sits where `value` and `private` go *)
  add(f: () -> ()) = testFunctions.insert(f)
end

test fail(message: String) =        (* a test-only helper that is not itself a case *)
  print message
  throw TestFailure
```

*Seen in: Library/incomplete/basic/Fortress.Standard.fss:52-58, Library/incomplete/basic/Fortress.Standard.fss:60-64*

There is no `test` member of a trait or object anywhere, and `private test` never occurs in either order.

### Assertions

`assert` has four arities and fortressc implements all of them. [fortressc]

```fortress
assert(true)                                  (* Boolean *)
assert(1 < 2, "one is less than two")         (* Boolean plus a failure message *)
assert(3, 3)                                  (* two values, compared with = *)
assert(2 + 2, 4, "arithmetic still works")    (* comparison plus a varargs message *)
```

*Seen in: fortressc/tests/builtins.fss:25-28, Library/FortressLibrary.fss:286-291*

`assert(a, s)` where `s` is a String VARIABLE is the (Boolean, String) form, so deciding by whether the second argument is a literal is wrong. In fortressc a failed assert halts with a diagnostic and exit 1 instead of throwing, and it is only as strong as `=`, so `assert("a","b")` is refused. The library builds the message lazily. The varargs declaration itself is out of the subset: fortressc answers ``expected `)`, found Dot`` at the `Any...`. [legacy]  ⚠ 2026-08-23: varargs parse now.

```fortress
assert(x:Any, y:Any, failMsg: Any...): () =
    if x =/= y then
        msg = x.asDebugString " =/= " y.asDebugString "; " (BIG || failMsg)
        fail(msg)                      (* the message is only built when the check fails *)
    end

    assert( |r|, |elts|, "Size mismatch between ", r, " and ", elts)   (* real varargs usage *)
```

*Seen in: Library/FortressLibrary.fss:296-300, Library/Avl.fss:352*

```fortress
deny(flag:Boolean): () = assert(NOT flag)    (* the negated form, same arities *)
deny(flag: Boolean, failMsg: String): () = assert(NOT flag, failMsg)

        deny(range.isEmpty, "SubString (" range ") has empty range")
```

*Seen in: Library/FortressLibrary.fss:302-304, Library/String.fss:429-430*

`deny` is an ordinary library function, not a keyword, and fortressc has no definition for it: "unknown name `deny`". Both bottom out in `fail`, whose `throw` is refused with "reserved word `throw` is not in the implemented subset". [legacy]

```fortress
fail[\T\](s:String):T = do          (* generic in the return type so it fits any expression slot *)
    errorPrintln("FAIL: " s)
    throw FailCalled(s)
  end
```

*Seen in: Library/FortressLibrary.fss:54-57, Library/ChunkedSparseArray.fss:111*

There is no `assertRaises` builtin. Exception expectations are ordinary Boolean predicates handed to `assert`, which puts them out of the subset too: "reserved word `try` is not in the implemented subset". [legacy]

```fortress
shouldOverflow(f: () -> ZZ64): Boolean =
  try
    ignore f()
    false
  catch e
    IntegerOverflow => true
    Exception => false
  end

  assert(shouldOverflow(fn () => -minValue))   (* the call site: a thunk inside the predicate *)
```

*Seen in: ProjectFortress/library_tests/Integer4.fss:17-24, ProjectFortress/library_tests/Integer4.fss:93*

`shouldOverflow` and `shouldDivideByZero` have 60 uses across 5 files; 23 of them are in that one file, 21 of those call sites and 2 the definitions. The library's `shouldRaise[\Ex extends Exception\](expr: ()->()): ()` is defined once, in the Unicode `⟦ ⟧` spelling.

### Modifiers

`private` restricts a declaration to its component and is the most-used real modifier after `getter` and `var`: 287 uses over 36 files. fortressc answers "reserved word `private` is not in the implemented subset".

```fortress
private object Concat[\E\] extends MonoidReduction[\ List[\E\] \]
private trait Sized
private x: ZZ32 = 0                                    (* component-level binding *)
    private getter w(): ZZ32                           (* private comes first when combined *)
private value object SeqListGenerator[\E\]( it: FingerTree[\E\] )   (* order: private, value, object *)
```

*Seen in: Library/PureList.fss:145, Library/PureList.fss:266, ProjectFortress/parser_tests/VarNYITest.fss:15, Library/Treap.fss:25, Library/PureList.fss:248*

`abstract` marks a member with no implementation, and only ever a member: `abstract object` and `abstract trait` do not exist.

```fortress
  abstract opr <(self, other:T): Boolean   (* abstract precedes opr *)
  abstract getter min(): Maybe[\T\]        (* and precedes getter *)
  abstract random(): T
```

*Seen in: Library/CompilerAlgebra.fss:18-22, Library/Random.fss:32, Library/Random.fss:42*

A bodiless member without `abstract` means the same thing in practice, which is why fortressc needs no rule for it: a bodiless declaration is simply never a dispatch target.

```fortress
value object CaseInsensitiveString(s:String)           (* immutable value type, 83 uses / 41 files *)
value trait AnyMaybe extends { Equality[\AnyMaybe\], AnyUniqueItem } excludes Number
value object MaximalElement[\opr PRECEQ\] end          (* static params only, empty body *)

native component FlatString      (* the ONLY position `native` takes: a component header, 9 files *)
```

*Seen in: Library/CaseInsensitiveString.fss:20, Library/FortressLibrary.fss:1292, Library/incomplete/advanced/Fortress.PartialTotalOrders.fss:101, Library/FlatString.fss:12*

`io` marks I/O, mostly on arrow types rather than declarations: 7 uses in 2 files, 5 of them on an arrow type. It is also the proof that modifiers may repeat.

```fortress
  a: io ZZ32->String                 (* io modifies an ARROW TYPE *)
  g: ZZ32 -> (io String -> ())       (* including a nested one inside parens *)

trait T
  io atomic io f():()                (* method-modifier position, and a duplicated modifier *)
end
```

*Seen in: ProjectFortress/parser_tests/ioTests.fss:18-21, ProjectFortress/parser_tests/XXXMultipleModifiers.fss:15-17*

That line is the one declaration-modifier use of `atomic` in the corpus; the other 181 are the concurrency construct, 116 of them the `atomic do ... end` block and the rest `atomic` applied to a single expression. `tryatomic` is a separate word, expression-only, 6 uses in 6 files. `default`, `most`, `also` and `forbid` look like modifiers and are not: they belong to the dimensions, case, block and exception families.

Field modifiers control the generated accessors.

```fortress
trait A
  hidden x: ZZ32                     (* suppresses the generated getter *)
end
    hidden settable my: T            (* hidden comes first; `hidden` 2 uses, `wrapped` 2, `settable` 4, the rarest field modifiers *)

object Player
  thisWon : ZZ32 := 0                (* a := initializer alone makes a field mutable *)
  var thisLost : ZZ32 := 0
  settable indices: ZZ32 = 3         (* settable generates the setter *)
  setter fld(x:ZZ32):() = fld := x   (* or write the setter out yourself *)

trait T[\X\] extends X
  wrapped x:X                        (* forwards X's members; always paired with extends X *)
end
object O[\Y\](wrapped x:Y) extends Y end   (* also legal on a constructor parameter *)
```

*Seen in: ProjectFortress/compiler_tests/Compiled6.b.fss:15-17, ProjectFortress/compiler_tests/VarianceTest8.fss:16, ProjectFortress/tests/setterTest.fss:15-20, ProjectFortress/compiler_tests/Compiled5.ao.fss:14-17*

```fortress
object B extends A
 override   f(self, other:Number):String = "f PASS"   (* deliberately replacing an inherited method *)
 override   g(other:Number):String = "g PASS"
end
```

*Seen in: ProjectFortress/tests/disp0.fss:21-24*

4 uses in 2 files. Shipping library code never writes `override`, it just redeclares.

A `getter` is invoked as `x.name`, a `setter` as `x.name := e`. `getter` is the single most common modifier in the corpus at 2310 uses over 233 files; `setter` is about 140x rarer at 16 uses over 13 files. fortressc compiles both DECLARATIONS and refuses the use: reading `O.n` gives "`n` is a getter or setter; accessors parse but are not implemented, and `n` is read rather than called", and `o.fld := 5` gives "only a variable or an array element can be assigned to". [parses]  ⚠ 2026-08-23: BOTH WORK NOW. A getter is read as `x.name`, and `o.fld := 5` assigns when the field is declared `var`. An immutable field answers `` field `n` is immutable; declare it `var n: T = ...` to assign to it ``. A DECLARED `setter` IS CALLED, fixed later the same day: `o.n := e` is a CALL to the setter, and it is chosen by the WRITTEN MODIFIER rather than by arity -- an ordinary dotted method `n(x: T)` has the same shape and does NOT capture the assignment. A setter needs no backing field at all, one over an immutable field is legal, and a setter on a trait dispatches to the object's override. The COMPOUND form `o.n += 1` is refused by name: it would have to read through the getter first.

```fortress
trait Avl[\K extends StandardTotalOrder[\K\],V\]
        comprises { AvlEmpty[\K,V\], AvlNode[\K,V\] }
    getter depth() : ZZ32                                    (* bodiless, the empty () is still written *)
    getter asDebugStriing(): AvlDump[\K,V\] = AvlDump[\K,V\](self)

  setter fld(x:ZZ32):() = fld := x
  setter message(String):()          (* bodiless setter: the parameter is a bare TYPE, unnamed *)
  setter x(y: ZZ32) = ()             (* return type may be omitted, and setters overload *)
```

*Seen in: Library/Avl.fss:16-19, ProjectFortress/tests/setterTest.fss:20, Library/incomplete/basic/Fortress.Standard.fss:25-27*

`var` marks mutability in three positions. 505 uses over 193 files: 73 at component level, 30 in a parameter list, the rest members and locals inside bodies.

```fortress
  var cumulativeSize: ZZ32 := 0      (* mutable member field *)
    var b:String                     (* declared with a type and no initializer *)

value object Lazy[\T\](var s : State[\T\])              (* constructor param becomes a mutable field *)
object Sudoku(var cands : ZZ32, var props : ZZ32,       (* repeated: var does not distribute *)

    var s:ZZ32 := 0                  (* local mutable, := form *)
  var x: ZZ32 = 0                    (* local mutable, = form, commoner in the corpus *)
```

*Seen in: Library/String.fss:171-173, ProjectFortress/compiler_tests/Compiled6.av.fss:14, Library/Lazy.fss:18, ProjectFortress/demos/aStar.fss:126, ProjectFortress/tests/contracts1.fss:41, Documentation/Specification/Code/DoAbbrev1.fss:18-20*

fortressc reads `var` only in member position, and even there only the `= e` form reaches the checker: `var c: ZZ32 = 0` in an object body answers `` `var c`: mutable fields are not implemented ``, `var c: ZZ32 := 0` never gets that far and answers ``expected a newline or `;`, found ColonEq``, and the bodiless `var c: ZZ32` is accepted silently. In a parameter list it answers `expected a parameter name, found KwVar` and in statement position `expected an expression, found KwVar`. Its mutable local is written without `var` at all, as `s: ZZ32 := 0`. [parses]

`coerce` is a member declaring an implicit conversion INTO the enclosing type. 48 uses over 21 files, 15 of them in one file building the numeric tower.

```fortress
trait ZZ extends { Number, Equality[\ZZ\] } excludes { RR64, ZZ64, ZZ32, NN32, NN64, IntLiteral }
    coerce(x: IntLiteral) = x.asZZ       (* an overload set of conversions into ZZ *)
    coerce(x: ZZ32) = x.asZZ
  coerce(t: (D, D)) = C                  (* the source type may be a tuple *)
    coerce(_: TestNothing) = TestNothingObject[\T\]   (* _ when the value is unused *)
```

*Seen in: ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:494-497, ProjectFortress/compiler_tests/Compiled270.fss:21-24, ProjectFortress/other_compiler_tests/CoerceTest2.fss:31*

Static parameters carry their own modifiers and relational bounds, all rare.

```fortress
  trait Test[\covariant T\]                        (* variance annotation, before the name *)
  trait Test[\contravariant T extends Any\]        (* combines with a bound *)
    f[\U dominates T\](x: U) : U                   (* a relational bound in place of extends *)
trait Float1[\unit U absorbs unit, nat e, nat s\] end   (* always the exact phrase `absorbs unit` *)
```

*Seen in: ProjectFortress/compiler_tests/VarianceTest1.fss:16, ProjectFortress/compiler_tests/VarianceTest2.fss:16, ProjectFortress/compiler_tests/VarianceTest4.fss:17, ProjectFortress/tests/dimensionUnitDecl.fss:24*

`widens` and `coerces` are relations between types inside a `where` clause. Note `coerces` (the relation) is a different word from `coerce` (the member).

```fortress
    where { T coerces MaximalElement[\PRECEQ\] }   (* an implicit conversion from T must exist *)
  where [\bool b', nat n\]
        { S extends Number, type IntList = List[\ZZ64\],
          S widens String, NOT b, b IMPLIES b',
          n = i, U = dimensionless, 2 n + i < 2^8 }
```

*Seen in: Library/incomplete/advanced/Fortress.PartialTotalOrders.fss:105, ProjectFortress/tests/whereTest.fss:18-21*

## 15. What the Rust rewrite compiles today

Everything below compiles with `fortressc` unless it carries [parses] (the parser takes it, the checker names it and refuses) or [legacy] (it exists in the corpus and the rewrite does not accept it).

### The driver

```text
fortressc prog.fss                     # lex, parse, typecheck, emit an object, link it
fortressc prog.fss -o prog             # -o defaults to the source path with the extension stripped
fortressc prog.fss --emit-ir           # textual LLVM IR to stdout, stops before any object
fortressc prog.fss --emit-obj -o p.o   # object at exactly -o, no link (the cluster build's split)
fortressc prog.fss --cc mpicc          # linker driver, defaults to cc
fortressc prog.fss --target-cpu skylake-avx512
# --target-cpu takes exactly six names: x86-64, x86-64-v2, x86-64-v3 (default),
# x86-64-v4, skylake-avx512, native. Anything else is refused with `unknown target CPU`.
# exit 1 is a diagnostic against your source. exit 70 is a compiler bug.
# Every link appends -lgc and -lm; runtime/mpi_shims.c joins only if you called an MPI builtin.
```

### Component shape

```fortress
component skeleton     (* the name may be dotted: component Foo.Bar *)
export Executable      (* NOT enforced: a component with no export line compiles *)

run() = do                       (* the entry point. run(): () = ... is the same thing *)
   println("the pipe exists")
end
end
```

`run` is the only entry point rule that bites: it must take no parameters, because generated `main` calls it with none. A component with no `run` at all still compiles.

*Seen in: fortressc/tests/skeleton.fss:1-7, fortressc/tests/assertfail.fss:8-12*

```fortress
export Executable      (* headerless: no component and no api line at all *)

  run(): () = println "Hello, World!"

end                    (* a stray end the parser tolerates; nothing opened it *)
```

Roughly 375 of 1789 corpus files are headerless. The compiler reports the component name as the empty string, which is why you see `typechecked `` with 1 function(s)`.

*Seen in: ProjectFortress/compiler_tests/Compiled0.u.fss:11-15*

```fortress
import List.{...}                            (* the dominant form: dotted name, brace group *)
import Map.{...} except { opr BIG UNION }    (* except takes a brace group or a bare name *)
import api Collection                        (* the api variant: 1 occurrence in the whole corpus *)
```

Only the dotted name is really parsed. The brace group and the `except` clause are swallowed as balanced token runs and discarded, because whole-program monomorphization has no separate compilation to feed. `import` and `export` may come in either order.

*Seen in: Library/Avl.fss:13, Library/Relation.fss:14, Library/incomplete/Sequence.fss:14*

### Types

```fortress
greet(): () = println("hello from a void function")   (* () is the void type AND the unit value *)
halve(x: RR64): RR64 = x/2                            (* no exponent syntax in float literals *)
f(x: (ZZ32)): (ZZ32) = x                              (* a parenthesised type is just the type *)
```

Seven types exist and there are no others: `ZZ32`, `ZZ64`, `RR64`, `Boolean`, `String`, `()` and `Array[\T\]`. `NN32`, `NN64`, `IntLiteral`, `Any` and everything else the legacy library declares are unknown names here.  ⚠ 2026-08-23: OUT OF DATE. `Type` has TWELVE variants: `ZZ32 ZZ64 RR64 Boolean String Char Void Array(Elem, rank) Object Trait Thread Tuple`. `Char` is a real type, arrays carry their RANK, `Any` and `Object` are root traits, and a `Thread[\T\]` handle exists. `NN32`, `NN64` and `IntLiteral` are still unknown unless the library declares them and an import brings them in -- there IS an import resolver now.

*Seen in: fortressc/tests/unitvoid.fss:4-6, fortressc/tests/rr64literal.fss:4-6, fortressc/tests/parenthesised.fss:4*

```fortress
j:ZZ64 = widen(20)     (* the ONLY numeric conversion: ZZ32 -> ZZ64, explicit, never inferred *)
```

`b: ZZ64 = a` with `a: ZZ32` is refused with `a ZZ32 value is not implicitly converted to ZZ64; write widen(...)`, and so is `b + a`, because an infix operator pushes the LEFT operand's type onto the right one. Turn it round and the expectation is the narrower type: `a + b` gives `expected ZZ32, found ZZ64`. `operands are ZZ32 and ZZ64; Fortress does not mix numeric types implicitly` is the JUXTAPOSITION message. An integer literal takes its type from the slot it lands in.

*Seen in: fortressc/tests/fact.fss:7-8*

### Functions and bindings

```fortress
f(x:ZZ64):ZZ64 = if x < 2 then 1 else x f(x-1) end   (* spacing around : is irrelevant *)
even(x: ZZ64): Boolean = x - (x / 2) 2 = 0           (* that trailing = is EQUALITY, not a second def *)
f(x: ZZ32): ZZ32 = x                                 (* the return annotation may be dropped *)
```

Top level only. No default arguments, no varargs, no keyword arguments. A top level declaration with no `= expr` never reaches the checker: the parser stops at `` expected `=`, found KwEnd ``. A trait member may be bodiless, and so may an object METHOD, which simply never becomes a dispatch target, so a call that would reach it gives `` no declaration of `noise` applies to (Rock) ``. An object FIELD may not: `` field `f` is not a constructor parameter, so it needs `= ...` ``.

*Seen in: fortressc/tests/fact.fss:4, fortressc/tests/parallelcollatz.fss:7, fortressc/tests/plainnamed.fss:4-6*

```fortress
n:ZZ64 = 100                        (* immutable: an SSA value, no alloca *)
squares:Array[\ZZ64\] = array(n)
i:ZZ64 := 0                         (* := declares mutable storage, and only := gets an alloca *)
while i < n do
   squares[i] := i i                (* array element: one of exactly two assignable targets *)
   i := i + 1                       (* variable: the other *)
end
```

The type annotation may be dropped when the initializer settles the type (`i = Cell[\ZZ32\](7)`). Assigning to an immutable gives `` `x` is immutable; declare it with `:=` to assign to it ``. Assigning to a name never declared gives `` `x` is not declared; write `x:T := ...` to declare it ``. Mutables declared inside a loop body are fine: every alloca is hoisted to the function entry block.

*Seen in: fortressc/tests/arraysum.fss:5-6, fortressc/tests/arraysum.fss:8-12, fortressc/tests/loopalloca.fss:5-8*

### Blocks and control flow

```fortress
side(): ZZ32 = do
   println("SIDE")
   7                    (* a block's value is its last item, so this returns 7 *)
end
inner(n:ZZ64):ZZ64 = if n < 1 then 0 else do
      s:String = "x" n
      inner(n-1)
   end
end
```

Items are separated by a newline or by a single `;`, and that `;` may sit mid line (`do a = 1; b = 2; a + b end` compiles) or be the last thing before `end`. `a;;b` has no parse, and neither does a `;` that opens a line.

*Seen in: fortressc/tests/builtins.fss:7-10, fortressc/tests/gcsoak.fss:4-8*

```fortress
pickShape(n: ZZ32): Shape =
   if n === 0 then Dot
   elif n === 1 then Box[\ZZ64\](1)
   else Box[\String\]("x") end     (* used as a VALUE it needs an else, and arms must agree *)

if even(n) then n := n / 2 else n := 3 n + 1 end   (* in statement position, else is optional *)
```

*Seen in: fortressc/tests/genericdispatch.fss:18-21, fortressc/tests/parallelcollatz.fss:13*

```fortress
i:ZZ32 := 0
while i < 2 do          (* the only sequential loop in the subset; its value is () *)
   j:ZZ32 := 0
   while j < 2 do
      println(draw(inkOf(i), faceOf(j)))
      j := j + 1
   end
   i := i + 1
end
```

There is no `do ... while`, no `break` and no `continue`. `break`, `continue`, `return`, `match`, `let` and `null` are not words in Fortress at all; `exit` and `label` are reserved and refused.

*Seen in: fortressc/tests/dispatch.fss:27-35*

### Juxtaposition

Two expressions side by side with no operator. This is the strangest thing in the language and it means three different operations.

```fortress
squares[i] := i i                        (* same numeric type: MULTIPLICATION, i squared *)
println("length = " length(squares))     (* either side a String: CONCATENATION *)
println(double 21)                       (* leading element is a function name: APPLICATION *)
println "Hello"
println(answer ())                       (* f () is the nullary call: the argument is unit *)
```

*Seen in: fortressc/tests/arraysum.fss:10, fortressc/tests/juxtapply.fss:4-9, fortressc/tests/juxtnullary.fss:4-6*

```fortress
apply(f: ZZ64, y: ZZ64): ZZ64 = f y   (* f is a PARAMETER, so it is a value: this multiplies *)
```

A name with a visible local or parameter binding is a value, and so is a singleton object, so `Marker 2` is refused. Three or more elements led by a function is refused too: `println(g 1 2)` gives `a juxtaposition of 3 elements led by a function is not implemented; parenthesise the application`. Mixed numeric types do not juxtapose.

*Seen in: fortressc/tests/juxtshadow.fss:6*

### Operators

```fortress
println(2^10)
println(2^3^2)     (* 64, not 512: the ^ group is LEFT associative *)
println(2 3^2)     (* ^ binds above juxtaposition and above every infix operator *)
println(2^2 + 1)
println(xr^rz)     (* operands need NOT agree: all four ZZ64/RR64 pairs work *)
```

`^` is the one binary operator exempt from the mixed-operand rule. The shim calls `pow`, which is why every link takes `-lm`. A negative integer exponent halts with a diagnostic and exit 1 rather than inventing zero. `**` is not an alternative spelling: it is a hard lex error, `` `**` is not a valid operator in Fortress ``. The corpus is full of `**` runs and not one of them is an operator: they all sit inside `(****` banner comments or string literals, where the lexer never looks for one, and stripping those leaves zero.

*Seen in: fortressc/tests/exponent.fss:12-16, fortressc/tests/exponent.fss:17,19,21, fortressc/tests/negexponent.fss:8-10*

```fortress
if 0 <= 0 < 1 = 1 < 2 <= 2 then println("YES") else println("NO") end
(* ONE chain, not four comparisons. Equivalences mix freely into orderings of one sense. *)
if 0 < mid(1) < 2 then println("YES") else println("NO") end   (* mid runs exactly ONCE *)
run(): () = if 1 <= 2 > 0 then println("y") else println("n") end
(* REFUSED: a chain mixes `<=` with `>`; chained ordering operators must have the same sense *)
```

*Seen in: fortressc/tests/chainmixed.fss:5, fortressc/tests/chainonce.fss:10, fortressc/tests/badchainsense.fss:4*

```fortress
println((true = true) AND (false =/= true))   (* = is equality here, =/= is its negation *)
topOf(n: ZZ32): Top = if n === 0 then OL else OR end   (* === is identity *)
```

There is no `==`. One `Eq` token serves both readings of `=` and the parser disambiguates by position. Ordering is not defined on Boolean: `true < false` gives `` `<` is not defined on Boolean; equality is, ordering is not ``.

*Seen in: fortressc/tests/logical.fss:27, fortressc/tests/ambiguous.fss:19*

```fortress
println(NOT true AND true)    (* parses as (NOT true) AND true: NOT binds above every infix *)
println(true AND true OR false)
println(false AND loud())     (* AND and OR short circuit: loud() never runs *)
println(true OR loud())
println(n AND true)           (* REFUSED: `AND` takes Boolean operands; this one is ZZ32 *)
println(NOT x < y)            (* REFUSED as (NOT x) < y, which is what pins the precedence *)
```

`AND`, `OR` and `NOT` are not keywords. They lex as ordinary identifiers and the parser recognises them by spelling. `||` and `&` lex but are not logical operators here.

*Seen in: fortressc/tests/logical.fss:23-26, fortressc/tests/badlogical.fss:13-14, fortressc/tests/badnot.fss:9-11*

### Builtins

```fortress
print("a")                              (* print, println, ignore, assert, widen, array, length *)
ignore(side())                          (* evaluates and discards *)
assert(true)
assert(1 < 2, "one is less than two")   (* the two-arg forms are told apart by TYPE *)
assert(3, 3)
assert(2 + 2, 4, "arithmetic still works")
assert("a", "b")   (* REFUSED: assert compares with =, which is not defined on String *)
```

These names are resolved ahead of user declarations, so a user function called `println` is unreachable. A failed assert halts with a diagnostic on stderr and exit 1; there are no exceptions in the subset.

*Seen in: fortressc/tests/builtins.fss:19-28, fortressc/tests/builtins.fss:16, fortressc/tests/badassert.fss:7-9*

### Arrays

```fortress
n:ZZ64 = 100
squares:Array[\ZZ64\] = array(n)   (* the annotation is the ONLY thing saying what it holds *)
held:Array[\String\] = array(64)   (* String storage is scanned by the collector *)
i:ZZ64 := 0
while i < 64 do
   held[i] := "item " i            (* subscripts are ZZ64, and every one is bounds checked *)
   i := i + 1
end
println(length(held))              (* length returns ZZ64 *)
```

One dimensional, homogeneous, header and elements in one allocation. Element types are exactly five: ZZ32, ZZ64, RR64, Boolean, String. `Array[\Array[\ZZ64\]\]` is refused as unrepresentable, `array(n)` with no annotation is refused, and `array(n)` refuses an object or trait element type. An out of range subscript halts with a diagnostic and exit 1 rather than faulting.

*Seen in: fortressc/tests/arraysum.fss:5-6, fortressc/tests/gcarray.fss:11-16, fortressc/tests/oob.fss:5-6*

### MPI

```fortress
run() = do
   mpiInit()
   rank:ZZ32 = mpiCommRank()     (* all four MPI builtins take NO arguments *)
   size:ZZ32 = mpiCommSize()
   println("rank " rank " of " size)
   mpiFinalize()
end
```

Exactly four MPI builtins exist and they have zero occurrences anywhere in the legacy corpus. The communicator is fixed to `MPI_COMM_WORLD` inside the shim, because that macro is a pointer under OpenMPI and an integer under MPICH. Link with `--cc mpicc`.

*Seen in: fortressc/tests/mpi_hello.fss:4-10*

### Traits and objects

```fortress
trait Top end
trait Left extends {Top} end     (* the brace form; bare `extends Top` is equally accepted *)
trait Animal
  noise(): ZZ32 = 0              (* a member may carry a default body *)
end
```

```fortress
trait Avl[\K,V\] comprises { AvlEmpty[\K,V\], AvlNode[\K,V\] }
(* parsed, recorded, never read: exclusion is decided closed-world from the concrete
   types the program declares. excludes is the same, and where {...} is discarded.
   The clause has to stay on the trait's OWN line. The corpus spelling puts it on a
   continuation line, Library/Avl.fss:16-17, and that is a parse error here:
   expected a field or method name, found KwComprises.
   `extends` and `excludes` on a continuation line fail the same way. *)
end
```

Traits have no run time representation: membership is a compile time fact about a concrete tag. Extending something that is not a trait, or a cycle, is a diagnostic.

*Seen in: fortressc/tests/ambiguous.fss:8-10, fortressc/tests/dottedmethod.fss:8-10, Library/Avl.fss:16-17*

```fortress
object Solid extends {Ink} end            (* NO parameter list at all: a SINGLETON *)
object Dotted(width: ZZ32) extends {Ink} end   (* a constructor. object O() ... end is NOT this *)
object Square(side: ZZ32) extends {Face}
   mark: String = "sq"                    (* a non-parameter field MUST have an initializer *)
end
s:Square = Square(5)
println(s.side)
println(Dotted(9).width)
```

An object is one scanned heap block: a 32-bit type tag at offset 0, fields from +8. Calling a singleton gives ``Marker` is a singleton object; write `Marker`, not `Marker(...)``. Singleton fields are computed once in declaration order before `run`, and one may not reach another singleton or a user function.

*Seen in: fortressc/tests/dispatch.fss:7-12, fortressc/tests/dispatch.fss:40-43*

```fortress
object b
  var x: ZZ32 = 0     (* [parses] the checker answers: `var x`: mutable fields are not implemented *)
end
```

`var` parses in member position and nowhere else. At component level it gives `expected a function name, found KwVar`, inside a `do` block `expected an expression, found KwVar`, and those are the dominant corpus spellings, so most of the 505 uses never reach the checker's refusal at all.

*Seen in: SpecData/examples/basic/Expr.Assign.a.fss:18-20*

### Methods

```fortress
trait Animal
  noise(): ZZ32 = 0
end
object Dog extends Animal
  noise(): ZZ32 = 1        (* the object's own method beats the default, by most-specific *)
end
object Point(x: ZZ32, y: ZZ32)
  sum(): ZZ32 = x + y      (* a body names constructor parameters directly, no self. prefix *)
end
speak(a: Animal): ZZ32 = a.noise()   (* trait-typed receiver, so this really dispatches *)
```

Dotted and functional methods have separate namespaces: `x.f(y)` is not `f(x, y)` and is not desugared into it. An object extending a trait whose member has no body, and providing none, is refused as `` no declaration of `noise` applies to (Rock) ``.

*Seen in: fortressc/tests/dottedmethod.fss:8-14, fortressc/tests/dottedmethod.fss:18-21, fortressc/tests/badabstract.fss:7-15*

```fortress
trait Shape
   area(self): ZZ32 = 0            (* a self parameter makes this a FUNCTIONAL method *)
end
object Square(side: ZZ32) extends Shape
   area(self): ZZ32 = side side    (* written area(s), NEVER s.area() *)
   scaled(k: ZZ32, self): ZZ32 = k side   (* self keeps whatever position the source gave it *)
end
area(n: ZZ32): ZZ32 = n + 100      (* an ordinary top-level member of the SAME overload set *)
```

A generic functional method parses and is refused: `foo[\T\](self, x: T): T = x` gives `` `foo` is a generic functional method; it parses, but a static argument on one cannot be resolved before the receiver has a type ``.

*Seen in: fortressc/tests/functionalmethod.fss:12-19, fortressc/tests/functionalmethod.fss:23, fortressc/tests/genericfunctional.fss:12*

```fortress
trait Shape
   getter size(): ZZ32          (* DECLARING an accessor compiles *)
   area(self, k: ZZ32): ZZ32
end
(*)    getter asString(): String = (BIG ||[i <- self] "," i)[1:]
```

The split is the point: declaring is fine, reading is not. Accessors change how a member is invoked (`x.f`, not `x.f()`), so they are recorded and left out of the dotted method sets; reading one gives `` `f` is a getter or setter; accessors parse but are not implemented ``  ⚠ 2026-08-23: reading one WORKS now. `getter` is heavy at 2310 uses in 233 files, `setter` is rare at 16 uses in 13 files.

*Seen in: fortressc/tests/selfgetter.fss:13-16, Library/GeneratorLibrary.fss:37*

### Symmetric multiple dispatch

```fortress
draw(i: Ink,    f: Face):   ZZ32 = 1000
draw(i: Solid,  f: Face):   ZZ32 = 2000     (* four overlapping declarations, one per cell *)
draw(i: Solid,  f: Round):  ZZ32 = 3000
draw(i: Dotted, f: Square): ZZ32 = 4000
```

Every tuple of concrete types reaching an overload set is enumerated and must have exactly one most-specific winner. That single computation is the ambiguity check, the dispatch table and the exhaustiveness proof; it flattens to a nested switch on tags with a direct call at every leaf, and statically concrete arguments never reach a switch.

```fortress
name(x: Solid): ZZ32 = 1
name(x: Ink): ZZ32 = 2
pick(n: ZZ32): Ink = if n === 0 then Solid else Dotted end
(* the static type at the call site is Ink, but the cell (Solid) still picks name(Solid):
   the decision belongs to the table, not to the static tuple *)

pick(x: Top,  y: Top):   ZZ32 = 0
pick(x: Left, y: Top):   ZZ32 = 1
pick(x: Top,  y: Right): ZZ32 = 2
(* REFUSED: both apply to (OL, OR) and neither is more specific *)
```

Two deliberate deviations from specification 1.0, both signed off: an ambiguous call is a compile error naming the tuple and both declarations, where 1.0 would pick one arbitrarily, and exclusion is closed-world. The ceiling is 1000000 cells, reported as `` the dispatch table for `X` would have N cells; narrow the parameter types ``.

*Seen in: fortressc/tests/dispatch.fss:16-19, fortressc/tests/specificity.fss:13-16, fortressc/tests/ambiguous.fss:15-17*

### Generics

```fortress
object Cell[\T\](held: T)
   twice: ZZ32 = 2
end
pick[\T\](a: T, b: T, first: Boolean): T = if first then a else b end
swap[\A, B\](x: A, y: B): B = y
object Pen[\T extends Ink\](tip: T) end   (* a bound; Pen[\Plain\] does not satisfy it *)
```

```fortress
i: Cell[\ZZ64\] = Cell[\ZZ64\](7)     (* stores an i64 in the object *)
s: Cell[\String\] = Cell[\String\]("hi")   (* stores a pointer: two layouts, no boxing *)
println(pick[\ZZ64\](3, 9, true))     (* static arguments are WRITTEN at every use site *)
println(swap[\ZZ32, String\](1, "second"))
```

Monomorphization, never erasure and never boxing. Static arguments are never inferred: naming a generic without them gives `` `X` is generic; write its static arguments, as in `X[\ZZ64\]`. They are never inferred ``. `nat`, `int`, `bool`, `opr`, `unit` and `dim` static parameters are refused at the parser. Only ASCII `[\ \]` lexes; the 79 Unicode `⟦ ⟧` pairs in 7 corpus files do not.

*Seen in: fortressc/tests/generics.fss:6-12, fortressc/tests/generics.fss:15-16,20,22, fortressc/tests/badbound.fss:9*

```fortress
object O extends T
   f[\S\](): ZZ32 = 1
   g[\S, U\](): ZZ32 = f[\S\]() + 5     (* a generic method calling one at its own parameter *)
end
println(O.f[\ZZ32\]())
println(pick(0).f[\ZZ32\]())            (* the receiver may be statically unknown *)
```

Expansion has no types, so it stamps a generic method at the written static arguments into every type declaring one of matching arity and lets dispatch choose by receiver. A stamp carrying a bound the argument cannot satisfy is pruned entirely rather than failing the component.

*Seen in: fortressc/tests/genericmethod.fss:17-20, fortressc/tests/genericmethod.fss:38,40,42, fortressc/tests/prunedstamp.fss:16-22*

```fortress
size[\T\](x: T): ZZ32 = 1
size(x: ZZ32): ZZ32 = 2      (* REFUSED: a set is uniformly generic or uniformly ground *)

tag[\T\](x: Red): ZZ32 = 1
tag[\T\](x: Blue): ZZ32 = 2  (* ACCEPTED: both generic, so one instantiation gives a ground set of 2 *)

deeper[\T\](x: T): ZZ32 = deeper[\Wrap[\T\]\](Wrap[\T\](x))
(* polymorphic recursion. REFUSED at 4096 instantiations, the total per component. *)
```

The uniformity rule kills candidate growth by construction. What the 4096 ceiling pins is not a wrong answer but a hang, so the failure is exit 1 with the limit named. Library/PureList.fss:137 is a real corpus occurrence of polymorphic recursion, and no monomorphizing compiler compiles it at any limit.

*Seen in: fortressc/tests/badoverload.fss:6-7, fortressc/tests/genericoverload.fss:10-11, fortressc/tests/polyrec.fss:10*

### The parallel for loop

```fortress
n: ZZ64 = 1000000
a: Array[\ZZ64\] = array(n)
for i <- 0#n do             (* half-open lo#count: the body is outlined and split across cores *)
   a[i] := i i + 7
end
for i <- seq(0#5000) do     (* seq(...) is a promise about ORDER, honoured at any range size *)
   println(i)
end
```

```fortress
    for k<-0:n1'-1 do       (* the INCLUSIVE generator lo:hi. Both forms collapse to [lo, hi) *)
```

`<-` is not a token: it is `Lt` glued to `Minus`, decided by span adjacency, and `seq` is recognised by spelling at the generator position rather than as a call. The index and the bounds are ZZ64 because array subscripts are. At run time, ranges under 4096, nested loops and `seq(...)` never reach the pool; the pool is min(nproc,16) and the calling thread takes chunk 0, so P-way parallelism costs P-1 threads.

*Seen in: fortressc/tests/parallelfill.fss:12-16, fortressc/tests/parallelseq.fss:9-11, ProjectFortress/demos/npbft.fss:66*

```fortress
total: ZZ64 := 0
for i <- 0#1000 do
   total := total + i    (* REFUSED: total is declared OUTSIDE the loop, so it is shared *)
end

a: Array[\ZZ64\] = array(1000)
for i <- 0#999 do
   a[i + 1] := i         (* REFUSED: a[i] is this iteration's slot, a[i+1] is somebody else's *)
end
```

The whole of the race freedom is one comparison: a name that resolves below the loop's own scope is shared. The array rule has two halves: the base must be loop-local, or the index must be the binder, because a loop-local array is fresh per iteration. Out of the subset, each with its own diagnostic: `atomic`, reduction variables, `at`, tuple binders, array generators (`for x <- a`), and a loop body with a value.

*Seen in: fortressc/tests/badparallelescape.fss:9-12, fortressc/tests/badparallelindex.fss:8-11*

### Literals, comments, continuation

```fortress
  assert(123'456'789, 123456789)   (* ' is a group separator and is stripped: same number *)
pi : FloatLiteral = 3.141592653589793 (* Double whose sin is closest to 0 *)
(* the float literal lexes; FloatLiteral is a legacy type name the rewrite does not know *)
  assert(9, 3&                     (* & continues the expression onto the next line *)
ZZ32_MIN: ZZ32 = 8000'0000_16      (* REFUSED: radix numerals are not in the M1 subset *)
```

There is no exponent syntax. String escapes are exactly b, t, n, f, r, `"`, `\` and the two curly double quotes, and a string never spans a line. Block comments `(* ... *)` nest; `(*)` opens a line comment. Every non-ASCII character outside those four quote characters is a hard lex error, and `a +b` and `a+ b` are not interchangeable.

*Seen in: ProjectFortress/tests/NumeralTest.fss:43, Library/Constants.fss:15, ProjectFortress/tests/ampersand.fss:19*

### What the parser takes and the checker refuses [parses]

```fortress
f(): (ZZ32, String) = 1     (* a tuple type is not implemented in this subset *)
x = (1, 2)                  (* a tuple expression is not implemented in this subset *)
f(): ZZ32 -> String = 1     (* an arrow type ...; -> is Minus glued to Gt, not a token *)
f(x: ()): ZZ32 = 1          (* () has no value, so it cannot be stored in a parameter *)
```

Each diagnostic names the construct so the file lands in its own blocker bucket instead of under a generic syntax error. `->` alone has 1691 corpus uses outside the `|->` maplet, so these four account for a large share of the 1676 corpus files that exit 1.

*Seen in: fortressc/tests/badtupletype.fss:4, fortressc/tests/badarrowtype.fss:4, fortressc/tests/badvoidparam.fss:4*

```fortress
run(): () = do
    isZero(x) = x = 0      (* REFUSED at the parser: declare it at component level *)
  end

noisy: ZZ32 = do println("INITIALIZER RAN"); 7 end
(* REFUSED at component level: there is no component initialization, so the body would never run.
   The identical syntax INSIDE a block is the ordinary immutable binding and compiles. *)
```

*Seen in: fortressc/tests/localfn.fss:4-7, fortressc/tests/badvaluebinding.fss:7*

### What does not reach the checker at all [legacy]

The lexer knows 90 reserved words. Twenty-two are acted on (`component`, `export`, `end`, `do`, `if`, `then`, `else`, `elif`, `while`, `api`, `trait`, `object`, `extends`, `comprises`, `excludes`, `where`, `var`, `import`, `except`, `self`, `getter`, `setter`), `true` and `false` are literals, `for` routes to the loop parser, and the rest give one identical message.

```text
BIG FORALL SI_unit Self Zilch absorbs abstract also asif at atomic bool case catch
coerce coerces contravariant covariant default dim dominates ensures exit finally
fn forbid goto grammar hidden idiom int invariant io label most nat native of opr
or override private property provided public pure reciprocal requires settable
spawn static syntax test throw throws try tryatomic type typecase typed unit value
widens with wrapped
```

```fortress
import Map.{...} except { opr BIG UNION }
(* opr is the largest refused word at 2640 uses / 228 files. Here it survives only because
   an import brace group is skipped wholesale. A live one gives:
   33..36: reserved word `opr` is not in the implemented subset *)
```

*Seen in: Library/Relation.fss:14*

```fortress
  for i <- seq(1 # (|x| - 1)) do result[i] += result[i-1] end
(* |x| lexes as Bar, expr, Bar and stops at `expected an expression, found Bar` *)
```

Enclosing operators and the literal syntaxes are tokenised so a file reaches the parser at all, but none has a production: `<|1, 2|>` gives `expected an expression, found LeftBar`, `{1, 2}` and `{1 |-> 2}` both give `expected an expression, found LBrace` (the brace pair lexes only because `extends {A, B}` needs it)  ⚠ 2026-08-23: both now PARSE and come back as `` unknown name `<|_|>` `` and `` unknown name `{_}` ``., and `7 // 2` gives `` expected `)`, found SlashSlash ``. `2 ** 3` is worse: a hard lex error before the parser ever sees it.

*Seen in: Library/CompilerLibrary.fss:450*


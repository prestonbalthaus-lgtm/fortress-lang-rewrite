# Ruling: juxtaposing a String CONCATENATES, it does not insert a space

`GenMet0`–`GenMet3` compile, link, run, exit 0 and print `bcat`. Their `.test`
record says `b cat`. The oracle gate counts those four as failures, and the
question was handed to the semantics lane as *"whether the fix is ours or the
expectation is a legacy quirk is group 3's call."*

**Ruling: ours is right. The expectation is a legacy quirk of one of the two 1.0
libraries, and the four files are a documented divergence rather than four
bugs.**

The program in question, `ProjectFortress/other_compiler_tests/GenMet0.fss`:

```
trait a
  m[\T extends String\](x:T) = println("a" x)
end
trait b extends a
  m[\T extends String\](x:T) = println("b" x)
end
object o extends b end
run():() = do
  o.m[\String\]("cat")
end
```

---

## The specification does not decide it

`Specification/basic/operators/juxtameaning.tex:103-110` says only that a
juxtaposition containing a `String` is a **static error** if any adjacent pair
has neither element a `String`, and that what remains

> is considered to be a binary or multifix application of the juxtaposition
> operator

So the meaning is whatever the **library** declares `opr juxtaposition` to be.
There is no rule in the specification proper to appeal to.

## The two 1.0 libraries declare it differently

```
Library/FortressLibrary.fss:4049
    opr juxtaposition(self, b:String):String = self || b

ProjectFortress/LibraryBuiltin/CompilerBuiltin.fss:384
    opr juxtaposition(self, b:Object): String = self ||| b
```

`||` is plain concatenation. `|||` is a *different operator*, and its body is on
disk — `ProjectFortress/src/com/sun/fortress/nativeHelpers/simpleConcatenate.java:20-24`:

```java
public static String nativeSmartConcatenate(String s1, String s2) {
    if (s1.length() == 0) return s2;
    else if (s2.length() == 0) return s1;
    else return s1 + " " + s2;
}
```

**A single space, unless either side is empty.** The interpreter library used
plain concatenation; the *compiler* library used the space-inserting one. The
`GenMet` tests declare `compile` / `link` / `run`, so they ran through the
compiler library and recorded `b cat`.

fortressc implements the compiler path, so on a naive reading of provenance the
legacy answer would be the space. That reading is wrong, and the corpus is why.

## The corpus was overwhelmingly written for plain concatenation

Counted over all 1956 corpus files, with comments stripped the way
`lexer/src/raw.rs` strips them (block comments nest, `(*)` is an inert line
comment), over parenthesised `println`/`print` calls containing a
`"literal" identifier` juxtaposition on one line, keywords excluded:

| how the literal ends | sites | reading it was written for |
|---|---|---|
| whitespace, including an escaped `\t` or `\n` | **247** | plain |
| abutting punctuation — `(`, `,`, `:`, `=`, `"` | **46** | plain |
| a word character | **22** | space-inserting |

**293 against 22 — and 18 of the 22 are the `GenMet`/`GenFun` family itself**,
which is the disputed evidence, not independent support for it. Outside that one
test family the entire corpus contains **four** space-style sites, and all four
are ambiguous: `println("y is" y)` (`Block1.fss`), `println("s" s)` and
`println("v" v)` (`conjGrad.fss`), and one literal inside `tree.fss`'s
`println("Legendre_" i "(" r[i] ...)` whose *very next* literal is abutting
punctuation.

What the space-inserting rule would do to the majority is the decisive part.
`ProjectFortress/.../ChunkedSparseArray.fss`:

```
println("FAIL: " d ": unexpected value " n " at " i)
```

reads correctly only under plain concatenation. Under the smart rule every one
of those separators doubles: `FAIL:  d :  unexpected value  n  at  i`. The same
happens to 292 other sites.

## The ruling, and what it costs

- **`opr juxtaposition` on `String` is plain concatenation.** `"b" x` with
  `x = "cat"` is `bcat`. That is what fortressc does and it does not change.
- **`GenMet0`, `GenMet1`, `GenMet2` and `GenMet3` are a DOCUMENTED DIVERGENCE**,
  and belong in the oracle gate's accepted-divergence record with this ruling
  attached — the same category `tools/oracle-accepted-must-fail.txt` already
  defines for a legacy static error against a rescoped feature. They are not
  four failures and they should not be counted as four failures.
- The cost is exactly those four recorded expectations, all from one family.
- **This forecloses nothing.** If a later milestone wants 1.0's compiler-library
  behaviour it is one function — `fn concatenation` in `crates/types/src/lib.rs`
  — plus a smart variant of `Target::Concat`. **The two empty-string carve-outs
  are load bearing**: without them `"" x` gains a leading space and every
  accumulator loop that starts from `""` is wrong.

## What this does NOT rule on

`|||` itself. The operator is real, it is declared in `CompilerBuiltin.fsi:38`,
and if it is ever implemented it must be the space-inserting one with both
carve-outs. The ruling here is only about what a **juxtaposition** means, which
is the thing `GenMet` exercises.

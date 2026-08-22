# The Generator protocol: every prerequisite EXISTS, and wiring it is worth ZERO files

**2026-08-22.** The brief asked for two things under "the Generator Protocol":
implement the core protocol, and wire up the lowering for array generators
(`for x <- a`). Measured, both answers are unexpected and neither is the work
that was priced.

## 1. `for x <- a` was already done

It is not a milestone, it is a passing test. `crates/types/src/lib.rs:5104`
desugars it into a `for` over `0 # length(a)` with the element bound inside,
and it runs:

```
a: ZZ32[3] = [10 20 30]
for x <- a do println(x) end        -->  10 20 30
```

It is RANK ONE only, refused by name above that -- 1.0 gives `Array2` a
per-dimension extent and no single length, so a total would invent a meaning
and an extent would pick a dimension. That refusal is deliberate and unchanged.

## 2. Every prerequisite for the PROTOCOL already exists

1.0's protocol is `Library/FortressLibrary.fsi:626`:

```
abstract generate[\R\](r: Reduction[\R\], body: E->R): R
```

The expectation going in -- recorded in `closure.rs`'s own header -- was that
this needed anonymous closures with captured environments, which that file says
it does not do. **That comment is STALE.** Probed with the compiler, one
construct at a time:

| construct | result |
|---|---|
| an arrow-typed parameter taking a NAMED function | compiles, runs, `8` |
| an anonymous `fn` with no capture | compiles, runs, `8` |
| an anonymous `fn` WITH a capture | compiles, runs, `12` (= 4 x 3, so the capture is real) |
| a generic method carrying its own `[\R\]` | compiles |
| a trait with an `abstract generate` and an object implementing it | compiles |

And the whole protocol composes. A hand-written generator, run end to end:

```
trait Gen  abstract generate(body: ZZ64->ZZ32): ZZ32  end
object Upto(n: ZZ32) extends Gen
  generate(body: ZZ64->ZZ32): ZZ32 = do
    var total: ZZ32 = 0
    for i <- seq(0#widen(n)) do total := total + body(i) end
    total
  end
end
run():()= do g: Gen = Upto(5); println(g.generate(fn(x:ZZ64):ZZ32 => 1)) end
```

prints `5`. **The protocol is expressible in the subset today.**

## 3. The ONE missing piece is worth ZERO corpus files

What does not work is the desugaring:

```
for x <- g do println(x) end
        ^  expected an array, found Gen
```

So `for` would have to dispatch to `g.generate(...)` when its source is not an
array. Measured over all 1956 files at `1c0bbfaa6`:

| | |
|---|---|
| first-blockers on `expected an array` | **2** |
| ...of which are generators | **ZERO** |

Both are something else. `Compiled1.k.fss:16` writes `(a^b[c])` -- subscripting
a SCALAR -- and `StringIndexing.fss` is in `long_term_not_working`. Nothing in
the corpus is waiting on this desugaring.

The categories that sound like they are waiting say the same thing, and `alone*`
is the number that says it:

| category | first | appears | alone* |
|---|---|---|---|
| generator-bindings | 21 | 25 | **0** |
| comprehensions-and-big | 13 | 112 | **0** |
| function-types | 9 | 257 | 10 |

`alone*` of ZERO means every one of those 25 files also uses something else
unimplemented. The ceiling on implementing generator bindings BY ITSELF is
nothing.

## 4. Where the real consumers are, and what they are actually behind

`unknown type Generator` is 13 first-blockers and `unknown type Reduction` is
12. Those files do not want a desugaring -- they want to IMPORT the library
that declares those traits. That is `Library/FortressLibrary.fsi`, which after
today's shadowing fix walks to :1730 and stops on **tuple types**.

**So the Generator milestone is downstream of tuples, not of closures.**

## 5. Why the desugaring is NOT being written anyway

It looks cheap and it is a trap. 1.0's `generate` takes TWO arguments and the
first is a `Reduction[\R\]` -- a monoid object with an identity and a join.
This subset has no `Reduction`. Wiring `for x <- g` today means either

* inventing a one-argument `generate(body)` that this compiler made up, which
  collides with the real signature the day the library becomes importable, or
* building `Reduction` first, which is the milestone.

For zero corpus files. The reduction machinery that exists (`TypedReduction`,
`fortress_reduction_alloc`) is a compiler-recognised SHAPE over ZZ32/ZZ64/RR64
accumulators, not a first-class object a program can pass, so it is not the
same thing wearing a different name.

**Recommendation: sequence generators after tuples, and after a first-class
`Reduction`.** Nothing is gained by doing it sooner, and a made-up protocol is
harder to remove than to not write.

## 6. One thing to fix while passing

`crates/types/src/closure.rs:22-27` still says the pass does not do "`fn`
syntax, so no anonymous closure and no captured environment". It does both --
measured above. The comment predates the feature and reading it is what made
this milestone look like it needed a BUILD.

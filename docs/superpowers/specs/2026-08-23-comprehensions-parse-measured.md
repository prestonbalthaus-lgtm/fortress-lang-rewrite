# Comprehensions parse, and the ceiling really is zero

**Date:** 2026-08-23. Phase C. **Result:** corpus 539 -> 539, zero lost. The
`List[\T\]` object is NOT built, and the reason is a number.

---

## What landed

`<| e | x <- g, p |>` and the same shape in every other bracket. 1.0 gives list,
set and map ONE production (`DelimitedExpr.rats:290-314`), so the node carries
the bracket pair as the operator NAME the declaration side already builds and
nothing here is list-specific. Verified on all four shapes plus tuple binders
and both range forms.

Three things the grammar insists on and this follows:

* **static arguments go INSIDE the opener** -- `<|[\E\] e | ... |>`. 471 corpus
  sites write one, and without it the family stopped at `expected an
  expression, found LGeneric`.
* **a guard is a generator clause with NO binder.** `Fortress.ast:1679` has no
  discriminator either; the two are told apart only by whether a `<-` was
  written.
* **the separator is a bare `|` with whitespace on both sides.**
  `DelimitedExpr.rats:298,306` write it `wr bar wr` and `Spacing.rats:93` makes
  the whitespace mandatory, so `<|x|x<-s|>` does not parse in 1.0 either.

`Symbol.rats:51-58` decides the separator with UNBOUNDED LOOKAHEAD -- a `|` is
one only if a whole generator clause list and a closer follow. The cheap test
here is the spacing rule plus a scan for a `<-` before the closing run, and the
scan is load bearing: `ps || <| ... |> || qs` is 160 corpus sites and takes the
wrong branch without it.

A comprehension generator takes the same range forms a `for` does. Without that,
`<| x | x <- 1:10 |>` parses the `1`, stops at the `:`, and the closing run
reports the wrong token.

## And the ceiling is what the mapping said

**ONE corpus file** now names the comprehension as its first blocker:
`not_working_static_tests/SetComprehension.fss`. The other 46 candidates are
blocked EARLIER -- `expected ], found Hash`, `unknown type`, `unknown name`,
`Gt`, `Lt`, `Reserved("asif")` -- exactly the 0-of-46 the read-only mapping
measured before any of this was written.

So the `List[\T\]` object -- a monomorphized generic collection, plus whatever
answers "nothing in this backend grows storage" without a second allocation path
-- would today clear ONE file. Route 4 remains the right design and the corpus
still wants it; what it does not have is a reason to be built before the walls
in front of it. Recorded, not built.

## What is now cheap that was not

The ceiling is DIRECTLY MEASURABLE for the first time. Every wall behind a
comprehension is one `--emit-obj` away instead of behind a placeholder rewrite
that corrupted two files the last time it was attempted. When the walls in front
come down, re-run the sweep and the number will be a fact rather than an
estimate.

# `true : Boolean` is a parse error, and it cost five apis

**Date:** 2026-08-23. **Result:** corpus 515 -> 520. `API_FLOOR` 120 -> 125.

`CompilerLibrary/FortressLibrary.fsi:2550-2551` declares values named `true` and
`false`. Both are 1.0 reserved words -- `Keyword.rats:49`, with a
`transient String` production each at `:156-157` -- so neither can name a
declaration.

**1.0's own team already made this correction in the sibling file.**
`Library/FortressLibrary.fsi:2584-2585` is the same two lines with the same
`(*)` in front of them. The `CompilerLibrary/` copy was missed.

## Why two lines cost five files

It is a PARSE error, and an api that does not parse is `unreadable` to the
resolver -- it merges **nothing**. So every `CompilerLibrary/` api that
implicitly imports this one lost every name it declares, while reporting a core
type that is declared in the very file it could not read:

```
CompilerLibrary/List.fsi     unknown type `LexicographicOrder`
CompilerLibrary/Map.fsi      unknown type `ZeroIndexed`
CompilerLibrary/Pairs.fsi    unknown type `LexicographicOrder`
CompilerLibrary/Set.fsi      unknown type `MonoidReduction`
CompilerLibrary/System.fsi   unknown type `ImmutableArray`
```

All four of those names ARE declared in `CompilerLibrary/FortressLibrary.fsi`.
A diagnostic that names a missing type is not always about the type.

## What holds it

The api floor, which is the only thing that can reach corpus source, and it was
proved by refusal: with the two lines put back apply-gate reports
`120 corpus .fsi files check / floor is 125` and goes 50/1.

Found while investigating `unknown type Generator` in `Library/Reader.fss`,
which is a different cause entirely -- see
`2026-08-23-component-side-core-import-measured.md`.

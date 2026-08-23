# 1.0's own collision matrix, and the call it does not decide

**Date:** 2026-08-23.
**Result:** `Library/File.fsi` and `CompilerLibrary/File.fsi` CHECK. Corpus
505 -> 507, zero lost. `API_FLOOR` 114 -> 116.

---

## The rule we had was not a rule 1.0 has

`build_signatures` refused a top-level function whose name appeared anywhere in
the TYPE namespace:

```rust
if declared.contains_key(intern(&f.name)) {
    return Err(TypeError::DuplicateDefinition { .. });
}
```

`declared` is traits and objects together, so this refused all three of

* a function beside a **trait** of its name,
* a function beside a **singleton object** of its name,
* a function beside an **object constructor** of its name.

`Library/File.fsi:16-18` is the third:

```
FileReadStream(filename: String): FileReadStream

object FileReadStream(filename:String) extends { FileStream, ReadStream }
```

## The oracle was already in the corpus, in a comment

`ProjectFortress/compiler_tests/Compiled9.c.fss` opens with an eleven-by-eleven
matrix of which kinds of declaration may share a name, and every body in the
file is labelled with the cell it exercises -- `(* 5-3 *)`, `(* 2-1 *)`. Row 5
is the top-level function:

```
                       | 1| 2| 3| 4| 5| 6| 7| 8| 9|10|11|
 5) top-level function | Y| N| Y| N| Y
```

| cell | with                | verdict | the file's own body                |
|------|---------------------|---------|------------------------------------|
| 5-1  | trait               | **Y**   | `s() = ()` under `trait s`         |
| 5-2  | singleton object    | **N**   | `o() = ()` under `object o`        |
| 5-3  | object constructor  | **Y**   | `q() = ()` under `object q(x:String)` |
| 5-4  | top-level variable  | **N**   | `v() = ()` under `v = 2`           |
| 5-5  | top-level function  | **Y**   | `g() = ()` under `g(z:ZZ32)`       |

Two more witnesses agree and neither is a reading of a comment.
`ProjectFortress/tests/OverloadConstructor1-3.fss` are 1.0's POSITIVE tests for
5-3 -- `object Thing(x:ZZ32)` beside `Thing():Thing = Thing(0)`. And
`Library/File.fss:20` writes the pair with **different parameter types**: a
`String` factory that converts and calls the real `FlatString` constructor. The
api hides `FlatString` and writes both at `String`, which is why the api copy
looks like a duplicate and the component copy does not.

The distinction the matrix draws is the one the value namespace draws. A trait
puts no name in it at all. A constructor puts one that a call could still tell
apart by its arguments. A singleton puts a **value** of that name there
outright, and there is nothing left for a function to overload against. The
rule narrows to `registry.is_singleton`, and `declared` stops being threaded
into `build_signatures` because that was its only remaining reader.

---

## 5-3 IS LEGAL TO DECLARE AND IS STILL REFUSED TO CALL

Accepting the declaration is not the same as being able to compile a call, and
this compiler cannot compile one. `call` reaches `construct` **by name**:

```rust
_ if self.registry.is_object(name) => self.construct(...),
_ => /* the overload set */
```

The constructor arm is above the overload set and the two never meet, so a
constructor and a function of one name would not tie -- the constructor would
take **every** call and the function would be silently unreachable.
`object O(x: Any)` beside `O(x: ZZ32)` runs the wrong declaration, exits 0 and
prints the wrong number. That is a wrong answer, not a missing feature.

`ConstructorOverloadUnsupported` refuses that call by name. **An api has no
calls**, which is the whole reason `File.fsi` checks anyway, and it is what
separates the deliverable from the milestone: putting a constructor into
`self.functions` as a `Signature` -- with a mangled symbol, a dispatch row and
an exhaustiveness obligation -- is its own piece of work.

What moved and did not start passing, from the sweep:

```
Compiled9.b.fss          `B` is defined twice          -> `AND` takes Boolean operands
OverloadConstructor1     `Thing` is defined twice      -> both a constructor and a function
OverloadConstructor2/3   `Thing` is defined twice      -> declarations differ in their static parameters
```

## What holds it

* `ctorfn.fsi` and `ctorfnrev.fsi` -- 5-3 accepted, **both orders**. A fixture
  that writes the function first only tests the order the shipped library
  happens to use.
* `traitfn.fss` -- 5-1, built and RUN, prints 42.
* `badsingletonfn.fss` -- 5-2, still `is defined twice`.
* `badctorcall.fss` -- the call, refused by its own message.
* Three mutation rows: put the over-broad rule back, drop the singleton cell,
  and let the constructor take the call silently. Each caught by exactly the
  assertion that speaks for it, 24/0 on the table.

api implicitcore

(* `basic/components/source-code.tex:305` -- "Every component implicitly
   imports the Fortress core APIs". `Maybe`, `ZeroIndexed` and `TotalComparison`
   are `FortressLibrary`'s and NO IMPORT IS WRITTEN HERE. `RR32` is the
   builtin's, and BOTH layers must arrive: the two core apis are ordered, and a
   core api implicitly imports only the ones below it. *)
f(x: Maybe[\ZZ32\]): TotalComparison
g(x: ZeroIndexed[\Char\]): RR32

end

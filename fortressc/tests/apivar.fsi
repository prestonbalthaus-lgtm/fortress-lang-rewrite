api apivar

(* `variables.tex:176-179` -- an api declares a variable with no initialiser,
   and `var` makes it mutable. `Library/String.fsi:43` is the one that matters:
   `var maxLeafSize: ZZ32`, with the comment "Clients may assign to this
   variable". *)
var maxLeafSize: ZZ32
newline: String

f(x: ZZ32): ZZ32

end

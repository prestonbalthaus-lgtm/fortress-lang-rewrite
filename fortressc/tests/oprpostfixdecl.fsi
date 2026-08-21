api oprpostfixdecl

(* A POSTFIX declaration has no trailing parameter list: the leading operand is
   its only one. Library/FortressLibrary.fsi:2171 is this shape. A postfix
   operator in EXPRESSION position is still refused; this is the declaration. *)
trait I end
opr (x:I)#[\J\] : I
opr (x:I)// : I

end

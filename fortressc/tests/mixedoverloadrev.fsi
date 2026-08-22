api mixedoverloadrev

(* THE SAME PAIR AS `mixedoverload.fsi`, WRITTEN THE OTHER WAY ROUND, and it
   exists because the mutation table found that one file could not see the
   difference. DEV-15 asks whether BOTH declarations are bodiless; a relaxation
   that asks only about the one IN HAND meets the bodied declaration second in
   one order and first in the other.

   AND WHAT SEPARATES THEM IS THE DIAGNOSTIC, NOT THE EXIT CODE. A body in an
   api is refused by a second rule whichever way this is written, so a gate
   asserting `exit 1` passes on both readings and says nothing. The uniformity
   message is what says the pair was compared. *)
size(x: ZZ32): ZZ32 = 2
size[\T\](x: T): ZZ32

end

api mixedoverloadrev

(* THE SAME PAIR AS `mixedoverload.fsi`, WRITTEN THE OTHER WAY ROUND, and it
   exists because the mutation table found that one file could not see the
   difference. DEV-15 asks whether BOTH declarations are bodiless; a relaxation
   that asks only about the one in hand is refused by this order and accepted by
   that one, so it takes both files to pin the rule. *)
size(x: ZZ32): ZZ32 = 2
size[\T\](x: T): ZZ32

end

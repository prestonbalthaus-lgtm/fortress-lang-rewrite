api mixedoverload

(* DEV-15 relaxes the uniformity rule for a pair of BODILESS declarations. This
   pair is not one: `size(x: ZZ32)` carries a body, so it can be CALLED, and a
   call needs an overload set whose members agree on how many static arguments
   they take. It must stay REFUSED. *)
size[\T\](x: T): ZZ32
size(x: ZZ32): ZZ32 = 2

end

api badvarargstwice

(* TWO varargs parameters, caught by the same rule: the first has a parameter
   after it. `Parameter.rats:33-34` admits one `Varargs` and no more.

   IT IS AN api ON PURPOSE. The position rule is GRAMMATICAL, so unlike
   `reject_elided_name` it is NOT gated on the body and holds here exactly as
   it holds in a component. A varargs parameter is still RECORDED AND NOT
   LOWERED in a bodiless declaration -- that is what keeps the fifteen library
   apis that write `Any...` compiling -- but a list the grammar does not admit
   is refused on either side. *)
g(a: ZZ32..., b: ZZ32...): ZZ32

end

api scopedarityapi

(* THIS api's OWN REFERENCE TO ITS OWN DECLARATION. `AndZeroed` names `Zeroed`
   at TWO static arguments because that is what this file declares. An importer
   that declares a one-parameter `Zeroed` of its own must not re-point this
   line at it: no substitution makes the two declarations one. *)

trait Zeroed[\R, L\] end

object AndZeroed extends { Zeroed[\ZZ32, ZZ32\] } end

(* AND A STATIC PARAMETER OF THAT NAME SHADOWS IT. `Shadower`'s `Zeroed` is a
   TYPE VARIABLE, not this file's trait, so the rewrite must leave it alone.
   Rewriting it spells the parameter one way and its uses another, and the use
   then resolves to a four-argument trait named with none: `is generic; write
   its static arguments`. *)

trait Shadower[\Zeroed\]
    getter held(): Zeroed
end

object UsesShadower extends { Shadower[\ZZ32\] } end

end

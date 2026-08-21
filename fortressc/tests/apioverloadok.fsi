api apioverloadok

trait T end
object O extends T end

(* Different arity, and a strictly more specific pair: both are unambiguous and
   must stay accepted -- the rule is not "refuse every overloaded api". *)
g(): ()
g(x: T): ()
g(x: O): ()

end

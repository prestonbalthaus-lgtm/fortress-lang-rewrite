api badapioverload

trait T end
object O extends T end

(* overloading.tex -- ambiguous at (O, O): neither is more specific. An api has
   no call sites, so M3c's call-driven check never sees this. *)
f(x: O, y: T): ()
f(x: T, y: O): ()

end

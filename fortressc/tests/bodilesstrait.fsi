api bodilesstrait

(* A TRAIT IS NEVER A SIGNATURE for DEV-15's purposes. Its name is written in
   TYPE position, which is demand, so expansion can meet a set whose members
   disagree on static arity. Refused inside an api exactly as inside a
   component. *)
trait Holder[\T\] end
trait Holder end

end

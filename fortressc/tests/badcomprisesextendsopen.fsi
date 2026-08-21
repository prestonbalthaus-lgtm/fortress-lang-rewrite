api badcomprisesextendsopen

(* traits.tex:236-241 -- the api may not declare what extends its own open
   comprises trait. *)
trait T comprises { ... } end
trait S extends T end

end

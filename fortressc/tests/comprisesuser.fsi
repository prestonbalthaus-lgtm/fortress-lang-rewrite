api comprisesuser

(* THE MERGED CLAUSE IS NOT THIS FILE'S TO ANSWER FOR. `comprisesapi` says
   `Owner comprises { Sub }` and means ITS `Sub`; this file declares an
   unrelated one, and the resolver deliberately lets this file's declaration win
   the name. Reporting the api's clause against this file names two types that
   have nothing to do with each other. *)
import comprisesapi.{Owner}

object Sub end

end

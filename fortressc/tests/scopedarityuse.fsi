api scopedarityuse

(* THE IMPORTER WINS ITS OWN NAME AND NOT THE api's REFERENCES. `Zeroed` here
   takes ONE static parameter and `MyZeroed` means THIS one; `scopedarityapi`'s
   `AndZeroed` still means the two-parameter one it was declared against.
   Without that split the merged `AndZeroed` reads `Zeroed takes 1 static
   argument(s), found 2` -- which is `Library/GeneratorLibrary.fsi` against
   `FortressLibrary.fsi:1871`, in six lines. *)

import scopedarityapi.{ ... }

trait Zeroed[\R\] end

object MyZeroed extends { Zeroed[\ZZ32\] } end

end

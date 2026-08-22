api staticcomprises

(* `Library/CompilerAlgebra.fsi:24` writes `trait Equality[\T\] comprises T`,
   where `T` is the STATIC PARAMETER. This file declares an unrelated `trait T`,
   which is exactly the collision that made the implicit import report the two
   against each other. *)
trait Equality[\T\] comprises T end

trait T end

end

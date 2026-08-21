api openapicomprises

(* THIS API IS ILL FORMED ON PURPOSE and is refused when compiled on its own:
   traits.tex:236-241 says an api may not declare a trait that extends one of
   its own open-comprises traits. `Library/FortressLibrary.fsi` has exactly this
   shape at six sites, which is why the fixture exists. *)
trait Q comprises { ... } end
trait R extends { Q } end

end

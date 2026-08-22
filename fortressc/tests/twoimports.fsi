api twoimports

(* TWO REQUESTS FOR ONE api AT DIFFERENT NAME SETS. This file asks `twonames`
   for `Beta`; `viaalpha` asks the same api for `Alpha`. With the api NAME alone
   as the resolver's key the first request seen decided and the other was
   silently dropped, leaving one of the two names unknown. *)
import twonames.{Beta}
import viaalpha.{UsesAlpha}

trait X extends { Beta, UsesAlpha } end

end

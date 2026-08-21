api oprsubscriptdecl

(* subscripting.tex:44-54. All four spacings the corpus writes, multiple
   indices, an `_` index name, and the `abstract` prefix -- one per trait,
   because `[_]:=` is ONE member name and declaring it twice in one trait is
   correctly a duplicate. Declarations only: `a[i]` on a user object is NOT
   dispatched yet, which is a checker item and not this one. *)
trait A
  abstract opr[i:ZZ32]:=(v:ZZ32) : ()
end
trait B
  opr [i: ZZ32] := (v: ZZ32): ()
end
trait C
  opr[x:ZZ32,y:ZZ32] := (v:ZZ32):()
end
trait D
  opr[_:ZZ32]:=(v:ZZ32):()
end
(* the GET and the SET coexist: two different members of one trait *)
trait E
  opr [i: ZZ32]: ZZ32
  opr [i: ZZ32] := (v: ZZ32): ()
end

end

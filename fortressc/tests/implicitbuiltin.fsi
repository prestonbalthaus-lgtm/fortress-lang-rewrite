api implicitbuiltin

(* NO IMPORT IS WRITTEN. `library/structure.tex:16-18`: the default libraries
   "are automatically imported by every Fortress component and API". `RR32` is
   declared in `ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi:443` and
   nothing here names that file. *)
narrow(x: RR64): RR32

end

api condunit

(* `ProjectFortress/LibraryBuiltin/CompilerBuiltin.fsi:453` in ten lines:
   a trait whose static parameter is instantiated at `()`, whose members then
   take a `()` parameter and return one. The gate reproduces the shape here
   rather than compiling the 754-line library, so a failure names the feature
   instead of naming a file. *)
trait Cond[\E extends Any\]
    abstract getDefault(defaultValue: E): E
    abstract loop(f: E -> ()): ()
end

trait Bool extends { Cond[\()\] } end

end

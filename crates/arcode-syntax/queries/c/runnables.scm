; C / C++ runnables for Arcode. Conventions: see `src/runnables.rs`.

; `int main(...)` at file level.
((translation_unit
  (function_definition
    type: (primitive_type) @_type
    declarator: (function_declarator
      declarator: (identifier) @name)) @run)
  (#eq? @_type "int")
  (#eq? @name "main")
  (#set! kind "main"))

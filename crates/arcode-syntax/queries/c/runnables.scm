; C / C++ runnables for Arcode. Conventions: see `src/runnables.rs`.

; `int main(...)` at file level, also inside `#if`/`#ifdef` blocks.
([
  (translation_unit
    (function_definition
      type: (primitive_type) @_type
      declarator: (function_declarator
        declarator: (identifier) @name)) @run)
  (preproc_if
    (function_definition
      type: (primitive_type) @_type
      declarator: (function_declarator
        declarator: (identifier) @name)) @run)
  (preproc_ifdef
    (function_definition
      type: (primitive_type) @_type
      declarator: (function_declarator
        declarator: (identifier) @name)) @run)
  (preproc_else
    (function_definition
      type: (primitive_type) @_type
      declarator: (function_declarator
        declarator: (identifier) @name)) @run)
  (preproc_elif
    (function_definition
      type: (primitive_type) @_type
      declarator: (function_declarator
        declarator: (identifier) @name)) @run)
]
  (#eq? @_type "int")
  (#eq? @name "main")
  (#set! kind "main"))

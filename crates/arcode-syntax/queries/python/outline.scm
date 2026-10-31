; Python outline for Arcode. Conventions: see `src/outline.rs`.
; General patterns first: for the same `@item` the later pattern wins.

((function_definition
  name: (identifier) @name) @item
  (#set! kind "function"))
((class_definition
  name: (identifier) @name) @item
  (#set! kind "class"))

; Functions directly in a class body are methods.
((class_definition
  body: (block
    [
      (function_definition
        name: (identifier) @name) @item
      (decorated_definition
        definition: (function_definition
          name: (identifier) @name) @item)
    ]))
  (#set! kind "method"))

; Module-level UPPER_CASE assignments are constants.
((module
  (expression_statement
    (assignment
      left: (identifier) @name) @item))
  (#match? @name "^_*[A-Z][A-Z\\d_]*$")
  (#set! kind "const"))

; JavaScript outline for Arcode (also the base of the TypeScript outline).
; Conventions: see `src/outline.rs`. For the same `@item` the later pattern wins.

((function_declaration
  name: (identifier) @name) @item
  (#set! kind "function"))
((generator_function_declaration
  name: (identifier) @name) @item
  (#set! kind "function"))
((class_declaration
  name: (_) @name) @item
  (#set! kind "class"))
((method_definition
  name: (_) @name) @item
  (#set! kind "method"))
((method_definition
  name: (property_identifier) @name) @item
  (#eq? @name "constructor")
  (#set! kind "constructor"))

; Top-level (or exported) `const f = () => ...` / `function () {}`.
((program
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: [
        (arrow_function)
        (function_expression)
      ]) @item))
  (#set! kind "function"))
((export_statement
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: [
        (arrow_function)
        (function_expression)
      ]) @item))
  (#set! kind "function"))

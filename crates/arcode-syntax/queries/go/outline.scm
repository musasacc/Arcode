; Go outline for Arcode. Conventions: see `src/outline.rs`.
; General patterns first: for the same `@item` the later pattern wins.

((function_declaration
  name: (identifier) @name) @item
  (#set! kind "function"))

; Methods: "Receiver.Method" when the receiver type is a plain name.
((method_declaration
  name: (field_identifier) @name) @item
  (#set! kind "method"))
((method_declaration
  receiver: (parameter_list
    (parameter_declaration
      type: [
        (type_identifier) @name
        (pointer_type
          (type_identifier) @name)
        (generic_type
          type: (type_identifier) @name)
        (pointer_type
          (generic_type
            type: (type_identifier) @name))
      ]))
  name: (field_identifier) @name) @item
  (#set! kind "method")
  (#set! join "."))

((type_spec
  name: (type_identifier) @name) @item
  (#set! kind "type"))
((type_alias
  name: (type_identifier) @name) @item
  (#set! kind "type"))
((type_spec
  name: (type_identifier) @name
  type: (struct_type)) @item
  (#set! kind "struct"))
((type_spec
  name: (type_identifier) @name
  type: (interface_type)) @item
  (#set! kind "interface"))
((method_elem
  name: (field_identifier) @name) @item
  (#set! kind "method"))

; Package-level constants. `const a, b = 1, 2` lists `a` and `b`: without
; the `name:` field tree-sitter yields one match per name identifier (with
; it, only the first name matches).
((source_file
  (const_declaration
    (const_spec
      (identifier) @name) @item))
  (#set! kind "const"))

; Rust outline for Arcode. Conventions: see `src/outline.rs`.
; General patterns first: for the same `@item` the later pattern wins.

((function_item
  name: (identifier) @name) @item
  (#set! kind "function"))
((function_signature_item
  name: (identifier) @name) @item
  (#set! kind "function"))

((struct_item
  name: (type_identifier) @name) @item
  (#set! kind "struct"))
((union_item
  name: (type_identifier) @name) @item
  (#set! kind "struct"))
((enum_item
  name: (type_identifier) @name) @item
  (#set! kind "enum"))
((trait_item
  name: (type_identifier) @name) @item
  (#set! kind "trait"))
((type_item
  name: (type_identifier) @name) @item
  (#set! kind "type"))
((mod_item
  name: (identifier) @name) @item
  (#set! kind "module"))
((const_item
  name: (identifier) @name) @item
  (#set! kind "const"))
((static_item
  name: (identifier) @name) @item
  (#set! kind "const"))
((macro_definition
  name: (identifier) @name) @item
  (#set! kind "macro"))

; `impl Point` -> "Point"; `impl Display for Point` -> "Display for Point".
((impl_item
  !trait
  type: (_) @name) @item
  (#set! kind "impl"))
((impl_item
  trait: (_) @name
  type: (_) @name) @item
  (#set! kind "impl")
  (#set! join " for "))

; Functions in impls and traits are methods.
((impl_item
  body: (declaration_list
    (function_item
      name: (identifier) @name) @item))
  (#set! kind "method"))
((trait_item
  body: (declaration_list
    [
      (function_item
        name: (identifier) @name)
      (function_signature_item
        name: (identifier) @name)
    ] @item))
  (#set! kind "method"))

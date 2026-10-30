; Java outline for Arcode. Conventions: see `src/outline.rs`.

((class_declaration
  name: (identifier) @name) @item
  (#set! kind "class"))
((record_declaration
  name: (identifier) @name) @item
  (#set! kind "class"))
((interface_declaration
  name: (identifier) @name) @item
  (#set! kind "interface"))
((annotation_type_declaration
  name: (identifier) @name) @item
  (#set! kind "interface"))
((enum_declaration
  name: (identifier) @name) @item
  (#set! kind "enum"))
((method_declaration
  name: (identifier) @name) @item
  (#set! kind "method"))
((constructor_declaration
  name: (identifier) @name) @item
  (#set! kind "constructor"))


; TypeScript outline (appended to the JavaScript outline).

((function_signature
  name: (identifier) @name) @item
  (#set! kind "function"))
((abstract_class_declaration
  name: (type_identifier) @name) @item
  (#set! kind "class"))
((interface_declaration
  name: (type_identifier) @name) @item
  (#set! kind "interface"))
((type_alias_declaration
  name: (type_identifier) @name) @item
  (#set! kind "type"))
((enum_declaration
  name: (identifier) @name) @item
  (#set! kind "enum"))
((internal_module
  name: (_) @name) @item
  (#set! kind "module"))
((module
  name: (_) @name) @item
  (#set! kind "module"))
((method_signature
  name: (_) @name) @item
  (#set! kind "method"))
((abstract_method_signature
  name: (_) @name) @item
  (#set! kind "method"))

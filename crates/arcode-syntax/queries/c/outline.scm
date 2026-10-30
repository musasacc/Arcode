; C outline for Arcode (also the base of the C++ outline).
; Conventions: see `src/outline.rs`. For the same `@item` the later pattern wins.

((function_definition
  declarator: (function_declarator
    declarator: (identifier) @name)) @item
  (#set! kind "function"))
((function_definition
  declarator: (pointer_declarator
    declarator: (function_declarator
      declarator: (identifier) @name))) @item
  (#set! kind "function"))
((function_definition
  declarator: (pointer_declarator
    declarator: (pointer_declarator
      declarator: (function_declarator
        declarator: (identifier) @name)))) @item
  (#set! kind "function"))
((function_definition
  declarator: (pointer_declarator
    declarator: (pointer_declarator
      declarator: (pointer_declarator
        declarator: (function_declarator
          declarator: (identifier) @name))))) @item
  (#set! kind "function"))
; Functions returning a function pointer: `int (*getf(void))(int)`.
((function_definition
  declarator: (function_declarator
    declarator: (parenthesized_declarator
      (pointer_declarator
        declarator: (function_declarator
          declarator: (identifier) @name))))) @item
  (#set! kind "function"))

((struct_specifier
  name: (type_identifier) @name
  body: (field_declaration_list)) @item
  (#set! kind "struct"))
((union_specifier
  name: (type_identifier) @name
  body: (field_declaration_list)) @item
  (#set! kind "struct"))
((enum_specifier
  name: (type_identifier) @name
  body: (enumerator_list)) @item
  (#set! kind "enum"))
((type_definition
  declarator: (type_identifier) @name) @item
  (#set! kind "type"))
((preproc_function_def
  name: (identifier) @name) @item
  (#set! kind "macro"))

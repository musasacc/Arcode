
; C++ outline (appended to the C outline).

((namespace_definition
  name: (_) @name) @item
  (#set! kind "module"))
((class_specifier
  name: (_) @name
  body: (field_declaration_list)) @item
  (#set! kind "class"))
((struct_specifier
  name: (template_type) @name
  body: (field_declaration_list)) @item
  (#set! kind "struct"))
((alias_declaration
  name: (type_identifier) @name) @item
  (#set! kind "type"))

; Out-of-class definitions `void Point::move() {}` are methods.
((function_definition
  declarator: (function_declarator
    declarator: (qualified_identifier) @name)) @item
  (#set! kind "method"))
((function_definition
  declarator: (reference_declarator
    (function_declarator
      declarator: (qualified_identifier) @name))) @item
  (#set! kind "method"))
((function_definition
  declarator: (pointer_declarator
    declarator: (function_declarator
      declarator: (qualified_identifier) @name))) @item
  (#set! kind "method"))

((function_definition
  declarator: (pointer_declarator
    declarator: (pointer_declarator
      declarator: (function_declarator
        declarator: (qualified_identifier) @name)))) @item
  (#set! kind "method"))

; GoogleTest `TEST(Suite, Name) { ... }` is named `Suite.Name`, not `TEST`.
((function_definition
  declarator: (function_declarator
    declarator: (identifier) @_macro
    parameters: (parameter_list
      .
      (parameter_declaration
        type: (type_identifier) @name)
      .
      (parameter_declaration
        type: (type_identifier) @name)
      .))) @item
  (#any-of? @_macro "TEST" "TEST_F" "TEST_P" "TYPED_TEST" "TYPED_TEST_P")
  (#set! kind "function")
  (#set! join "."))

; Member functions defined or declared in a class body.
((field_declaration_list
  (function_definition
    declarator: (function_declarator
      declarator: [
        (field_identifier)
        (identifier)
        (destructor_name)
        (operator_name)
      ] @name)) @item)
  (#set! kind "method"))
((field_declaration_list
  [
    (field_declaration
      declarator: (function_declarator
        declarator: [
          (field_identifier)
          (operator_name)
        ] @name))
    (declaration
      declarator: (function_declarator
        declarator: [
          (identifier)
          (destructor_name)
        ] @name))
  ] @item)
  (#set! kind "method"))

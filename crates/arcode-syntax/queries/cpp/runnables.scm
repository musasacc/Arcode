
; GoogleTest `TEST(Suite, Name)`, `TEST_F(Fixture, Name)`, ...: the name is
; `Suite.Name` (a `--gtest_filter` pattern).
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
      .))) @run
  (#any-of? @_macro "TEST" "TEST_F" "TEST_P" "TYPED_TEST" "TYPED_TEST_P")
  (#set! kind "test")
  (#set! join "."))


; C++ (appended to the C query, adapted from tree-sitter-cpp, MIT) -------------

(namespace_identifier) @module
((namespace_identifier) @type
  (#match? @type "^[A-Z]"))
(auto) @type.builtin

(call_expression
  function: (qualified_identifier
    name: (identifier) @function))
(call_expression
  function: (field_expression
    field: (field_identifier) @function.method))
(template_function
  name: (identifier) @function)
(template_method
  name: (field_identifier) @function.method)
(function_declarator
  declarator: (qualified_identifier
    name: (identifier) @function))
(function_declarator
  declarator: (field_identifier) @function.method)
(function_declarator
  declarator: (qualified_identifier
    name: (qualified_identifier
      name: (identifier) @function)))

(this) @variable.builtin
(null
  "nullptr" @constant.builtin)
(raw_string_literal) @string

[
  "::" 
] @punctuation.delimiter

[
  "class" "co_await" "co_return" "co_yield" "concept" "constexpr" "consteval"
  "constinit" "decltype" "delete" "explicit" "final" "friend" "mutable"
  "namespace" "new" "noexcept" "operator" "override" "private" "protected"
  "public" "requires" "template" "typename" "using" "virtual"
] @keyword

[
  "catch" "throw" "try"
] @keyword.control

(comment) @comment

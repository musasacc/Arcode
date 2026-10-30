; PHP highlights for Arcode (adapted from tree-sitter-php, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Embedded HTML outside `<?php ... ?>` is not highlighted (no injections).

; Identifiers -----------------------------------------------------------------

(variable_name) @variable
((name) @constant
  (#match? @constant "^_?[A-Z][A-Z\\d_]+$"))
((name) @constant.builtin
  (#match? @constant.builtin "^__[A-Z][A-Z\\d_]+__$"))
(const_declaration
  (const_element
    (name) @constant))
((variable_name
  (name) @variable.builtin)
  (#eq? @variable.builtin "this"))
(relative_scope) @variable.builtin

(namespace_definition
  name: (namespace_name
    (name) @module))
(namespace_name
  (name) @module)

(primitive_type) @type.builtin
(cast_type) @type.builtin
(named_type
  [
    (name) @type
    (qualified_name (name) @type)
  ])
(named_type
  (name) @type.builtin
  (#any-of? @type.builtin "static" "self"))
(class_declaration
  name: (name) @type)
(interface_declaration
  name: (name) @type)
(trait_declaration
  name: (name) @type)
(enum_declaration
  name: (name) @type)
(scoped_call_expression
  scope: [
    (name) @type
    (qualified_name (name) @type)
  ])
(class_constant_access_expression
  .
  (name) @type)

(property_element
  (variable_name) @property)
(member_access_expression
  name: (name) @property)
(member_access_expression
  name: (variable_name (name)) @property)
(simple_parameter
  name: (variable_name) @variable.parameter)

; Functions -------------------------------------------------------------------

(function_definition
  name: (name) @function)
(method_declaration
  name: (name) @function.method)
(method_declaration
  name: (name) @constructor
  (#eq? @constructor "__construct"))
(function_call_expression
  function: [
    (qualified_name (name))
    (name)
  ] @function)
(scoped_call_expression
  name: (name) @function)
(member_call_expression
  name: (name) @function.method)
(object_creation_expression
  [
    (name) @constructor
    (qualified_name (name) @constructor)
  ])
(attribute
  (name) @attribute)

; Literals --------------------------------------------------------------------

[
  (string)
  (string_content)
  (encapsed_string)
  (heredoc)
  (heredoc_body)
  (nowdoc_body)
] @string
(escape_sequence) @string.escape
(boolean) @boolean
(null) @constant.builtin
[
  (integer)
  (float)
] @number

; Punctuation and operators ---------------------------------------------------

[
  "=" "==" "===" "!=" "!==" "<>" "<" ">" "<=" ">=" "<=>" "+" "-" "*" "/"
  "%" "**" "." "++" "--" "+=" "-=" "*=" "/=" ".=" "??" "??=" "&&" "||" "!"
  "&" "|" "^" "~" "<<" ">>" "=>" "->" "?->" "::" "?"
] @operator

[
  ";" ","
] @punctuation.delimiter

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

[
  (php_tag)
  (php_end_tag)
] @tag

; Keywords --------------------------------------------------------------------

[
  "abstract" "and" "as" "class" "clone" "const" "declare" "echo" "enddeclare"
  "enum" "extends" "final" "fn" "function" "global" "implements" "include"
  "include_once" "instanceof" "insteadof" "interface" "namespace" "new" "or"
  "print" "readonly" "require" "require_once" "static" "trait" "use" "xor"
  (abstract_modifier) (final_modifier) (readonly_modifier) (static_modifier)
  (visibility_modifier)
] @keyword

[
  "break" "case" "catch" "continue" "default" "do" "else" "elseif" "endfor"
  "endforeach" "endif" "endswitch" "endwhile" "exit" "finally" "for"
  "foreach" "goto" "if" "match" "return" "switch" "throw" "try" "while"
  "yield" "yield from"
] @keyword.control

; Comments --------------------------------------------------------------------

(comment) @comment

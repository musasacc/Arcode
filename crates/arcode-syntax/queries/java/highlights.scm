; Java highlights for Arcode (adapted from tree-sitter-java, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

; Identifiers -----------------------------------------------------------------

((identifier) @constant
  (#match? @constant "^_*[A-Z][A-Z\\d_]+$"))
(type_identifier) @type
[
  (boolean_type)
  (integral_type)
  (floating_point_type)
  (void_type)
] @type.builtin

(interface_declaration
  name: (identifier) @type)
(class_declaration
  name: (identifier) @type)
(enum_declaration
  name: (identifier) @type)
(record_declaration
  name: (identifier) @type)
((field_access
  object: (identifier) @type)
  (#match? @type "^[A-Z][a-z]"))
((scoped_identifier
  scope: (identifier) @type)
  (#match? @type "^[A-Z][a-z]"))
((method_invocation
  object: (identifier) @type)
  (#match? @type "^[A-Z][a-z]"))

(field_access
  field: (identifier) @property)
(formal_parameter
  name: (identifier) @variable.parameter)

; Functions -------------------------------------------------------------------

(method_declaration
  name: (identifier) @function.method)
(method_invocation
  name: (identifier) @function.method)
(constructor_declaration
  name: (identifier) @constructor)
(object_creation_expression
  type: (type_identifier) @constructor)
(super) @variable.builtin
(this) @variable.builtin

(annotation
  "@" @attribute
  name: (identifier) @attribute)
(marker_annotation
  "@" @attribute
  name: (identifier) @attribute)

; Literals --------------------------------------------------------------------

[
  (hex_integer_literal)
  (decimal_integer_literal)
  (octal_integer_literal)
  (binary_integer_literal)
  (decimal_floating_point_literal)
  (hex_floating_point_literal)
] @number
[
  (character_literal)
  (string_literal)
] @string
(escape_sequence) @string.escape
[
  (true)
  (false)
] @boolean
(null_literal) @constant.builtin

; Punctuation and operators ---------------------------------------------------

[
  "+" "-" "*" "/" "%" "++" "--" "=" "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^="
  "<<=" ">>=" ">>>=" "==" "!=" "<" ">" "<=" ">=" "&&" "||" "!" "&" "|" "^"
  "~" "<<" ">>" ">>>" "->" "?" "::"
] @operator

[
  ";" "." "," ":"
] @punctuation.delimiter

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

; Keywords --------------------------------------------------------------------

[
  "abstract" "assert" "class" "enum" "exports" "extends" "final" "implements"
  "import" "instanceof" "interface" "module" "native" "new" "non-sealed"
  "open" "opens" "package" "permits" "private" "protected" "provides" "public"
  "record" "requires" "sealed" "static" "strictfp" "synchronized" "throws"
  "to" "transient" "transitive" "uses" "volatile" "with"
] @keyword

[
  "break" "case" "catch" "continue" "default" "do" "else" "finally" "for" "if"
  "return" "switch" "throw" "try" "when" "while" "yield"
] @keyword.control

; Comments --------------------------------------------------------------------

[
  (line_comment)
  (block_comment)
] @comment
((block_comment) @comment.documentation
  (#match? @comment.documentation "^/\\*\\*"))

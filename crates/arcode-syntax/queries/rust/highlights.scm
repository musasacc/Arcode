; Rust highlights for Arcode.
;
; Precedence: when captures overlap, the innermost node wins; for the same
; node, the LATER pattern wins. Keep general patterns first and specific
; ones further down. Only use names listed in `HIGHLIGHT_NAMES`.

; Identifier conventions ------------------------------------------------------

((identifier) @constructor
  (#match? @constructor "^[A-Z]"))

((identifier) @constant
  (#match? @constant "^[A-Z][A-Z\\d_]+$"))

(type_identifier) @type
(primitive_type) @type.builtin
(field_identifier) @property
(shorthand_field_identifier) @property

; Paths: lowercase segments are modules, uppercase ones are types.
(scoped_identifier
  path: (identifier) @module)
((scoped_identifier
  path: (identifier) @type)
  (#match? @type "^[A-Z]"))
(scoped_type_identifier
  path: (identifier) @module)
((scoped_type_identifier
  path: (identifier) @type)
  (#match? @type "^[A-Z]"))
(mod_item
  name: (identifier) @module)

(parameter
  pattern: (identifier) @variable.parameter)
(closure_parameters
  (identifier) @variable.parameter)

(lifetime) @label
(label) @label

(enum_variant
  name: (identifier) @constructor)
(struct_pattern
  type: (scoped_type_identifier
    name: (type_identifier) @constructor))

; Functions -------------------------------------------------------------------

(function_item
  name: (identifier) @function)
(function_signature_item
  name: (identifier) @function)

(call_expression
  function: (identifier) @function)
(call_expression
  function: (scoped_identifier
    name: (identifier) @function))
(call_expression
  function: (field_expression
    field: (field_identifier) @function.method))
(generic_function
  function: (identifier) @function)
(generic_function
  function: (scoped_identifier
    name: (identifier) @function))
(generic_function
  function: (field_expression
    field: (field_identifier) @function.method))

; Uppercase calls are tuple structs / enum variants: `Some(x)`, `Foo::Bar(y)`.
((call_expression
  function: (identifier) @constructor)
  (#match? @constructor "^[A-Z]"))
((call_expression
  function: (scoped_identifier
    name: (identifier) @constructor))
  (#match? @constructor "^[A-Z]"))

(macro_invocation
  macro: (identifier) @function.macro
  "!" @function.macro)
(macro_invocation
  macro: (scoped_identifier
    name: (identifier) @function.macro))
(macro_definition
  name: (identifier) @function.macro)

; Literals --------------------------------------------------------------------

(boolean_literal) @boolean
(integer_literal) @number
(float_literal) @number
(char_literal) @string
(string_literal) @string
(raw_string_literal) @string
(escape_sequence) @string.escape

; Punctuation and operators ---------------------------------------------------

; "/", "*" and "!" also appear inside doc comment markers (`///`, `/**`,
; `//!`), so they are only captured in expression and type contexts.
[
  "&" "+" "-" "%" "^" "|" "?" "@"
  "=" "==" "!=" "<" ">" "<=" ">=" "&&" "||"
  "<<" ">>" "+=" "-=" "*=" "/=" "%=" "^=" "&=" "|=" "<<=" ">>="
  ".." "..." "..="
] @operator
(binary_expression
  [
    "/"
    "*"
  ] @operator)
(unary_expression
  [
    "!"
    "*"
  ] @operator)
(pointer_type
  "*" @operator)
(never_type
  "!" @operator)
(use_wildcard
  "*" @operator)

[
  "::" ":" "." "," ";" "->" "=>"
] @punctuation.delimiter

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

(type_arguments
  [
    "<"
    ">"
  ] @punctuation.bracket)
(type_parameters
  [
    "<"
    ">"
  ] @punctuation.bracket)

; Keywords --------------------------------------------------------------------

[
  "as" "async" "await" "const" "default" "dyn" "enum" "extern" "fn" "gen"
  "impl" "let" "macro_rules!" "mod" "move" "pub" "raw" "ref" "static"
  "struct" "trait" "type" "union" "unsafe" "use" "where"
] @keyword

[
  "break" "continue" "else" "for" "if" "in" "loop" "match" "return" "while"
  "yield"
] @keyword.control

(crate) @keyword
(super) @keyword
(mutable_specifier) @keyword
(self) @variable.builtin

; Attributes ------------------------------------------------------------------

(attribute_item) @attribute
(inner_attribute_item) @attribute

; Comments (last: nothing inside a comment overrides it) ----------------------

(line_comment) @comment
(block_comment) @comment
(line_comment
  (doc_comment)) @comment.documentation
(block_comment
  (doc_comment)) @comment.documentation

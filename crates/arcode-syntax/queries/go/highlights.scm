; Go highlights for Arcode (adapted from tree-sitter-go, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

; Identifiers -----------------------------------------------------------------

(type_identifier) @type
(field_identifier) @property
(package_identifier) @module
(label_name) @label

((identifier) @constant
  (#match? @constant "^_*[A-Z][A-Z\\d_]+$"))

(parameter_declaration
  name: (identifier) @variable.parameter)
(variadic_parameter_declaration
  name: (identifier) @variable.parameter)

((type_identifier) @type.builtin
  (#any-of? @type.builtin
    "any" "bool" "byte" "comparable" "complex128" "complex64" "error"
    "float32" "float64" "int" "int16" "int32" "int64" "int8" "rune" "string"
    "uint" "uint16" "uint32" "uint64" "uint8" "uintptr"))

; Functions -------------------------------------------------------------------

(call_expression
  function: (identifier) @function)
(call_expression
  function: (selector_expression
    field: (field_identifier) @function.method))

((call_expression
  function: (identifier) @function.builtin)
  (#any-of? @function.builtin
    "append" "cap" "clear" "close" "complex" "copy" "delete" "imag" "len"
    "make" "max" "min" "new" "panic" "print" "println" "real" "recover"))

(function_declaration
  name: (identifier) @function)
(method_declaration
  name: (field_identifier) @function.method)
(method_elem
  name: (field_identifier) @function.method)

; Literals --------------------------------------------------------------------

[
  (interpreted_string_literal)
  (raw_string_literal)
  (rune_literal)
] @string
(escape_sequence) @string.escape

[
  (int_literal)
  (float_literal)
  (imaginary_literal)
] @number

[
  (true)
  (false)
] @boolean

[
  (nil)
  (iota)
] @constant.builtin

; Punctuation and operators ---------------------------------------------------

[
  "--" "-" "-=" ":=" "!" "!=" "..." "*" "*=" "/" "/=" "&" "&&" "&=" "&^"
  "&^=" "%" "%=" "^" "^=" "+" "++" "+=" "<-" "<" "<<" "<<=" "<=" "=" "=="
  ">" ">=" ">>" ">>=" "|" "|=" "||" "~"
] @operator

[
  "." "," ";" ":"
] @punctuation.delimiter

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

; Keywords --------------------------------------------------------------------

[
  "chan" "const" "func" "import" "interface" "map" "package" "struct" "type"
  "var"
] @keyword

[
  "break" "case" "continue" "default" "defer" "else" "fallthrough" "for" "go"
  "goto" "if" "range" "return" "select" "switch"
] @keyword.control

; Comments --------------------------------------------------------------------

(comment) @comment

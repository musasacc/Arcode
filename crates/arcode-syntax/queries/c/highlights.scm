; C highlights for Arcode (adapted from tree-sitter-c, MIT).
; Also the base of the C++ query.
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

; Identifiers -----------------------------------------------------------------

((identifier) @constant
  (#match? @constant "^_*[A-Z][A-Z\\d_]+$"))

(field_identifier) @property
(statement_identifier) @label
(type_identifier) @type
(primitive_type) @type.builtin
(sized_type_specifier) @type.builtin

(parameter_declaration
  declarator: (identifier) @variable.parameter)
(parameter_declaration
  declarator: (pointer_declarator
    declarator: (identifier) @variable.parameter))

; Functions -------------------------------------------------------------------

(call_expression
  function: (identifier) @function)
(call_expression
  function: (field_expression
    field: (field_identifier) @function.method))
(function_declarator
  declarator: (identifier) @function)
(preproc_function_def
  name: (identifier) @function.macro)
(preproc_def
  name: (identifier) @constant)

; Literals --------------------------------------------------------------------

[
  (string_literal)
  (system_lib_string)
  (char_literal)
] @string
(escape_sequence) @string.escape
(number_literal) @number
[
  (true)
  (false)
] @boolean
(null) @constant.builtin

; Punctuation and operators ---------------------------------------------------

[
  "--" "-" "-=" "->" "=" "!=" "*" "&" "&&" "+" "++" "+=" "<" "==" ">" "||"
  "!" "~" "|" "^" "%" "/" "<<" ">>" "<=" ">=" "*=" "/=" "%=" "&=" "|=" "^="
  "<<=" ">>=" "?"
] @operator

[
  "." "," ";" ":"
] @punctuation.delimiter

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

; Keywords --------------------------------------------------------------------

[
  "const" "enum" "extern" "inline" "sizeof" "static" "struct" "typedef"
  "union" "volatile" "register" "restrict" "_Atomic" "_Noreturn" "noreturn"
  "_Alignas" "alignas" "_Alignof" "alignof" "offsetof" "_Generic" "asm"
  "__asm__" "thread_local" "__thread" "__attribute__" "__declspec"
] @keyword

[
  "break" "case" "continue" "default" "do" "else" "for" "goto" "if" "return"
  "switch" "while"
] @keyword.control

[
  "#define" "#elif" "#else" "#endif" "#if" "#ifdef" "#ifndef" "#include"
  (preproc_directive)
] @keyword

; Comments --------------------------------------------------------------------

(comment) @comment

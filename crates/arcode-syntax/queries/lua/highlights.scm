; Lua highlights for Arcode (adapted from tree-sitter-lua, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

; Identifiers -----------------------------------------------------------------

((identifier) @constant
  (#match? @constant "^[A-Z][A-Z_0-9]*$"))
((identifier) @variable.builtin
  (#eq? @variable.builtin "self"))
(field
  name: (identifier) @property)
(dot_index_expression
  field: (identifier) @property)
(parameters
  (identifier) @variable.parameter)
(variable_list
  (attribute
    "<" @punctuation.bracket
    (identifier) @attribute
    ">" @punctuation.bracket))
(label_statement) @label
(vararg_expression) @constant

; Functions -------------------------------------------------------------------

(function_declaration
  name: [
    (identifier) @function
    (dot_index_expression
      field: (identifier) @function)
  ])
(function_declaration
  name: (method_index_expression
    method: (identifier) @function.method))
(assignment_statement
  (variable_list
    .
    name: [
      (identifier) @function
      (dot_index_expression
        field: (identifier) @function)
    ])
  (expression_list
    .
    value: (function_definition)))
(table_constructor
  (field
    name: (identifier) @function
    value: (function_definition)))

(function_call
  name: [
    (identifier) @function
    (dot_index_expression
      field: (identifier) @function)
    (method_index_expression
      method: (identifier) @function.method)
  ])
(function_call
  (identifier) @function.builtin
  (#any-of? @function.builtin
    "assert" "collectgarbage" "dofile" "error" "getfenv" "getmetatable"
    "ipairs" "load" "loadfile" "loadstring" "module" "next" "pairs" "pcall"
    "print" "rawequal" "rawget" "rawlen" "rawset" "require" "select"
    "setfenv" "setmetatable" "tonumber" "tostring" "type" "unpack" "xpcall"))

; Literals --------------------------------------------------------------------

(nil) @constant.builtin
[
  (false)
  (true)
] @boolean
(number) @number
(string) @string
(escape_sequence) @string.escape

; Punctuation and operators ---------------------------------------------------

(binary_expression
  operator: _ @operator)
(unary_expression
  operator: _ @operator)
"=" @operator
[
  "and" "not" "or"
] @keyword.operator

[
  ";" ":" "," "." "::"
] @punctuation.delimiter

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

; Keywords --------------------------------------------------------------------

[
  "function" "local" "global" "end" "in"
] @keyword

[
  "do" "else" "elseif" "for" "goto" "if" "repeat" "return" "then"
  "until" "while"
] @keyword.control
(break_statement) @keyword.control

; Comments --------------------------------------------------------------------

(comment) @comment
(hash_bang_line) @comment

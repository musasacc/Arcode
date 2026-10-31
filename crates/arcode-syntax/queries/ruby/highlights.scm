; Ruby highlights for Arcode (adapted from tree-sitter-ruby, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

; Identifiers -----------------------------------------------------------------

(constant) @type
((constant) @constant
  (#match? @constant "^[A-Z\\d_]+$"))
[
  (class_variable)
  (instance_variable)
] @property
(global_variable) @variable.builtin
[
  (self)
  (super)
] @variable.builtin

[
  (block_parameter (identifier))
  (block_parameters (identifier))
  (destructured_parameter (identifier))
  (hash_splat_parameter (identifier))
  (lambda_parameters (identifier))
  (method_parameters (identifier))
  (splat_parameter (identifier))
  (keyword_parameter name: (identifier))
  (optional_parameter name: (identifier))
] @variable.parameter

; Functions -------------------------------------------------------------------

(call
  method: [(identifier) (constant)] @function.method)
(method
  name: [(identifier) (constant)] @function)
(singleton_method
  name: [(identifier) (constant)] @function)
(alias
  (identifier) @function)
(setter
  (identifier) @function)
((identifier) @function.builtin
  (#any-of? @function.builtin
    "require" "require_relative" "load" "puts" "print" "p" "raise"
    "attr_reader" "attr_writer" "attr_accessor" "include" "extend" "prepend"))
((identifier) @keyword
  (#any-of? @keyword "private" "protected" "public" "module_function"))
"defined?" @function.builtin

; Literals --------------------------------------------------------------------

[
  (string)
  (bare_string)
  (subshell)
  (heredoc_body)
  (heredoc_beginning)
  (character)
] @string
[
  (simple_symbol)
  (delimited_symbol)
  (hash_key_symbol)
  (bare_symbol)
] @string.special
(regex) @string.regex
(escape_sequence) @string.escape
[
  (integer)
  (float)
  (rational)
  (complex)
] @number
[
  (true)
  (false)
] @boolean
(nil) @constant.builtin

(interpolation
  "#{" @punctuation.special
  "}" @punctuation.special) @embedded

; Punctuation and operators ---------------------------------------------------

[
  "=" "=>" "->" "+" "-" "*" "/" "**" "%" "==" "!=" "<" ">" "<=" ">=" "<=>"
  "===" "=~" "!~" "&&" "||" "!" "&" "|" "^" "~" "<<" ">>" "+=" "-=" "*="
  "/=" "||=" "&&=" ".." "..."
] @operator

[
  "," ";" "." "::"
] @punctuation.delimiter

[
  "(" ")" "[" "]" "{" "}" "%w(" "%i("
] @punctuation.bracket

; Keywords --------------------------------------------------------------------

[
  "alias" "and" "begin" "class" "def" "do" "end" "module" "not" "or" "then"
  "undef" "BEGIN" "END"
] @keyword

[
  "break" "case" "else" "elsif" "ensure" "for" "if" "in" "next" "redo"
  "rescue" "retry" "return" "unless" "until" "when" "while" "yield"
] @keyword.control

; Comments --------------------------------------------------------------------

(comment) @comment

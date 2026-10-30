; JavaScript highlights for Arcode (adapted from tree-sitter-javascript, MIT).
; Also the base of the JSX, TypeScript and TSX queries.
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

; Identifier conventions ------------------------------------------------------

(property_identifier) @property
(shorthand_property_identifier) @property
(private_property_identifier) @property

((identifier) @constructor
  (#match? @constructor "^[A-Z]"))

([
  (identifier)
  (shorthand_property_identifier)
  (shorthand_property_identifier_pattern)
] @constant
  (#match? @constant "^_*[A-Z][A-Z\\d_]+$"))

((identifier) @variable.builtin
  (#any-of? @variable.builtin "arguments" "module" "console" "window" "document" "globalThis"))


(statement_identifier) @label

; Functions -------------------------------------------------------------------

(function_expression
  name: (identifier) @function)
(function_declaration
  name: (identifier) @function)
(generator_function_declaration
  name: (identifier) @function)
(method_definition
  name: [(property_identifier) (private_property_identifier)] @function.method)
(pair
  key: (property_identifier) @function.method
  value: [(function_expression) (arrow_function)])
(assignment_expression
  left: (member_expression
    property: (property_identifier) @function.method)
  right: [(function_expression) (arrow_function)])
(variable_declarator
  name: (identifier) @function
  value: [(function_expression) (arrow_function)])
(assignment_expression
  left: (identifier) @function
  right: [(function_expression) (arrow_function)])

(call_expression
  function: (identifier) @function)
(call_expression
  function: (member_expression
    property: [(property_identifier) (private_property_identifier)] @function.method))

((identifier) @function.builtin
  (#eq? @function.builtin "require"))

(new_expression
  constructor: (identifier) @constructor)

; Literals --------------------------------------------------------------------

[
  (this)
  (super)
] @variable.builtin

[
  (true)
  (false)
] @boolean

[
  (null)
  (undefined)
] @constant.builtin

[
  (string)
  (template_string)
] @string
(escape_sequence) @string.escape
(regex) @string.regex
(number) @number

(template_substitution
  "${" @punctuation.special
  "}" @punctuation.special) @embedded

; Punctuation and operators ---------------------------------------------------

[
  ";" "." "," ":" (optional_chain)
] @punctuation.delimiter

[
  "-" "--" "-=" "+" "++" "+=" "*" "*=" "**" "**=" "/" "/=" "%" "%=" "<" "<="
  "<<" "<<=" "=" "==" "===" "!" "!=" "!==" "=>" ">" ">=" ">>" ">>=" ">>>"
  ">>>=" "~" "^" "&" "|" "^=" "&=" "|=" "&&" "||" "??" "&&=" "||=" "??="
  "?" "..."
] @operator

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

; The slashes of a regex literal belong to the regex, not to `/`.
(regex
  "/" @string.regex)

; Keywords --------------------------------------------------------------------

[
  "as" "async" "await" "class" "const" "debugger" "delete" "export" "extends"
  "from" "function" "get" "import" "in" "instanceof" "let" "new" "of" "set"
  "static" "target" "typeof" "using" "var" "void" "with"
] @keyword

[
  "break" "case" "catch" "continue" "default" "do" "else" "finally" "for" "if"
  "return" "switch" "throw" "try" "while" "yield"
] @keyword.control

; Comments --------------------------------------------------------------------

(comment) @comment
((comment) @comment.documentation
  (#match? @comment.documentation "^/\\*\\*"))

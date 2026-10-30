; Python highlights for Arcode (adapted from tree-sitter-python, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

; Identifier conventions ------------------------------------------------------

((identifier) @constructor
  (#match? @constructor "^[A-Z]"))

((identifier) @constant
  (#match? @constant "^_*[A-Z][A-Z\\d_]*$"))

(attribute
  attribute: (identifier) @property)

(type
  (identifier) @type)

(parameters
  (identifier) @variable.parameter)
(default_parameter
  name: (identifier) @variable.parameter)
(typed_parameter
  (identifier) @variable.parameter)
(typed_default_parameter
  name: (identifier) @variable.parameter)
(keyword_argument
  name: (identifier) @variable.parameter)

((identifier) @variable.builtin
  (#any-of? @variable.builtin "self" "cls"))

; Functions -------------------------------------------------------------------

(decorator) @attribute
(decorator
  (identifier) @attribute)

(call
  function: (identifier) @function)
(call
  function: (attribute
    attribute: (identifier) @function.method))

((call
  function: (identifier) @function.builtin)
  (#any-of? @function.builtin
    "abs" "all" "any" "ascii" "bin" "bool" "breakpoint" "bytearray" "bytes"
    "callable" "chr" "classmethod" "compile" "complex" "delattr" "dict" "dir"
    "divmod" "enumerate" "eval" "exec" "filter" "float" "format" "frozenset"
    "getattr" "globals" "hasattr" "hash" "help" "hex" "id" "input" "int"
    "isinstance" "issubclass" "iter" "len" "list" "locals" "map" "max"
    "memoryview" "min" "next" "object" "oct" "open" "ord" "pow" "print"
    "property" "range" "repr" "reversed" "round" "set" "setattr" "slice"
    "sorted" "staticmethod" "str" "sum" "super" "tuple" "type" "vars" "zip"
    "__import__"))

(function_definition
  name: (identifier) @function)
(class_definition
  name: (identifier) @type)

; Literals --------------------------------------------------------------------

[
  (none)
  (true)
  (false)
] @constant.builtin

[
  (integer)
  (float)
] @number

(string) @string
(escape_sequence) @string.escape

; Docstrings: a string that is the first statement of a module/def/class.
(module
  .
  (expression_statement
    (string) @comment.documentation))
(function_definition
  body: (block
    .
    (expression_statement
      (string) @comment.documentation)))
(class_definition
  body: (block
    .
    (expression_statement
      (string) @comment.documentation)))

(interpolation
  "{" @punctuation.special
  "}" @punctuation.special) @embedded

; Punctuation and operators ---------------------------------------------------

[
  "-" "-=" "!=" "*" "**" "**=" "*=" "/" "//" "//=" "/=" "&" "&=" "%" "%="
  "^" "^=" "+" "->" "+=" "<" "<<" "<<=" "<=" "<>" "=" ":=" "==" ">" ">="
  ">>" ">>=" "|" "|=" "~" "@="
] @operator

[
  "and" "in" "is" "not" "or" "is not" "not in"
] @keyword.operator

[
  "," "." ":" ";"
] @punctuation.delimiter

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

; Keywords --------------------------------------------------------------------

[
  "as" "assert" "async" "await" "class" "def" "del" "exec" "from" "global"
  "import" "lambda" "nonlocal" "pass" "print" "type"
] @keyword

[
  "break" "case" "continue" "elif" "else" "except" "finally" "for" "if"
  "match" "raise" "return" "try" "while" "with" "yield"
] @keyword.control

; Comments --------------------------------------------------------------------

(comment) @comment

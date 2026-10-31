; CSS highlights for Arcode (adapted from tree-sitter-css, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

(tag_name) @tag
(nesting_selector) @tag
(universal_selector) @tag
(class_name) @type
(id_name) @constant
(namespace_name) @module
(property_name) @property
(feature_name) @property
(attribute_name) @attribute
(pseudo_element_selector
  (tag_name) @attribute)
(pseudo_class_selector
  (class_name) @attribute)

((property_name) @variable
  (#match? @variable "^--"))
((plain_value) @variable
  (#match? @variable "^--"))
(attribute_selector
  (plain_value) @string)

(function_name) @function

[
  "@media" "@import" "@charset" "@namespace" "@supports" "@keyframes"
  (at_keyword) (to) (from) (important)
] @keyword

(string_value) @string
(color_value) @string.special
(integer_value) @number
(float_value) @number
(unit) @type.builtin

[
  "~" ">" "+" "-" "*" "/" "=" "^=" "|=" "~=" "$=" "*=" "and" "or" "not"
  "only"
] @operator

[
  "," "." ":" "::" ";"
] @punctuation.delimiter
(id_selector
  "#" @punctuation.delimiter)

[
  "{" "}" "(" ")" "[" "]"
] @punctuation.bracket

(comment) @comment

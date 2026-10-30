
; JSX (appended to the JavaScript query) --------------------------------------

(jsx_opening_element
  name: (identifier) @tag)
(jsx_closing_element
  name: (identifier) @tag)
(jsx_self_closing_element
  name: (identifier) @tag)
((jsx_opening_element
  name: (identifier) @constructor)
  (#match? @constructor "^[A-Z]"))
((jsx_closing_element
  name: (identifier) @constructor)
  (#match? @constructor "^[A-Z]"))
((jsx_self_closing_element
  name: (identifier) @constructor)
  (#match? @constructor "^[A-Z]"))

(jsx_attribute
  (property_identifier) @attribute)

(jsx_opening_element
  [
    "<"
    ">"
  ] @punctuation.bracket)
(jsx_closing_element
  [
    "</"
    ">"
  ] @punctuation.bracket)
(jsx_self_closing_element
  [
    "<"
    "/>"
  ] @punctuation.bracket)

(jsx_expression
  [
    "{"
    "}"
  ] @punctuation.special)

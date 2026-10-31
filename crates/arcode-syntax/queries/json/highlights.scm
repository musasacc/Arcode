; JSON highlights for Arcode.
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.

(string) @string

(pair
  key: (string) @property)

(escape_sequence) @string.escape

(number) @number

[
  (true)
  (false)
] @boolean

(null) @constant.builtin

[
  "{"
  "}"
  "["
  "]"
] @punctuation.bracket

[
  ","
  ":"
] @punctuation.delimiter

(comment) @comment

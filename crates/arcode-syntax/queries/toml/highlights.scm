; TOML highlights for Arcode.
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.

; Keys ------------------------------------------------------------------------

(bare_key) @property
(quoted_key) @property

; Table headers: `[package]`, `[dependencies.foo]`, `[[bin]]`.
(table
  (bare_key) @type)
(table
  (quoted_key) @type)
(table
  (dotted_key
    (bare_key) @type))
(table
  (dotted_key
    (quoted_key) @type))
(table_array_element
  (bare_key) @type)
(table_array_element
  (quoted_key) @type)
(table_array_element
  (dotted_key
    (bare_key) @type))
(table_array_element
  (dotted_key
    (quoted_key) @type))

; Literals --------------------------------------------------------------------

(string) @string
(escape_sequence) @string.escape
(boolean) @boolean

[
  (integer)
  (float)
] @number

[
  (offset_date_time)
  (local_date_time)
  (local_date)
  (local_time)
] @string.special

; Punctuation -----------------------------------------------------------------

[
  "."
  ","
] @punctuation.delimiter

"=" @operator

[
  "["
  "]"
  "[["
  "]]"
  "{"
  "}"
] @punctuation.bracket

(comment) @comment

; YAML highlights for Arcode (adapted from tree-sitter-yaml, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

[
  (double_quote_scalar)
  (single_quote_scalar)
  (block_scalar)
  (string_scalar)
] @string
(escape_sequence) @string.escape

(boolean_scalar) @boolean
(null_scalar) @constant.builtin
[
  (integer_scalar)
  (float_scalar)
] @number

[
  (anchor_name)
  (alias_name)
] @label
(tag) @type
[
  (yaml_directive)
  (tag_directive)
  (reserved_directive)
] @attribute

; Mapping keys.
(block_mapping_pair
  key: (flow_node
    [
      (double_quote_scalar)
      (single_quote_scalar)
    ] @property))
(block_mapping_pair
  key: (flow_node
    (plain_scalar
      (string_scalar) @property)))
(flow_mapping
  (_
    key: (flow_node
      [
        (double_quote_scalar)
        (single_quote_scalar)
      ] @property)))
(flow_mapping
  (_
    key: (flow_node
      (plain_scalar
        (string_scalar) @property))))

[
  "," "-" ":" ">" "?" "|"
] @punctuation.delimiter

[
  "[" "]" "{" "}"
] @punctuation.bracket

[
  "*" "&" "---" "..."
] @punctuation.special

(comment) @comment

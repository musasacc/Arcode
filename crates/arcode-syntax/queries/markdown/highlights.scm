; Markdown highlights for Arcode (adapted from tree-sitter-md, MIT).
;
; Block structure only: inline markup (emphasis, code spans, inline links)
; needs the separate inline grammar and is not highlighted yet. Markup is
; mapped onto the code names themes already style: headings are `keyword`,
; code blocks `string`, link targets `string.special`.

(atx_heading
  (inline) @keyword)
(setext_heading
  (paragraph) @keyword)

[
  (atx_h1_marker)
  (atx_h2_marker)
  (atx_h3_marker)
  (atx_h4_marker)
  (atx_h5_marker)
  (atx_h6_marker)
  (setext_h1_underline)
  (setext_h2_underline)
] @punctuation.special

[
  (indented_code_block)
  (fenced_code_block)
] @string
(fenced_code_block_delimiter) @punctuation.delimiter
(info_string
  (language) @label)

(link_destination) @string.special
(link_label) @label
(link_title) @string

[
  (list_marker_plus)
  (list_marker_minus)
  (list_marker_star)
  (list_marker_dot)
  (list_marker_parenthesis)
  (thematic_break)
  (block_quote_marker)
] @punctuation.special

(task_list_marker_checked) @constant.builtin
(task_list_marker_unchecked) @constant.builtin

(html_block) @tag
(backslash_escape) @string.escape

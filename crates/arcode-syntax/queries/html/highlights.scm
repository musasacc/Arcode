; HTML highlights for Arcode (adapted from tree-sitter-html, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Embedded `<script>` / `<style>` content is not highlighted (no injections).

(tag_name) @tag
(erroneous_end_tag_name) @tag
(doctype) @constant
(attribute_name) @attribute
(attribute_value) @string
(quoted_attribute_value) @string
(entity) @string.escape

[
  "<" ">" "</" "/>" "<!"
] @punctuation.bracket
"=" @operator

(comment) @comment

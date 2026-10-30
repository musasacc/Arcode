; Markdown outline for Arcode: one item per ATX section, named by its
; heading, so sub-headings nest. The grammar does not open sections for
; setext headings (`Title` + `---`), so those span only their heading.
; Conventions: see `src/outline.rs`.

((section
  .
  (atx_heading
    heading_content: (inline) @name)) @item
  (#set! kind "heading"))
((setext_heading
  heading_content: (paragraph) @name) @item
  (#set! kind "heading"))

; Markdown outline for Arcode: one item per heading with its level; the
; item spans the heading's section (to the next heading of the same or a
; higher level), so sub-headings nest for ATX and setext headings alike.
; Conventions: see `src/outline.rs`.

((atx_heading
  (atx_h1_marker)
  heading_content: (inline) @name) @item
  (#set! kind "heading")
  (#set! level "1")
  (#set! strip "atx_closing"))
((atx_heading
  (atx_h2_marker)
  heading_content: (inline) @name) @item
  (#set! kind "heading")
  (#set! level "2")
  (#set! strip "atx_closing"))
((atx_heading
  (atx_h3_marker)
  heading_content: (inline) @name) @item
  (#set! kind "heading")
  (#set! level "3")
  (#set! strip "atx_closing"))
((atx_heading
  (atx_h4_marker)
  heading_content: (inline) @name) @item
  (#set! kind "heading")
  (#set! level "4")
  (#set! strip "atx_closing"))
((atx_heading
  (atx_h5_marker)
  heading_content: (inline) @name) @item
  (#set! kind "heading")
  (#set! level "5")
  (#set! strip "atx_closing"))
((atx_heading
  (atx_h6_marker)
  heading_content: (inline) @name) @item
  (#set! kind "heading")
  (#set! level "6")
  (#set! strip "atx_closing"))
((setext_heading
  heading_content: (paragraph) @name
  (setext_h1_underline)) @item
  (#set! kind "heading")
  (#set! level "1"))
((setext_heading
  heading_content: (paragraph) @name
  (setext_h2_underline)) @item
  (#set! kind "heading")
  (#set! level "2"))

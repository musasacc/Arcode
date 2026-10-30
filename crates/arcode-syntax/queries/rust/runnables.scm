; Rust runnables for Arcode. Conventions: see `src/runnables.rs`.
;
; Attribute patterns capture only the attribute as `@anchor`; the item is
; the first sibling after the run of attributes and comments (resolved in
; `src/anchor.rs`). A query-side sibling sequence would be cubic in the
; length of long attribute runs (bindgen `#[doc]`, rstest `#[case]`).

; `fn main` at the top level of the file.
((source_file
  (function_item
    name: (identifier) @name) @run)
  (#eq? @name "main")
  (#set! kind "main"))

; `#[test]`, `#[tokio::test]`, `#[some::path::test]`, possibly followed by
; more attributes (`#[should_panic]`, `#[ignore]`) or comments.
((attribute_item
  (attribute
    [
      (identifier) @_attr
      (scoped_identifier
        name: (identifier) @_attr)
    ])) @anchor
  (#eq? @_attr "test")
  (#set! kind "test")
  (#set! target "function_item")
  (#set! skip "attribute_item line_comment block_comment"))

; `#[bench]`.
((attribute_item
  (attribute
    (identifier) @_attr)) @anchor
  (#eq? @_attr "bench")
  (#set! kind "bench")
  (#set! target "function_item")
  (#set! skip "attribute_item line_comment block_comment"))

; `#[cfg(test)] mod tests { ... }`.
((attribute_item
  (attribute
    (identifier) @_cfg
    arguments: (token_tree
      (identifier) @_test))) @anchor
  (#eq? @_cfg "cfg")
  (#eq? @_test "test")
  (#set! kind "test_module")
  (#set! target "mod_item")
  (#set! require "body")
  (#set! skip "attribute_item line_comment block_comment"))

; `#[cfg(all(test, unix))] mod tests { ... }` (also `any(...)`).
((attribute_item
  (attribute
    (identifier) @_cfg
    arguments: (token_tree
      (identifier) @_all
      .
      (token_tree
        (identifier) @_test)))) @anchor
  (#eq? @_cfg "cfg")
  (#any-of? @_all "all" "any")
  (#eq? @_test "test")
  (#set! kind "test_module")
  (#set! target "mod_item")
  (#set! require "body")
  (#set! skip "attribute_item line_comment block_comment"))

; Every inline module is a scope of the test path (`a::tests::it_works`).
(mod_item
  name: (identifier) @name
  body: (declaration_list)) @scope

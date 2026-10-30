; Rust runnables for Arcode. Conventions: see `src/runnables.rs`.

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
    ]))
  .
  [
    (attribute_item)
    (line_comment)
    (block_comment)
  ]*
  .
  (function_item
    name: (identifier) @name) @run
  (#eq? @_attr "test")
  (#set! kind "test"))

; `#[bench]`.
((attribute_item
  (attribute
    (identifier) @_attr))
  .
  [
    (attribute_item)
    (line_comment)
    (block_comment)
  ]*
  .
  (function_item
    name: (identifier) @name) @run
  (#eq? @_attr "bench")
  (#set! kind "bench"))

; `#[cfg(test)] mod tests { ... }`.
((attribute_item
  (attribute
    (identifier) @_cfg
    arguments: (token_tree
      (identifier) @_test)))
  .
  [
    (attribute_item)
    (line_comment)
    (block_comment)
  ]*
  .
  (mod_item
    name: (identifier) @name
    body: (declaration_list)) @run
  (#eq? @_cfg "cfg")
  (#eq? @_test "test")
  (#set! kind "test_module"))

; Every inline module is a scope of the test path (`a::tests::it_works`).
(mod_item
  name: (identifier) @name
  body: (declaration_list)) @scope

; JavaScript / TypeScript runnables (Jest, Vitest, Mocha, node:test,
; Playwright) for Arcode. Conventions: see `src/runnables.rs`. The name is
; the first string argument, quotes stripped, inner whitespace kept.

; `describe("suite", ...)`, `describe.only(...)`, `suite(...)`.
((call_expression
  function: [
    (identifier) @_fn
    (member_expression
      object: (identifier) @_fn)
  ]
  arguments: (arguments
    .
    [
      (string)
      (template_string)
    ] @name)) @run @scope
  (#any-of? @_fn "describe" "context" "suite")
  (#set! kind "test_module"))

; `describe.each([...])("suite %s", ...)` and the tagged-template form.
((call_expression
  function: (call_expression
    function: (member_expression
      object: (identifier) @_fn
      property: (property_identifier) @_each))
  arguments: (arguments
    .
    [
      (string)
      (template_string)
    ] @name)) @run @scope
  (#any-of? @_fn "describe" "context" "suite")
  (#eq? @_each "each")
  (#set! kind "test_module"))

; Playwright `test.describe("suite", ...)`, `test.describe.serial(...)`.
((call_expression
  function: [
    (member_expression
      object: (identifier) @_fn
      property: (property_identifier) @_describe)
    (member_expression
      object: (member_expression
        object: (identifier) @_fn
        property: (property_identifier) @_describe))
  ]
  arguments: (arguments
    .
    [
      (string)
      (template_string)
    ] @name)) @run @scope
  (#eq? @_fn "test")
  (#eq? @_describe "describe")
  (#set! kind "test_module"))

; `it("does x", ...)`, `test("x", ...)`.
((call_expression
  function: (identifier) @_fn
  arguments: (arguments
    .
    [
      (string)
      (template_string)
    ] @name)) @run
  (#any-of? @_fn "it" "test" "specify")
  (#set! kind "test"))

; `it.skip(...)`, `test.only(...)`; not Playwright `test.describe`/`test.step`.
((call_expression
  function: (member_expression
    object: (identifier) @_fn
    property: (property_identifier) @_modifier)
  arguments: (arguments
    .
    [
      (string)
      (template_string)
    ] @name)) @run
  (#any-of? @_fn "it" "test" "specify")
  (#not-any-of? @_modifier "describe" "step")
  (#set! kind "test"))

; `it.each([...])("adds %i", ...)`, `test.each`...``(...)`.
((call_expression
  function: (call_expression
    function: (member_expression
      object: (identifier) @_fn
      property: (property_identifier) @_each))
  arguments: (arguments
    .
    [
      (string)
      (template_string)
    ] @name)) @run
  (#any-of? @_fn "it" "test" "specify")
  (#eq? @_each "each")
  (#set! kind "test"))

; Vitest `bench("name", ...)`.
((call_expression
  function: [
    (identifier) @_fn
    (member_expression
      object: (identifier) @_fn)
  ]
  arguments: (arguments
    .
    [
      (string)
      (template_string)
    ] @name)) @run
  (#eq? @_fn "bench")
  (#set! kind "bench"))

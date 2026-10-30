; JavaScript / TypeScript runnables (Jest, Vitest, Mocha, node:test) for
; Arcode. Conventions: see `src/runnables.rs`. The name is the first string
; argument, quotes stripped.

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

; `it("does x", ...)`, `test("x", ...)`, `it.skip(...)`, `test.only(...)`.
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
  (#any-of? @_fn "it" "test" "specify")
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

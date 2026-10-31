; Python runnables for Arcode. Conventions: see `src/runnables.rs`.

; `if __name__ == "__main__":` at module level.
((module
  (if_statement
    condition: (comparison_operator
      (identifier) @_name
      (string
        (string_content) @_main)) ) @run)
  (#eq? @_name "__name__")
  (#eq? @_main "__main__")
  (#set! kind "main")
  (#set! name "__main__"))
; The reversed spelling `if "__main__" == __name__:`.
((module
  (if_statement
    condition: (comparison_operator
      (string
        (string_content) @_main)
      (identifier) @_name)) @run)
  (#eq? @_name "__name__")
  (#eq? @_main "__main__")
  (#set! kind "main")
  (#set! name "__main__"))

; Module-level `def test_*` (pytest).
((module
  (function_definition
    name: (identifier) @name) @run)
  (#match? @name "^test")
  (#set! kind "test"))
((module
  (decorated_definition
    definition: (function_definition
      name: (identifier) @name) @run))
  (#match? @name "^test")
  (#set! kind "test"))

; `class Test*` (pytest) and `class X(unittest.TestCase)` with their
; `test*` methods.
((class_definition
  name: (identifier) @name) @run @scope
  (#match? @name "^Test")
  (#set! kind "test_module"))
((class_definition
  name: (identifier) @name
  superclasses: (argument_list
    [
      (identifier) @_base
      (attribute
        attribute: (identifier) @_base)
    ])) @run @scope
  (#match? @_base "TestCase$")
  (#set! kind "test_module"))

((class_definition
  name: (identifier) @_class
  body: (block
    [
      (function_definition
        name: (identifier) @name) @run
      (decorated_definition
        definition: (function_definition
          name: (identifier) @name) @run)
    ]))
  (#match? @_class "^Test")
  (#match? @name "^test")
  (#set! kind "test"))
((class_definition
  superclasses: (argument_list
    [
      (identifier) @_base
      (attribute
        attribute: (identifier) @_base)
    ])
  body: (block
    [
      (function_definition
        name: (identifier) @name) @run
      (decorated_definition
        definition: (function_definition
          name: (identifier) @name) @run)
    ]))
  (#match? @_base "TestCase$")
  (#match? @name "^test")
  (#set! kind "test"))

; Go runnables for Arcode. Conventions: see `src/runnables.rs`.

; `func main()` in `package main`.
((source_file
  (package_clause
    (package_identifier) @_pkg)
  (function_declaration
    name: (identifier) @name) @run)
  (#eq? @_pkg "main")
  (#eq? @name "main")
  (#set! kind "main"))

; `func TestXxx(t *testing.T)` and `func FuzzXxx(f *testing.F)`.
((function_declaration
  name: (identifier) @name
  parameters: (parameter_list
    .
    (parameter_declaration
      type: (pointer_type
        (qualified_type
          package: (package_identifier) @_testing
          name: (type_identifier) @_type)))
    .)) @run
  (#match? @name "^(Test|Fuzz)([^a-z]|$)")
  (#eq? @_testing "testing")
  (#any-of? @_type "T" "F")
  (#set! kind "test"))

; `func BenchmarkXxx(b *testing.B)`.
((function_declaration
  name: (identifier) @name
  parameters: (parameter_list
    .
    (parameter_declaration
      type: (pointer_type
        (qualified_type
          package: (package_identifier) @_testing
          name: (type_identifier) @_type)))
    .)) @run
  (#match? @name "^Benchmark([^a-z]|$)")
  (#eq? @_testing "testing")
  (#eq? @_type "B")
  (#set! kind "bench"))

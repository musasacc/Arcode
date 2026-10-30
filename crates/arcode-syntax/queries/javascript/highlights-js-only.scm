
; JavaScript-only patterns (TypeScript wraps parameters in
; `required_parameter` and names classes with `type_identifier`) -------------

(formal_parameters
  (identifier) @variable.parameter)
(formal_parameters
  (assignment_pattern
    left: (identifier) @variable.parameter))
(arrow_function
  parameter: (identifier) @variable.parameter)

(class_declaration
  name: (identifier) @type)
(class
  name: (identifier) @type)

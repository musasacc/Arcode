
; TypeScript (appended to the JavaScript query, adapted from
; tree-sitter-typescript, MIT) --------------------------------------------------

(type_identifier) @type
(predefined_type) @type.builtin

(interface_declaration
  name: (type_identifier) @type)
(type_alias_declaration
  name: (type_identifier) @type)
(enum_declaration
  name: (identifier) @type)
(abstract_class_declaration
  name: (type_identifier) @type)
(class_declaration
  name: (type_identifier) @type)
(internal_module
  name: (identifier) @module)

(type_arguments
  [
    "<"
    ">"
  ] @punctuation.bracket)
(type_parameters
  [
    "<"
    ">"
  ] @punctuation.bracket)

(required_parameter
  pattern: (identifier) @variable.parameter)
(optional_parameter
  pattern: (identifier) @variable.parameter)

(property_signature
  name: (property_identifier) @property)
(method_signature
  name: (property_identifier) @function.method)
(abstract_method_signature
  name: (property_identifier) @function.method)

(decorator) @attribute
(decorator
  (identifier) @attribute)
(decorator
  (call_expression
    function: (identifier) @attribute))

[
  "abstract" "declare" "enum" "implements" "interface" "keyof" "namespace"
  "private" "protected" "public" "type" "readonly" "override" "satisfies"
  "infer" "is" "asserts" "module"
] @keyword

(comment) @comment
((comment) @comment.documentation
  (#match? @comment.documentation "^/\\*\\*"))

; Shell script (bash) highlights for Arcode (adapted from tree-sitter-bash, MIT).
;
; Precedence: innermost node wins; for the same node the LATER pattern wins.
; Only use names listed in `HIGHLIGHT_NAMES`.

(command_name) @function
((command_name
  (word) @function.builtin)
  (#any-of? @function.builtin
    "alias" "bg" "bind" "builtin" "caller" "cd" "command" "compgen" "complete"
    "declare" "dirs" "disown" "echo" "enable" "eval" "exec" "exit" "export"
    "fc" "fg" "getopts" "hash" "help" "history" "jobs" "kill" "let" "local"
    "logout" "popd" "printf" "pushd" "pwd" "read" "readonly" "return" "set"
    "shift" "shopt" "source" "suspend" "test" "times" "trap" "type" "typeset"
    "ulimit" "umask" "unalias" "unset" "wait"))
(function_definition
  name: (word) @function)

(variable_name) @variable
((variable_name) @variable.builtin
  (#match? @variable.builtin "^(HOME|PATH|PWD|OLDPWD|SHELL|USER|IFS|PS1|PS2|RANDOM|LINENO|SECONDS|BASH_[A-Z_]+)$"))
(special_variable_name) @variable.builtin

(command
  argument: (word) @constant
  (#match? @constant "^-"))

[
  (string)
  (raw_string)
  (ansi_c_string)
  (heredoc_body)
] @string
(heredoc_start) @label
(heredoc_end) @label

(number) @number
(file_descriptor) @number
(test_operator) @operator

[
  (command_substitution)
  (process_substitution)
  (expansion)
] @embedded
(simple_expansion) @variable
(expansion
  [
    "${"
    "}"
  ] @punctuation.special)
(command_substitution
  [
    "$("
    ")"
  ] @punctuation.special)

[
  "$" "&&" ">" ">>" "<" "|" "||" "|&" "&>" "&>>" "<<" "<<-" "<<<" "=" "+="
  "==" "!=" "=~" "!"
] @operator

[
  ";" ";;" "&"
] @punctuation.delimiter

[
  "(" ")" "((" "))" "[" "]" "[[" "]]" "{" "}"
] @punctuation.bracket

[
  "declare" "export" "function" "local" "readonly" "typeset" "unset"
  "unsetenv"
] @keyword

[
  "case" "do" "done" "elif" "else" "esac" "fi" "for" "if" "in" "select"
  "then" "until" "while"
] @keyword.control

(comment) @comment

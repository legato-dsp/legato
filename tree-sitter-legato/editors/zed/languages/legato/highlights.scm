; Legato highlight queries for Zed.
; Mirrors ../../../queries/highlights.scm but uses Zed's native theme scope
; names. First-match-wins, so the generic `(identifier)` fallback comes last.

; ---- Keywords ----
[
  "patch"
  "kernel"
  "in"
] @keyword

; ---- Literals ----
(boolean) @boolean
(null) @constant
(comment) @comment
(string) @string
(escape_sequence) @string.escape
(template) @variable.special     ; $ident — a default-param reference
(float) @number
(integer) @number
(uint) @number

; ---- Declarations ----
(patch name: (identifier) @function)
(kernel name: (identifier) @function)
(default_params name: (identifier) @variable)
(scope namespace: (identifier) @constructor)
(node_declaration node_type: (identifier) @type)
(node_declaration alias: (identifier) @variable)
(node_declaration count: (uint) @number)
(pipe name: (identifier) @function)

; ---- Object / param keys ----
(param key: (identifier) @property)
(object_entry key: (identifier) @property)

; ---- Connections ----
(endpoint node: (identifier) @variable)
(port name: (identifier) @property)

; ---- Source / sink ----
(source name: (identifier) @variable)
(sink name: (identifier) @variable)

; ---- Operators ----
[
  ">>"
  "|"
  "*"
  "="
  ".."
] @operator

; ---- Punctuation ----
[ "{" "}" "(" ")" "[" "]" ] @punctuation.bracket
[ "," ":" "." ] @punctuation.delimiter

; ---- Fallback ----
(identifier) @variable

; Keywords
[
  "patch"
  "kernel"
  "in"
] @keyword

; Literals
(string) @string
(escape_sequence) @string.escape
(float) @constant.numeric.float
(integer) @constant.numeric.integer
(uint) @constant.numeric.integer
(boolean) @constant.builtin
(null) @constant.builtin
(template) @variable.parameter

(comment) @comment

; Scopes / namespaces
(scope namespace: (identifier) @namespace)

; Patch definition
(patch name: (identifier) @function)
(kernel name: (identifier) @function)

; Node declarations
(node_declaration node_type: (identifier) @type)
(node_declaration alias: (identifier) @variable)

; Pipes
(pipe name: (identifier) @function.method)

; Params / object keys
(param key: (identifier) @property)
(object_entry key: (identifier) @property)
(default_params name: (identifier) @variable.parameter)

; Virtual ports
(virtual_ports port: (identifier) @variable.parameter)

; Connection endpoints
(endpoint node: (identifier) @variable)
(port name: (identifier) @property)

; Source / sink markers
(source name: (identifier) @variable)
(sink name: (identifier) @variable)

; Punctuation
[ "{" "}" "(" ")" "[" "]" ] @punctuation.bracket
[ "," ":" "|" "=" "*" "." ">>" ".." ] @punctuation.delimiter

# tree-sitter-legato

A [tree-sitter](https://tree-sitter.github.io/tree-sitter/) grammar for the
Legato DSL, derived from the `chumsky` parser in `crates/src/dsl/parse.rs`.

## Development

```sh
tree-sitter generate
tree-sitter test
```

`editors/textmate/legato.tmLanguage.json` is a TextMate grammar that mirrors
`queries/highlights.scm`. The docs site uses it for Shiki highlighting, so keep
the two in step when adding syntax.

## External scanner

`src/scanner.c` emits one hidden token, `_newline`, used only to terminate a
patch's virtual-port line (`in gate freq_in`). The line break is the only signal
that ends the port list — without it the parser greedily pulls the following
scope's namespace identifier in as another port. Helix and Zed both compile
`src/scanner.c` automatically (it's picked up by filename convention), so no
extra build configuration is needed beyond committing the file.

## Known divergences from the reference parser

- **Comments are `extras`**, so they're skipped everywhere, not only where
  `extra_padded` is used. This is the conventional choice for an editor grammar
  and is strictly more permissive.
- **No error recovery** for arrays. The chumsky parser has `recover_with`
  combinators around array/`,` parsing; tree-sitter does its own error recovery
  via `ERROR` nodes, so those combinators have no grammar-level equivalent.
- **Numbers** are split into `float` (`-?\d+\.\d+`), `integer` (`-?\d+`, the
  `I32`/`U32` cases), and `uint` (`\d+`, the parser's `uint` used in counts,
  selectors and port indices). The grammar does not distinguish `I32` from `U32`
  the way `value_parser` does at the token level — both are `integer`. Downstream
  code can decide signedness from the presence of a leading `-`.

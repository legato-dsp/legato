# Legato — Zed extension

A dev extension that gives Zed syntax highlighting for `.legato` files.

## Install

1. Commit and push this repository (Zed builds the grammar from git, not a local
   path), then set `rev` in `extension.toml` to the pushed commit SHA.
2. In Zed, open the command palette and run **zed: install dev extension**.
3. Select this directory (`editors/zed`).

Zed clones the grammar repo at `rev`, builds `src/parser.c` to WebAssembly, and
loads `languages/legato/`. Open a `.legato` file to confirm highlighting.

To pick up later grammar changes: push them, bump `rev`, then run
**zed: reload extensions** (or reinstall the dev extension).

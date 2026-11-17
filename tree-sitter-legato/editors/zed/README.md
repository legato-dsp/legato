# Legato — Zed extension

A dev extension that gives Zed syntax highlighting for `.legato` files.

## Install

1. Commit and push the grammar (Zed builds it from a git commit, not the working
   tree), then set `rev` in `extension.toml` to that commit SHA. To test before
   pushing, set `repository` to `file:///path/to/legato` and use a local commit.
2. In Zed, open the command palette and run **zed: install dev extension**.
3. Select this directory (`editors/zed`).

Zed clones the grammar repo at `rev`, builds `src/parser.c` to WebAssembly, and
loads `languages/legato/`. Open a `.legato` file to confirm highlighting.

To pick up later grammar changes: push them, bump `rev`, then run
**zed: reload extensions** (or reinstall the dev extension).

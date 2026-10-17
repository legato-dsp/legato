#!/usr/bin/env bash
# Generate a tree-sitter corpus file from the .legato fixtures in test/sources/.
#
# These fixtures are the source strings lifted from the Rust e2e suite
# (crates e2e parse_and_lower tests). The e2e assertions are about the lowered
# IRGraph and can't live in a tree-sitter corpus, so this only pins the *parse*:
# it guarantees the grammar keeps accepting every real Legato program the
# pipeline consumes. Run after `tree-sitter generate`.
#
# Usage: ./test/gen_e2e_corpus.sh > test/corpus/e2e.txt
set -euo pipefail

cd "$(dirname "$0")/.."
sep_eq=$(printf '=%.0s' {1..80})
sep_dash=$(printf -- '-%.0s' {1..80})

for f in test/sources/*.legato; do
  name=$(basename "$f" .legato | tr '_' ' ')
  tree=$(tree-sitter parse "$f")
  # Strip the trailing position annotations tree-sitter parse appends
  # (e.g. " [0, 0] - [3, 1]") so the output is a clean corpus S-expression.
  tree=$(printf '%s\n' "$tree" | sed -E 's/ \[[0-9]+, [0-9]+\] - \[[0-9]+, [0-9]+\]//g')

  # Fail loudly if the fixture didn't parse cleanly — an ERROR node here means
  # a real grammar regression, not a corpus formatting issue.
  if printf '%s' "$tree" | grep -q 'ERROR\|MISSING'; then
    echo "ERROR: $f did not parse cleanly:" >&2
    printf '%s\n' "$tree" >&2
    exit 1
  fi

  printf '%s\n%s\n%s\n\n' "$sep_eq" "$name" "$sep_eq"
  cat "$f"
  printf '\n%s\n\n%s\n\n' "$sep_dash" "$tree"
done

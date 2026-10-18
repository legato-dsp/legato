package tree_sitter_legato_test

import (
	"testing"

	tree_sitter "github.com/tree-sitter/go-tree-sitter"
	tree_sitter_legato "github.com/tree-sitter/tree-sitter-legato/bindings/go"
)

func TestCanLoadGrammar(t *testing.T) {
	language := tree_sitter.NewLanguage(tree_sitter_legato.Language())
	if language == nil {
		t.Errorf("Error loading Legato grammar")
	}
}

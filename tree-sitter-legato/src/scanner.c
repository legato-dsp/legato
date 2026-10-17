#include "tree_sitter/parser.h"

// External scanner for tree-sitter-legato.
//
// Emits a single token, NEWLINE, used only to terminate a patch's virtual-port
// line (`in gate freq_in`). Everywhere else newlines are ordinary whitespace
// handled by the grammar's `extras`, so this token is requested by the parser
// exclusively at the end of `virtual_ports`. That makes the port list stop at
// the line break instead of greedily consuming the following scope's namespace.

enum TokenType {
  NEWLINE,
};

void *tree_sitter_legato_external_scanner_create(void) { return NULL; }
void tree_sitter_legato_external_scanner_destroy(void *p) { (void)p; }
unsigned tree_sitter_legato_external_scanner_serialize(void *p, char *b) {
  (void)p; (void)b; return 0;
}
void tree_sitter_legato_external_scanner_deserialize(void *p, const char *b, unsigned n) {
  (void)p; (void)b; (void)n;
}

static void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

bool tree_sitter_legato_external_scanner_scan(
    void *payload, TSLexer *lexer, const bool *valid_symbols) {
  (void)payload;

  if (!valid_symbols[NEWLINE]) {
    return false;
  }

  // Skip inline whitespace and an optional trailing line comment, then require
  // a newline (or EOF) to actually emit the token.
  bool saw_newline = false;
  for (;;) {
    if (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
        lexer->lookahead == '\r') {
      advance(lexer);
    } else if (lexer->lookahead == '/' ) {
      // Possible `// ...` trailing comment on the in-line. Peek one char; if not
      // a second slash we cannot consume it here, so bail to the normal lexer.
      advance(lexer);
      if (lexer->lookahead != '/') {
        return false;
      }
      while (lexer->lookahead != '\n' && lexer->lookahead != 0) {
        advance(lexer);
      }
    } else if (lexer->lookahead == '\n') {
      advance(lexer);
      saw_newline = true;
      break;
    } else if (lexer->lookahead == 0) {
      // EOF also legitimately ends the line.
      saw_newline = true;
      break;
    } else {
      // Another identifier or token on the same line: not a terminator.
      return false;
    }
  }

  if (saw_newline) {
    lexer->result_symbol = NEWLINE;
    return true;
  }
  return false;
}

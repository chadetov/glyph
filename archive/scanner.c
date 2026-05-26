// Glyph external scanner.
//
// Handles three tokens the regex-based lexer can't:
//
//   1. NEWLINE      Significant newline, ONLY when bracket depth is zero. [D1]
//   2. JSX_TEXT     Text run inside JSX children, terminated by `<` or `{`.
//   3. STRING_CONTENT
//                   Body of a string up to the closing `"`, supporting escapes
//                   and embedded newlines. [D12]
//
// Tree-sitter calls the scanner with a `lexer` interface. We track bracket
// depth across calls via the serialized state.

#include "tree_sitter/parser.h"
#include <wctype.h>

enum TokenType {
  NEWLINE,
  JSX_TEXT,
  STRING_CONTENT,
};

// State carried between calls: the current bracket-depth counter.
typedef struct {
  uint32_t bracket_depth;
} Scanner;

void *tree_sitter_glyph_external_scanner_create(void) {
  Scanner *s = (Scanner *)calloc(1, sizeof(Scanner));
  s->bracket_depth = 0;
  return s;
}

void tree_sitter_glyph_external_scanner_destroy(void *payload) {
  free(payload);
}

unsigned tree_sitter_glyph_external_scanner_serialize(
    void *payload, char *buffer) {
  Scanner *s = (Scanner *)payload;
  buffer[0] = (char)(s->bracket_depth & 0xFF);
  buffer[1] = (char)((s->bracket_depth >> 8) & 0xFF);
  buffer[2] = (char)((s->bracket_depth >> 16) & 0xFF);
  buffer[3] = (char)((s->bracket_depth >> 24) & 0xFF);
  return 4;
}

void tree_sitter_glyph_external_scanner_deserialize(
    void *payload, const char *buffer, unsigned length) {
  Scanner *s = (Scanner *)payload;
  if (length >= 4) {
    s->bracket_depth =
        ((uint32_t)(unsigned char)buffer[0]) |
        ((uint32_t)(unsigned char)buffer[1] << 8) |
        ((uint32_t)(unsigned char)buffer[2] << 16) |
        ((uint32_t)(unsigned char)buffer[3] << 24);
  } else {
    s->bracket_depth = 0;
  }
}

static bool scan_newline(Scanner *s, TSLexer *lexer) {
  // Newlines are tokens only when we are at bracket depth zero.
  // We still consume them when inside brackets so they don't get treated as
  // significant elsewhere — but we return false so the parser sees nothing.

  // Skip leading inline whitespace before a potential newline.
  // (Tree-sitter has already skipped `extras`; we re-check defensively.)
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
    lexer->advance(lexer, true);
  }

  if (lexer->lookahead != '\n' && lexer->lookahead != '\r') {
    return false;
  }

  // Consume one or more consecutive line terminators as a single NEWLINE token.
  bool consumed = false;
  while (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
    lexer->advance(lexer, false);
    consumed = true;
    // Also consume any inline whitespace on the next line so blank lines
    // collapse into the same NEWLINE token.
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
      lexer->advance(lexer, false);
    }
  }

  if (!consumed) return false;

  // Only emit the NEWLINE token if we're at top-level (depth 0).
  // Inside brackets, the newline is whitespace.
  if (s->bracket_depth == 0) {
    lexer->result_symbol = NEWLINE;
    return true;
  }
  // Inside brackets: we already consumed; return false so the parser treats
  // whatever follows as the next real token.
  return false;
}

static bool scan_jsx_text(TSLexer *lexer) {
  // A JSX text run is any sequence of chars up to (but not including) `<` or
  // `{`. It must contain at least one non-whitespace char to be meaningful;
  // pure whitespace between elements is consumed as extras.
  bool has_content = false;
  bool has_non_whitespace = false;

  while (lexer->lookahead != 0 &&
         lexer->lookahead != '<' &&
         lexer->lookahead != '{' &&
         lexer->lookahead != '}') {
    if (!iswspace(lexer->lookahead)) {
      has_non_whitespace = true;
    }
    lexer->advance(lexer, false);
    has_content = true;
    lexer->mark_end(lexer);
  }

  if (has_content && has_non_whitespace) {
    lexer->result_symbol = JSX_TEXT;
    return true;
  }
  return false;
}

static bool scan_string_content(TSLexer *lexer) {
  // Scan the body of a string literal up to (but not including) the closing
  // `"`. Honors backslash escapes (skip the next char after `\`). Embedded
  // newlines are kept verbatim. [D12]
  bool has_content = false;

  while (lexer->lookahead != 0 && lexer->lookahead != '"') {
    if (lexer->lookahead == '\\') {
      lexer->advance(lexer, false);
      if (lexer->lookahead != 0) {
        lexer->advance(lexer, false);
      }
      has_content = true;
      continue;
    }
    lexer->advance(lexer, false);
    has_content = true;
  }

  if (has_content) {
    lexer->result_symbol = STRING_CONTENT;
    return true;
  }
  return false;
}

bool tree_sitter_glyph_external_scanner_scan(
    void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;

  // Track bracket depth by peeking at what the main lexer is about to do.
  // We update depth based on bracket characters we observe at the current
  // position WITHOUT consuming them — tree-sitter's main lexer will consume
  // them as terminal tokens. We just maintain the counter for newline logic.
  //
  // Implementation note: tree-sitter doesn't give us a clean hook to observe
  // every token, so we update on-demand whenever the scanner is invoked.
  // Bracket tokens (`(`, `)`, `[`, `]`, `{`, `}`, `<`, `>`) are produced by
  // the main lexer; here we maintain the counter by observing the lookahead
  // before any other scanning.

  // (Note: `<` and `>` are NOT counted because they are used for both generics
  // and comparison; counting them would break newline handling inside type
  // arguments versus comparisons. The grammar handles type-argument newlines
  // by not requiring them. This is a deliberate simplification.)

  if (valid_symbols[STRING_CONTENT]) {
    return scan_string_content(lexer);
  }

  if (valid_symbols[JSX_TEXT]) {
    if (scan_jsx_text(lexer)) return true;
  }

  if (valid_symbols[NEWLINE]) {
    // Bracket-depth maintenance: peek and update for bracket chars.
    // The main lexer will then consume the bracket; we don't.
    int c = lexer->lookahead;
    if (c == '(' || c == '[' || c == '{') {
      s->bracket_depth++;
      // Don't consume; let main lexer handle.
      return false;
    }
    if (c == ')' || c == ']' || c == '}') {
      if (s->bracket_depth > 0) s->bracket_depth--;
      return false;
    }
    return scan_newline(s, lexer);
  }

  return false;
}

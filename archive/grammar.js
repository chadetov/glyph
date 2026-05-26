/**
 * Tree-sitter grammar for Glyph
 *
 * Grammar is the spec. Every rule below corresponds to a decision in
 * SPEC_DECISIONS.md (referenced as [Dn]) or PRECEDENCE.md.
 *
 * To regenerate the parser:
 *   npm install
 *   npx tree-sitter generate
 *   npx tree-sitter parse examples/01_validator.glyph
 *
 * To test against the corpus:
 *   npx tree-sitter parse examples/*.glyph
 */

// Precedence levels match PRECEDENCE.md exactly.
// Higher number = tighter binding in tree-sitter's `prec`.
const PREC = {
  // Level 12 (loosest in PRECEDENCE.md) is assignment — statement-level only,
  // so it doesn't participate in the expression precedence chain.
  AWAIT:        1,   // PRECEDENCE.md level 11 (prefix await)
  NULLISH:      2,   // level 10  ??       right
  LOGICAL_OR:   3,   // level 9   ||       left
  LOGICAL_AND:  4,   // level 8   &&       left
  EQUALITY:     5,   // level 7   == !=    left
  COMPARISON:   6,   // level 6   < <= > >=  left
  ADDITIVE:     7,   // level 5   + -      left
  MULTIPLICATIVE: 8, // level 4   * / %    left
  PREFIX_UNARY: 9,   // level 3   ! -      right
  POSTFIX_TRY:  10,  // level 2   ?        postfix; binds tighter than member
  MEMBER:       11,  // level 1   . ?. [] ()  left
  // Used to bias type-vs-value disambiguation; not in PRECEDENCE.md.
  TYPE_GENERIC: 20,
};

module.exports = grammar({
  name: 'glyph',

  extras: $ => [
    /[ \t]/,           // whitespace, but NOT newlines (D1: newlines are significant)
    $.line_comment,
  ],

  // Words: identifiers that might also be keywords are routed through here so
  // the lexer doesn't tokenize `match` as an identifier and confuse the parser.
  word: $ => $.identifier,

  // Conflicts the GLR engine needs to resolve dynamically.
  // Each entry says "these productions look alike for a few tokens; pick whichever
  // ends up valid."
  conflicts: $ => [
    // `Name<T>(...)` — generic call vs (Name < T) > (...)
    [$.call_expression, $.binary_expression],
    // `fn name<T>(...)` declaration vs `fn(...)` expression at parse start
    [$._function_declaration_head, $._function_expression_head],
    // Pattern `Name({ x })` vs expression `Name({ x })` inside match arms
    [$.constructor_pattern, $.call_expression],
    [$.object_pattern, $.object_literal],
    [$.array_pattern, $.array_literal],
    // Identifier-as-pattern vs identifier-as-expression in match arms
    [$.identifier_pattern, $.identifier_expression],
  ],

  externals: $ => [
    $._newline,            // significant newline (D1: outside brackets only)
    $._jsx_text,           // text run inside JSX children
    $._string_content,     // body of a string literal up to the closing quote
  ],

  rules: {
    // ------------------------------------------------------------------------
    // Source file
    // ------------------------------------------------------------------------

    source_file: $ => seq(
      repeat($._newline),
      optional($.module_declaration),
      repeat(seq($._top_level_item, repeat1($._newline))),
      optional($._top_level_item),
    ),

    module_declaration: $ => seq(
      'module',
      field('path', $.module_path),
      repeat1($._newline),
    ),

    module_path: $ => sep1($.identifier, '/'),

    _top_level_item: $ => choice(
      $.import_declaration,
      $.type_declaration,
      $.function_declaration,
      $.component_declaration,
      $.const_declaration,
      $.expression_statement,    // top-level expression statements appear in
                                 // file 01 (the `match User.parse(input)` block)
    ),

    // ------------------------------------------------------------------------
    // Imports [D15]
    // ------------------------------------------------------------------------

    import_declaration: $ => seq(
      'import',
      field('path', $.module_path),
      choice(
        // import std/http
        seq(),
        // import std/http as h
        seq('as', field('alias', $.identifier)),
        // import std/result { Result, Ok, Err }
        seq(
          '{',
          sep1(field('name', $.identifier), ','),
          optional(','),
          '}',
        ),
      ),
    ),

    // ------------------------------------------------------------------------
    // Type declarations [D8, D16]
    // ------------------------------------------------------------------------

    type_declaration: $ => seq(
      'type',
      field('name', $.identifier),
      optional($.generic_parameters),
      '=',
      field('definition', choice(
        $.tagged_union,
        $._type_expression,
      )),
    ),

    // Tagged unions: leading `|` required for multi-line, omitted for single-line.
    // Both shapes share one rule; the grammar accepts both. The formatter
    // enforces the canonical shape based on whether the union is multi-line.
    tagged_union: $ => prec.right(seq(
      optional('|'),
      $._union_variant,
      repeat(seq('|', $._union_variant)),
    )),

    _union_variant: $ => choice(
      // Bare: `Idle`
      field('tag', $.constructor_name),
      // Payload: `Loaded({ users: Array<User> })`
      seq(
        field('tag', $.constructor_name),
        '(',
        field('payload', $._type_expression),
        ')',
      ),
    ),

    // Constructor names are capitalized identifiers. Greppability: every
    // constructor declaration has the same syntactic shape.
    constructor_name: $ => /[A-Z][a-zA-Z0-9_]*/,

    // ------------------------------------------------------------------------
    // Type expressions [D7]
    // ------------------------------------------------------------------------

    _type_expression: $ => choice(
      $.type_reference,
      $.record_type,
      $.function_type,
      $.tuple_type,
    ),

    type_reference: $ => prec.left(seq(
      choice($.identifier, $.constructor_name, $.qualified_type_name),
      optional($.generic_arguments),
    )),

    qualified_type_name: $ => seq(
      $.identifier,
      repeat1(seq('.', choice($.identifier, $.constructor_name))),
    ),

    generic_parameters: $ => seq(
      '<',
      sep1(field('param', $.identifier), ','),
      optional(','),
      '>',
    ),

    generic_arguments: $ => prec(PREC.TYPE_GENERIC, seq(
      '<',
      sep1(field('arg', $._type_expression), ','),
      optional(','),
      '>',
    )),

    record_type: $ => seq(
      '{',
      repeat($._newline),
      optional(seq(
        sep1Trailing($._record_type_field, ','),
      )),
      '}',
    ),

    _record_type_field: $ => seq(
      field('name', $.identifier),
      optional('?'),                     // optional field: `placeholder?: string`
      ':',
      field('type', $._type_expression),
    ),

    function_type: $ => seq(
      'fn',
      '(',
      optional(sep1Trailing($._function_type_param, ',')),
      ')',
      '->',
      field('return_type', $._type_expression),
    ),

    _function_type_param: $ => choice(
      // Named: `input: unknown`
      seq(field('name', $.identifier), ':', field('type', $._type_expression)),
      // Anonymous: just a type
      $._type_expression,
    ),

    tuple_type: $ => seq(
      '(',
      $._type_expression,
      ',',
      sep1Trailing($._type_expression, ','),
      ')',
    ),

    // ------------------------------------------------------------------------
    // Function declarations [D4]
    // ------------------------------------------------------------------------

    function_declaration: $ => seq(
      $._function_declaration_head,
      field('body', $.block),
    ),

    _function_declaration_head: $ => seq(
      optional('async'),
      'fn',
      field('name', $.identifier),
      optional($.generic_parameters),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type_expression))),
    ),

    parameter_list: $ => seq(
      '(',
      repeat($._newline),
      optional(sep1Trailing($.parameter, ',')),
      repeat($._newline),
      ')',
    ),

    parameter: $ => seq(
      field('name', $.identifier),
      optional('?'),
      optional(seq(':', field('type', $._type_expression))),
    ),

    // ------------------------------------------------------------------------
    // Component declarations [D19]
    // ------------------------------------------------------------------------

    component_declaration: $ => seq(
      'component',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type_expression))),
      field('body', $.block),
    ),

    // ------------------------------------------------------------------------
    // Module-level const [D20]
    // ------------------------------------------------------------------------

    const_declaration: $ => seq(
      'const',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type_expression))),
      '=',
      field('value', $._expression),
    ),

    // ------------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------------

    block: $ => seq(
      '{',
      repeat($._newline),
      repeat(seq($._statement, repeat1($._newline))),
      optional($._statement),
      '}',
    ),

    _statement: $ => choice(
      $.let_statement,
      $.mut_statement,
      $.return_statement,
      $.for_statement,
      $.expression_statement,
    ),

    // [D20] `let` is function-local only. The grammar enforces this by only
    // including `let_statement` inside `block`, never at the top level.
    let_statement: $ => seq(
      'let',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type_expression))),
      '=',
      field('value', $._expression),
    ),

    // [D5] `mut` prefixes assignments or method calls only.
    mut_statement: $ => seq(
      'mut',
      choice(
        $.assignment,
        $.method_call_statement,
      ),
    ),

    assignment: $ => seq(
      field('target', $._assignment_target),
      '=',
      field('value', $._expression),
    ),

    _assignment_target: $ => choice(
      $.identifier_expression,
      $.member_expression,
      $.index_expression,
    ),

    // A method call legal as the body of `mut`: receiver.method(args).
    // Free function calls (`mut foo()`) are syntactically rejected because
    // there's no receiver.
    method_call_statement: $ => prec(PREC.MEMBER, seq(
      field('receiver', $._expression),
      '.',
      field('method', $.identifier),
      field('arguments', $.argument_list),
    )),

    return_statement: $ => seq(
      'return',
      optional(field('value', $._expression)),
    ),

    for_statement: $ => seq(
      'for',
      // Single binding: `for item in xs`
      // Pair binding: `for key, value in xs`
      field('binding', choice(
        $.identifier,
        seq($.identifier, ',', $.identifier),
      )),
      'in',
      field('iterable', $._expression),
      field('body', $.block),
    ),

    expression_statement: $ => $._expression,

    // ------------------------------------------------------------------------
    // Expressions — precedence per PRECEDENCE.md [D18]
    // ------------------------------------------------------------------------

    _expression: $ => choice(
      $.match_expression,
      $.binary_expression,
      $.unary_expression,
      $.await_expression,
      $.try_expression,
      $.call_expression,
      $.member_expression,
      $.optional_chain_expression,
      $.index_expression,
      $.function_expression,
      $.jsx_element,
      $.object_literal,
      $.array_literal,
      $.identifier_expression,
      $.constructor_expression,
      $.literal,
      $.parenthesized_expression,
      $.spread_expression,
    ),

    // -- match (expression form) [D3] ----------------------------------------

    match_expression: $ => seq(
      'match',
      field('scrutinee', $._expression),
      '{',
      repeat($._newline),
      repeat(seq($.match_arm, repeat($._newline))),
      '}',
    ),

    // [D2] Every arm requires a trailing comma, including the last.
    match_arm: $ => seq(
      field('pattern', $._pattern),
      '=>',
      field('body', choice(
        $._expression,
        $.block,
      )),
      ',',
    ),

    // -- Binary / unary / await / try ----------------------------------------

    binary_expression: $ => {
      const table = [
        [PREC.NULLISH,        '??',  'right'],
        [PREC.LOGICAL_OR,     '||',  'left'],
        [PREC.LOGICAL_AND,    '&&',  'left'],
        [PREC.EQUALITY,       '==',  'left'],
        [PREC.EQUALITY,       '!=',  'left'],
        [PREC.COMPARISON,     '<',   'left'],
        [PREC.COMPARISON,     '<=',  'left'],
        [PREC.COMPARISON,     '>',   'left'],
        [PREC.COMPARISON,     '>=',  'left'],
        [PREC.ADDITIVE,       '+',   'left'],
        [PREC.ADDITIVE,       '-',   'left'],
        [PREC.MULTIPLICATIVE, '*',   'left'],
        [PREC.MULTIPLICATIVE, '/',   'left'],
        [PREC.MULTIPLICATIVE, '%',   'left'],
      ];

      return choice(...table.map(([p, op, assoc]) => {
        const fn = assoc === 'right' ? prec.right : prec.left;
        return fn(p, seq(
          field('left', $._expression),
          field('operator', op),
          field('right', $._expression),
        ));
      }));
    },

    unary_expression: $ => prec.right(PREC.PREFIX_UNARY, seq(
      field('operator', choice('!', '-')),
      field('operand', $._expression),
    )),

    // [PRECEDENCE.md] `await` binds looser than `?`. So `await x?` is `(await x)?`.
    // Implemented by giving `await` a lower precedence than `try` (the `?` op).
    await_expression: $ => prec.right(PREC.AWAIT, seq(
      'await',
      field('value', $._expression),
    )),

    // Postfix `?` for Result propagation. Binds tighter than `.` per PRECEDENCE.md.
    try_expression: $ => prec(PREC.POSTFIX_TRY, seq(
      field('value', $._expression),
      '?',
    )),

    // -- Member / call / index / optional chaining ---------------------------

    member_expression: $ => prec.left(PREC.MEMBER, seq(
      field('object', $._expression),
      '.',
      field('property', $.identifier),
    )),

    // [PRECEDENCE.md] `?.` is a single token, distinct from postfix `?`.
    optional_chain_expression: $ => prec.left(PREC.MEMBER, seq(
      field('object', $._expression),
      '?.',
      field('property', $.identifier),
    )),

    call_expression: $ => prec.left(PREC.MEMBER, seq(
      field('callee', $._expression),
      optional($.generic_arguments),
      field('arguments', $.argument_list),
    )),

    index_expression: $ => prec.left(PREC.MEMBER, seq(
      field('object', $._expression),
      '[',
      field('index', $._expression),
      ']',
    )),

    argument_list: $ => seq(
      '(',
      repeat($._newline),
      optional(sep1Trailing($._expression, ',')),
      repeat($._newline),
      ')',
    ),

    // -- Function expressions [D4] -------------------------------------------

    function_expression: $ => seq(
      $._function_expression_head,
      field('body', $.block),
    ),

    _function_expression_head: $ => seq(
      optional('async'),
      'fn',
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type_expression))),
    ),

    // -- Object & array literals [D10, D11] ----------------------------------

    object_literal: $ => seq(
      '{',
      repeat($._newline),
      optional(sep1Trailing($._object_member, ',')),
      repeat($._newline),
      '}',
    ),

    _object_member: $ => choice(
      $.object_field,
      $.spread_expression,
    ),

    // [D10] No shorthand: `name: value` is the only legal form.
    object_field: $ => seq(
      field('key', choice($.identifier, $.string_literal)),
      ':',
      field('value', $._expression),
    ),

    array_literal: $ => seq(
      '[',
      repeat($._newline),
      optional(sep1Trailing(
        choice($._expression, $.spread_expression),
        ',',
      )),
      repeat($._newline),
      ']',
    ),

    // [D11] Spread is an expression form usable in arrays, objects, and arg lists.
    spread_expression: $ => prec(PREC.PREFIX_UNARY, seq(
      '...',
      field('value', $._expression),
    )),

    // -- Identifiers and constructors as expressions -------------------------

    // An identifier in expression position (a binding reference).
    identifier_expression: $ => $.identifier,

    // A constructor reference: `Help`, `Idle`, `Loaded`, etc. Distinguished by
    // capitalization at the lexer level. Greppability pillar.
    constructor_expression: $ => $.constructor_name,

    parenthesized_expression: $ => seq(
      '(',
      $._expression,
      ')',
    ),

    // ------------------------------------------------------------------------
    // Patterns [D9]
    // ------------------------------------------------------------------------

    _pattern: $ => choice(
      $.wildcard_pattern,         // _
      $.else_pattern,             // else (catch-all arm only)
      $.literal_pattern,
      $.identifier_pattern,
      $.constructor_pattern,
      $.type_guard_pattern,       // `is string`, `is Array<unknown>`
      $.array_pattern,
      $.object_pattern,
    ),

    wildcard_pattern: $ => '_',

    else_pattern: $ => 'else',

    literal_pattern: $ => choice(
      $.number_literal,
      $.string_literal,
      $.boolean_literal,
    ),

    // A bare identifier in pattern position binds it.
    identifier_pattern: $ => $.identifier,

    // `Ok(user)`, `Err(NetworkError({ status }))`, `Loaded({ users })`.
    constructor_pattern: $ => seq(
      field('tag', $.constructor_name),
      optional(seq(
        '(',
        optional(sep1Trailing($._pattern, ',')),
        ')',
      )),
    ),

    type_guard_pattern: $ => seq(
      'is',
      field('type', $._type_expression),
    ),

    // `[]`, `["help", ..._]`, `[other, ..._]`, `["add", ...rest]`
    array_pattern: $ => seq(
      '[',
      optional(sep1Trailing(
        choice($._pattern, $.rest_pattern),
        ',',
      )),
      ']',
    ),

    rest_pattern: $ => seq(
      '...',
      choice($.identifier, $.wildcard_pattern),
    ),

    // `{ name }`, `{ name, age }`, `{ status }` — pattern-position destructure.
    // Note: in patterns, shorthand IS allowed (`{ status }` means "bind `status`
    // to the field named `status`"). This is a deliberate asymmetry with object
    // literals [D10]: in patterns the field name and the binding name are
    // typically the same and forcing `{ status: status }` would be noise.
    object_pattern: $ => seq(
      '{',
      optional(sep1Trailing($._object_pattern_field, ',')),
      '}',
    ),

    _object_pattern_field: $ => choice(
      // Shorthand: `{ status }`
      field('name', $.identifier),
      // Renamed: `{ status: s }`
      seq(
        field('name', $.identifier),
        ':',
        field('binding', $._pattern),
      ),
    ),

    // ------------------------------------------------------------------------
    // JSX [D6]
    // ------------------------------------------------------------------------

    jsx_element: $ => choice(
      $.jsx_self_closing,
      $.jsx_paired,
    ),

    jsx_self_closing: $ => seq(
      '<',
      field('tag', $._jsx_tag_name),
      repeat($.jsx_attribute),
      '/>',
    ),

    jsx_paired: $ => seq(
      $.jsx_opening,
      repeat($._jsx_child),
      $.jsx_closing,
    ),

    jsx_opening: $ => seq(
      '<',
      field('tag', $._jsx_tag_name),
      repeat($.jsx_attribute),
      '>',
    ),

    jsx_closing: $ => seq(
      '</',
      field('tag', $._jsx_tag_name),
      '>',
    ),

    // Tags are either lowercase HTML-style (`div`), capitalized component refs
    // (`ResultsList`), or one of the reserved directive names. The grammar
    // treats them uniformly; the typechecker enforces directive semantics.
    _jsx_tag_name: $ => choice(
      $.identifier,
      $.constructor_name,
    ),

    // [D6] Attributes: positional (just a constructor name like `Loaded`),
    // boolean (just an identifier), or named with value (`name="value"` or
    // `name={expr}`).
    jsx_attribute: $ => choice(
      // Positional constructor attribute: `<case Loaded>`
      field('positional', $.constructor_name),
      // Named: `class="foo"` or `value={expr}`
      seq(
        field('name', $.identifier),
        optional(seq(
          '=',
          field('value', choice(
            $.string_literal,
            $.jsx_expression,
          )),
        )),
      ),
    ),

    jsx_expression: $ => seq(
      '{',
      $._expression,
      '}',
    ),

    _jsx_child: $ => choice(
      $.jsx_element,
      $.jsx_expression,
      $._jsx_text,            // external token: text run between elements
    ),

    // ------------------------------------------------------------------------
    // Literals
    // ------------------------------------------------------------------------

    literal: $ => choice(
      $.number_literal,
      $.string_literal,
      $.boolean_literal,
      $.void_literal,
    ),

    // [D13] Integers and decimals; underscore separators allowed.
    number_literal: $ => /-?\d(\d|_)*(\.\d(\d|_)*)?([eE][+-]?\d+)?/,

    // [D12] Double-quoted, with escapes; newlines inside are literal.
    string_literal: $ => seq(
      '"',
      optional($._string_content),
      '"',
    ),

    boolean_literal: $ => choice('true', 'false'),

    // [D16] `void` is both a type and a value. As a value it's a literal.
    // As a type it's matched by type_reference (lexically just an identifier).
    void_literal: $ => 'void',

    // ------------------------------------------------------------------------
    // Lexical
    // ------------------------------------------------------------------------

    // [D14] `//` line comments only.
    line_comment: $ => token(seq('//', /[^\n]*/)),

    // Identifiers: lowercase or underscore start. Capitalized identifiers are
    // constructor_name (handled separately). This split is the lexer-level
    // greppability pillar: `Foo` is always a type or constructor, `foo` is
    // always a value or function.
    identifier: $ => /[a-z_][a-zA-Z0-9_]*/,
  },
});

// -- Helpers --------------------------------------------------------------

// One-or-more `rule` separated by `sep`, with NO trailing separator.
function sep1(rule, sep) {
  return seq(rule, repeat(seq(sep, rule)));
}

// One-or-more `rule` separated by `sep`, with optional trailing separator. [D17]
function sep1Trailing(rule, sep) {
  return seq(rule, repeat(seq(sep, rule)), optional(sep));
}

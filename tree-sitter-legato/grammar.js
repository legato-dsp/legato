/**
 * @file Legato DSL grammar for tree-sitter
 * @license MIT
 */

/* eslint-disable arrow-parens */
/* eslint-disable camelcase */
/* eslint-disable-next-line spaced-comment */
/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
  name: 'legato',

  extras: $ => [/\s/, $.comment],

  word: $ => $.identifier,

  // External scanner emits `_newline` at the end of a virtual-ports `in` line.
  // Whitespace is in `extras` and thus invisible to the parser, so this is the
  // only way to mark where the port list stops and the next scope begins.
  externals: $ => [$._newline],

  conflicts: $ => [
    // `{ ident: value, ... }` is shaped identically as node params and as an
    // object literal; they occur in different positions but share the prefix.
    [$.params, $.object],
  ],

  rules: {
    // top-level: source? macros* scopes* connections* sink
    // mirrors legato_parser_inner: every section is extra_padded, source/connections optional.
    // `patch` and `kernel` are the same production in the legato parser (both parsed by
    // patch_parser, distinguished only by MacroKind), so they interleave in one repeat.
    source_file: $ => seq(
      optional($.source),
      repeat(choice($.patch, $.kernel)),
      repeat($.scope),
      repeat($.connection),
      $.sink,
    ),

    // `{ ident }` appearing before any patch/scope
    source: $ => prec(1, $._braced_ident),
    // `{ ident }` appearing last
    sink: $ => $._braced_ident,

    _braced_ident: $ => seq('{', field('name', $.identifier), '}'),

    // patch NAME(defaults?) { vports? scopes connections? sink }
    patch: $ => seq(
      'patch',
      field('name', $.identifier),
      optional(field('default_params', $.default_params)),
      field('body', $.patch_body),
    ),

    // kernel NAME(defaults?) { ... } — structurally identical to `patch`; the keyword
    // selects the per-sample execution model (see legato parse.rs MacroKind::Kernel).
    kernel: $ => seq(
      'kernel',
      field('name', $.identifier),
      optional(field('default_params', $.default_params)),
      field('body', $.patch_body),
    ),

    patch_body: $ => seq(
      '{',
      optional($.virtual_ports),
      repeat($.scope),
      repeat($.connection),
      $.sink,
      '}',
    ),

    default_params: $ => seq(
      '(',
      commaSep(seq(
        field('name', $.identifier),
        '=',
        field('value', $.value),
      )),
      optional(','),
      ')',
    ),

    // `in gate freq_in` — one or more idents on a single line, terminated by a
    // newline emitted by the external scanner. The newline is what stops the
    // port list from swallowing the following scope's namespace identifier.
    virtual_ports: $ => seq(
      'in',
      repeat1(field('port', $.identifier)),
      $._newline,
    ),

    // NAME { decl, decl, }
    scope: $ => seq(
      field('namespace', $.identifier),
      '{',
      commaSep($.node_declaration),
      optional(','),
      '}',
    ),

    // type (:alias)? (*count)? params? pipes*
    node_declaration: $ => seq(
      field('node_type', $.identifier),
      optional(seq(':', field('alias', $.identifier))),
      optional(seq('*', field('count', $.uint))),
      optional(field('params', $.params)),
      repeat(field('pipe', $.pipe)),
    ),

    params: $ => seq(
      '{',
      commaSep($.param),
      optional(','),
      '}',
    ),

    param: $ => seq(
      field('key', $.identifier),
      ':',
      field('value', $.value),
    ),

    // | name(value?)
    pipe: $ => seq(
      '|',
      field('name', $.identifier),
      optional(seq('(', optional(field('params', $.value)), ')')),
    ),

    // endpoint >> endpoint (>> endpoint)*
    connection: $ => prec.left(seq(
      $.endpoint,
      repeat1(seq('>>', $.endpoint)),
    )),

    endpoint: $ => seq(
      field('node', $.identifier),
      optional(field('selector', $.node_selector)),
      optional(field('port', $.port)),
    ),

    // (n) | (n..m) | (*)
    node_selector: $ => seq(
      '(',
      choice(
        seq($.uint, '..', $.uint),
        $.uint,
        '*',
      ),
      ')',
    ),

    port: $ => choice(
      seq('.', field('name', $.identifier)),               // .mono
      seq('[', $.uint, ':', $.uint, ':', $.uint, ']'),     // [start:end:step]
      seq('[', $.uint, '..', $.uint, ']'),                 // [s..e]
      seq('[', $.uint, ']'),                               // [i]
    ),

    // ---- values ----
    value: $ => choice(
      $.float,
      $.integer,
      $.string,
      $.template,
      $.object,
      $.array,
      $.boolean,
      $.null,
      $.identifier,
    ),

    object: $ => seq(
      '{',
      commaSep($.object_entry),
      optional(','),
      '}',
    ),

    object_entry: $ => seq(
      field('key', $.identifier),
      ':',
      field('value', $.value),
    ),

    array: $ => seq(
      '[',
      commaSep($.value),
      optional(','),
      ']',
    ),

    // $ident
    template: $ => token(seq('$', /[a-zA-Z_]\w*/)),

    // f32: optional '-', int, '.', digits  -> higher token prec than integer
    float: $ => token(prec(2, /-?\d+\.\d+/)),

    // i32 (negative) or u32 (bare). Single node; sign is part of the token.
    integer: $ => token(prec(1, /-?\d+/)),

    // unsigned-only, used for counts, selectors and port indices (parser's `uint`)
    uint: _ => /\d+/,

    string: $ => seq(
      '"',
      repeat(choice(
        $.escape_sequence,
        token.immediate(prec(1, /[^"\\]+/)),
      )),
      '"',
    ),

    escape_sequence: _ => token.immediate(seq('\\', /["\\/nrt]/)),

    boolean: _ => choice('true', 'false'),
    null: _ => 'null',

    identifier: _ => /[a-zA-Z_]\w*/,

    comment: _ => token(choice(
      seq('//', /[^\n]*/),
      seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/'),
    )),
  },
});

/**
 * Comma-separated sequence of `rule`, no trailing comma (handle that at call site).
 * @param {RuleOrLiteral} rule
 * @returns {ChoiceRule}
 */
function commaSep(rule) {
  return optional(seq(rule, repeat(seq(',', rule))));
}

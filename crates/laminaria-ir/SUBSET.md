# Declared subset acceptance table (issue #27 A1)

Every accept/reject/ignore branch in `rust_frontend.rs`/`nim_frontend.rs`,
mapped to the real function/match-arm that implements it and the test that
exercises it. Built by reading both frontends' current source in full
(`rust_frontend.rs`, `nim_frontend.rs`), not from memory of "what the
subset generally covers" -- a row with **no test** is a real, named gap
this table exists to surface, not a decorative completeness claim.

Legend: **A** = accepted into the IR, **R** = rejected with a diagnostic
(never a panic, never a partial `Program`, never a fallback to invoking
`rustc`/`nim` -- `docs/compiler-ownership-contract.md`), **I** = ignored
(not inspected at all, because it lies outside the requested-functions
closure).

## File level

| Construct | Rust | Nim | Implementing code | Test |
|---|---|---|---|---|
| A file that doesn't parse/tokenize at all | R (`ParseError`) | R (`ParseError`, from `Lexer::tokenize`) | `lower_rust_source`'s `syn::parse_file` map_err; `Lexer::tokenize` | No dedicated test (exercised transitively by every malformed-source test below, which all fail earlier at parse time for genuinely broken syntax) |
| A top-level item that is not a requested function (a `main`, a `const`, an unrelated `struct`, Nim's own trailing `let`/`for` driver) | I | I | Rust: `by_name` only indexes `Item::Fn`, only requested names looked up. Nim: top-level loop only recurses into `proc` at column 1, else `parser.bump()`s past it | `rejects_a_struct_definition_referenced_from_the_requested_function` (struct itself ignored; only the *use* of it fails); `nim_frontend::ignores_unrelated_trailing_driver_code` |
| A requested function name not declared in the file | R (`UnsupportedShape`) | R (`UnsupportedShape`) | Rust: `by_name.get(*name)` else-branch; Nim: `found` set checked after the scan loop | `rust_frontend`: no dedicated test (only exercised via the Nim mirror); `nim_frontend::rejects_a_request_for_an_undeclared_function` |
| Lowering's own postcondition: the resulting `Program` fails `validate::validate_program` | R (`PostconditionViolated`) | R (`PostconditionViolated`) | Both frontends' own return path, issue #27 A2 | `validate::tests::a_program_produced_by_the_real_rust_frontend_validates` / `..._nim_frontend_validates` (positive side only -- this is a live regression guard neither frontend's own correct logic is expected to trip, not a currently reachable rejection) |
| A file-level inner attribute (`#![cfg(..)]`, `#![feature(..)]`, ...) | R | n/a (Nim has no file-level pragma equivalent in this subset) | `reject_unsupported_attrs(&file.attrs, ..)`, checked before any item is even looked at | `rejects_a_file_level_cfg_attribute` |
| A file-level doc comment (`//!`/`/*! */`) | A | n/a | Same helper, `attr.path().is_ident("doc")` allow-list | `accepts_a_file_level_doc_comment` |
| A *requested* function declared more than once in the file | R | n/a (Nim's own top-level scan re-parses each `proc` independently and the last one wins via `program.insert`; a genuine duplicate-`proc` check is not implemented on the Nim side -- a named, open asymmetry, not claimed equivalent) | `by_name` grouped by name (`Vec<&ItemFn>` per name, not overwritten via a single `insert`), checked before lowering begins | `rejects_a_duplicate_declaration_of_a_requested_function`, `rejects_a_duplicate_declaration_of_a_function_called_by_another_requested_function` |

## Function declaration level

| Construct | Rust | Nim | Implementing code | Test |
|---|---|---|---|---|
| Ordinary named function, `i32`/`int32` params and return | A | A | `lower_item_fn`; `parse_proc` | `lowers_the_existing_workload_functions_and_matches_real_semantics` (both frontends) |
| `async fn` | R | n/a (no Nim equivalent in this subset) | `item_fn.sig.asyncness` check | `rejects_an_async_function` |
| `unsafe fn` | R | n/a | `item_fn.sig.unsafety` check | `rejects_an_unsafe_function` |
| `extern "C" fn` | R | n/a | `item_fn.sig.abi` check | `rejects_an_extern_fn` |
| Generic function (`fn f<T>(..)`), including one whose params/return happen to be `i32` | R | n/a | `item_fn.sig.generics.params` check | `rejects_a_generic_function`, `rejects_a_generic_function_even_when_its_types_are_all_i32` |
| `where` clause | R | n/a | `item_fn.sig.generics.where_clause` check | `rejects_a_where_clause` |
| Non-`#[doc]` attribute on the function itself (e.g. `#[cfg(..)]`) | R | n/a (Nim pragmas: see below) | `reject_unsupported_attrs(&item_fn.attrs, ..)` | `rejects_a_cfg_attribute` |
| `#[doc = ..]` attribute (what `///`/`/** */` desugar to) | A | n/a | Same helper, `attr.path().is_ident("doc")` allow-list | `accepts_a_plain_doc_comment` |
| A Nim `{.pragma.}` on a `proc` | R (as an unrecognized token -- see "Unknown character/token" row) | R | Not specially handled at all; the parser expects `(` immediately after the proc name, so a pragma there is a `ParseError` | No dedicated test |
| `self`/`&self`/`&mut self` parameter | R | n/a | `FnArg::Receiver` arm | `rejects_a_self_parameter` |
| Non-identifier parameter pattern (e.g. a tuple pattern) | R | n/a (Nim's grammar here only ever produces identifiers) | `Pat::Ident` guard's `other` arm | `rejects_a_non_identifier_parameter_pattern` |
| Parameter type other than `i32`/`int32` | R | R | `type_is_i32` check; `expect_type_i32`'s `Ident(t)` arm | `nim_frontend::rejects_a_type_other_than_int32` (Rust side has no dedicated test, only exercised incidentally by the generic-function tests) |
| Non-`#[doc]` attribute on one parameter (`fn f(#[cfg(..)] x: i32)`) | R | n/a | `reject_unsupported_attrs(&pat_type.attrs, ..)` | `rejects_a_cfg_attribute_on_a_function_parameter` |
| Return type other than `i32`/`int32` | R | n/a (Nim's `expect_type_i32` is reused for the return type too, so the same row above covers it) | `ReturnType::Type(_, ty)` non-i32 arm | `rejects_a_return_type_other_than_i32` |
| No return type (`fn f(x: i32) { .. }`) | R | n/a (Nim's grammar always requires `: TYPE` before `=`) | `ReturnType::Default` arm | `rejects_a_function_with_no_return_type` |
| A tuple-destructuring `let` at proc/function scope is unrelated to this row; see the let-binding section | -- | -- | -- | -- |

## Let-binding level

| Construct | Rust | Nim | Implementing code | Test |
|---|---|---|---|---|
| `let NAME = EXPR;` / `let NAME: TYPE = EXPR` prefixing a block | A | A | `lower_block`'s `lets` collection; `parse_body_block`'s `is_ident("let")` arm | `lowers_let_bindings_and_multiple_wrapping_ops` (both) |
| Non-`#[doc]` attribute on a `let` statement (`#[cfg(..)] let y = x;`) | R | n/a (a Nim pragma here would fail to parse as an identifier list, surfacing as a `ParseError`, not a dedicated check) | `reject_unsupported_attrs(&local.attrs, ..)` | `rejects_a_cfg_attribute_on_a_let_statement` |
| Non-identifier `let` pattern (`let (a, b) = ..`) | R | R (tuple-destructuring names) | `Pat::Ident` guard's `other` arm; `names.len() != 1` check | `rejects_a_non_identifier_let_pattern`; `rejects_a_tuple_destructuring_let` |
| `let` with no initializer (`let x: i32;`) | R | n/a (Nim's grammar requires `=` right after the type) | `local.init` is `None` arm | `rejects_a_let_with_no_initializer` |
| `let-else` (`let Some(x) = y else { .. };`) | R | n/a | `init.diverge.is_some()` check | `rejects_let_else` |
| Source-level shadowing (`let x = 1; let x = 2; x`) | A (distinct `LocalId`s, restored on scope exit) | A | `Ctx.locals`/`Parser.locals` insert-then-restore-via-`shadowed` discipline | `validate::tests::shadowing_via_distinct_local_ids_validates` (Nim); no direct positive-value test on the Rust side (`rejects_calling_a_local_that_shadows_a_declared_functions_name` only tests the *call-position* shadowing interaction, not that a plain shadowed value round-trips correctly) |

## Statement/block-shape level

| Construct | Rust | Nim | Implementing code | Test |
|---|---|---|---|---|
| A statement after the block's tail form | R | R | `lower_block`'s `tail.is_some()` early check; `parse_body_block`'s post-tail `at_a_real_boundary` check | `rejects_a_semicolon_terminated_non_return_tail` covers the Rust-shaped analog directly reachable this way; `rejects_a_statement_following_the_tail_form_instead_of_silently_ignoring_it`, `rejects_unsupported_trailing_syntax_on_the_same_line_as_the_tail`, `rejects_unconsumed_content_at_a_deeper_indent_than_the_tail` (Nim, 3 distinct sub-shapes of this same hazard) |
| A block with no tail expression at all | R | n/a (an empty Nim body fails to parse a tail form directly, same rejection path as any other malformed body) | `unsupported_shape("block has no tail expression/return", ..)` | `rejects_a_block_with_no_tail_expression` |
| An item or macro statement inside a block | R | n/a | `lower_block`'s catch-all `other` match arm | `rejects_an_item_statement_inside_a_block` |
| Bare `return;` with no value | R | n/a (this subset has no bare-return equivalent in Nim's own grammar) | `ret.expr` is `None` arm | `rejects_a_bare_return_with_no_value` |
| `return EXPR;` | A | n/a (Nim's tail-expression form is the only return shape this subset parses) | `SynStmt::Return` match arm | `explicit_return_and_return_only_body_are_supported` |
| Trailing expression with no semicolon (implicit tail return) | A | A (every Nim proc body's tail is exactly this shape) | `lower_tail_stmt`'s `_ if semi.is_none()` arm | `explicit_return_and_return_only_body_are_supported` (Rust; the plain-tail form is also exercised by essentially every other passing test); every passing Nim test |
| Semicolon-terminated non-return tail (`{ x; }`, implicit `()`) | R | n/a | `lower_tail_stmt`'s final catch-all arm | `rejects_a_semicolon_terminated_non_return_tail` |

## `if`/`else` level

| Construct | Rust | Nim | Implementing code | Test |
|---|---|---|---|---|
| `if EXPR != 0 { .. } else { .. }` at a block's tail | A | A (`if EXPR != 0: .. else: ..`, block form only) | `lower_if`/`lower_condition`; `parse_if`/`parse_condition` | `lowers_the_existing_workload_functions_and_matches_real_semantics` (both) |
| `if EXPR == 0 { .. } else { .. }` (then/else swapped) | A | R (Nim's `parse_condition` only accepts `!=`, "purely to keep the two frontends' scope symmetric for this task" per that function's own doc comment) | `lower_condition`'s `BinOp::Eq` -> `swap = true` | No dedicated test (Rust; only `!=` forms are exercised directly, though the swap logic is structurally identical); intentionally out of scope on the Nim side, so absence of a test here is not a gap |
| `0 != EXPR` / `0 == EXPR` (literal-first) | A | A (`combine_zero_comparison` checks either side) | `lower_condition`'s `is_zero_lit` check on both operands; `combine_zero_comparison` | `accepts_the_literal_first_zero_comparison_form` (Nim); no dedicated test (Rust) |
| Condition using an operator other than `!=`/`==` | R | R (Nim: only `!=` is even attempted; anything else falls to the final `Err`) | `lower_condition`'s final `_` arm; `parse_condition`'s final `Err` | No dedicated test |
| Condition not comparing against a literal `0` | R | R | `lower_condition`'s final "must compare against a literal 0" branch; `combine_zero_comparison`'s `else` arm | No dedicated test |
| `if` with no `else` | R | R | `lower_if`'s `else_branch` is `None` check; `parse_if`'s `is_ident("else")` check | `rejects_an_if_with_no_else` (Nim); no dedicated test (Rust) |
| `else if` chain | A (recurses via `SynExpr::If(inner) => lower_if(inner, ctx)`) | n/a (this subset has no `elif`, per this module's own doc comment -- "no elif (not needed by this task's declared subset)") | `lower_if`'s `else_expr` match | `accepts_an_else_if_chain` |
| Non-block `else` shape (`else some_call()`) | R | n/a | `lower_if`'s final `other` arm | `rejects_a_non_block_else_branch` |
| Non-`#[doc]` attribute on an `if` used at a block's tail | R | n/a (a Nim pragma before `if` is not special-cased; it would fail as an unrecognized token at that position) | `reject_unsupported_attrs(&expr_if.attrs, ..)` | `rejects_a_cfg_attribute_on_an_if_tail` |
| `if` used as a value expression, not at a block's tail | R | n/a (Nim's grammar here only ever parses `if` as a body-block tail form; an `if` inside a general expression is not reachable through `parse_atom_expr` at all, so it surfaces as a plain unrecognized-token `ParseError`) | `SynExpr::If(_)` arm in `lower_expr` | `rejects_an_if_used_as_a_value_expression` |
| `NotEqZero` reachable anywhere other than an `if`'s own condition (e.g. `x != 0` used as a general `i32` value) | R | R (structurally: `parse_condition` is the only caller of `combine_zero_comparison`, so this shape cannot even be constructed from Nim source) | `lower_expr` no longer has a general `!=`/`==` arm at all (removed by a prior review fix); real Rust itself rejects `x != 0` as a type error | `rejects_a_bool_expression_used_as_an_i32_value` (Rust; the equivalent is structurally unreachable on the Nim side, not merely untested) |
| The reverse direction: an `if`'s own `cond` that is *not* a `NotEqZero` at all (a bare value, e.g. `Stmt::If{cond: Expr::Param(0), ..}`) | R (structurally unreachable from real Rust/Nim source -- both frontends only ever construct `NotEqZero` for a condition position) | R (same) | `validate::validate_stmt`'s `Stmt::If` arm now requires `matches!(cond, Expr::NotEqZero(..))`, not merely permits it (issue #27 A2's own bidirectional condition/value check, a review caught was previously one-directional) | `validate::tests::a_bare_value_used_directly_as_an_ifs_condition_is_rejected` (IR-level only, since no real source can construct this shape) |

## Expression level (shared arithmetic/literal/call shapes)

| Construct | Rust | Nim | Implementing code | Test |
|---|---|---|---|---|
| Integer literal fitting `i32`/`int32` | A | A | `parse_i32_literal`; `parse_atom_expr`'s `IntLit` arm | `lowers_let_bindings_and_multiple_wrapping_ops` and most other passing tests |
| Integer literal with a non-`i32` suffix (`5u64`) | R | R (`'i64`, etc.) | `parse_i32_literal`'s suffix check | `rejects_a_negative_literal_with_a_non_i32_suffix`, `rejects_a_positive_literal_with_a_non_i32_suffix` |
| Integer literal out of `i32`/`int32` range (including after negation) | R | R | `parse_i32_literal`'s range check; `parse_atom_expr`'s explicit range check | `rejects_a_positive_literal_out_of_i32_range`; `rejects_an_int32_literal_outside_int32_range` (Nim) |
| `i32::MIN`/equivalent as a literal (`-2147483648`) | A | A | `parse_i32_literal`'s widen-to-`i64`-then-negate-then-range-check discipline; `lex_number`'s own leading-`-` handling plus the same range check | `accepts_i32_min_as_a_negative_literal` (Rust), `accepts_int32_min_as_a_literal` (Nim) |
| Non-integer literal (string, float, bool, char, byte-string) | R | n/a (this tokenizer has no string/float/char lexing at all; such a literal never reaches a dedicated check, it simply never tokenizes as `IntLit` and falls through to an unrecognized-token error) | `Lit::Int` guard's `_` arm | `rejects_a_non_integer_literal` |
| Unary negation of a literal | A | A (folded directly into `lex_number`'s own leading-`-` handling, not a separate AST node at all) | `SynExpr::Unary` with `syn::UnOp::Neg` guard | `accepts_i32_min_as_a_negative_literal` |
| Unary negation of a non-literal expression (`-x`) | R | n/a (this subset's Nim grammar has no unary-minus operator on a general expression at all -- a bare `-` before a non-digit is tokenized as a generic `Operator`, so this is a parse-level rejection, not a dedicated semantic check) | `lower_expr`'s `Unary`/`Neg` arm's final `Err` when the operand isn't `SynExpr::Lit(Lit::Int(..))` | `rejects_unary_negation_of_a_non_literal` |
| `.wrapping_add/sub/mul(x)` / `a +% b`, `a -% b`, `a *% b` | A | A | `SynExpr::MethodCall` arm's `kind` match; `parse_expr`/`parse_term`'s operator dispatch | `lowers_let_bindings_and_multiple_wrapping_ops` (both); `multiplication_binds_tighter_than_addition`(`_reversed`) (Nim's own precedence-specific regression) |
| A method call other than `wrapping_add/sub/mul` | R | n/a (Nim has no method-call syntax in this subset at all) | `MethodCall`'s `method.as_str()` `_` arm | `rejects_a_method_call_other_than_wrapping_ops` |
| A `wrapping_*` call with other than exactly one argument | R | n/a | `m.args.len() != 1` check | `rejects_a_wrapping_call_with_the_wrong_argument_count` |
| Reference to a bound local or parameter | A | A | `SynExpr::Path` with `ctx.locals`/`ctx.params` lookup; `parse_atom_expr`'s `Ident` arm with `p.locals`/`p.params` lookup | Essentially every passing test |
| Reference to an undeclared identifier | R | R | `lower_expr`'s final "undeclared identifier" branch; `parse_atom_expr`'s equivalent | `rejects_a_reference_to_an_undeclared_identifier` (both) |
| Call to a function in the same lowering request | A | A | `SynExpr::Call` arm's `declared_functions.contains` check; `parse_atom_expr`'s equivalent | `lowers_the_existing_workload_functions_and_matches_real_semantics` (both) |
| Call to a function *not* in the same lowering request (even if declared elsewhere in the file) | R | R | Same two check sites, the negative branch | `rejects_a_call_to_a_function_not_in_the_lowering_request` (both) |
| Call to a name shadowed by a local/parameter | R | A -- **a known, named asymmetry, not a gap in this table**: Nim's `parse_atom_expr` checks `p.locals`/`p.params` for a call-syntax name resolution *after* already committing to the call form only when `p.declared_functions.contains(&name)`; a shadowing local of the *same name as a declared function* is not separately probed the way Rust's frontend does, so this specific hazard is Rust-only tested and Rust-only fixed today | `lower_expr`'s explicit `ctx.locals`/`ctx.params` pre-check before consulting `declared_functions` (Nim has no equivalent pre-check) | `rejects_calling_a_local_that_shadows_a_declared_functions_name` (Rust only) |
| Call to a non-identifier callee (`(f)(x)`, a closure call, ...) | R | n/a (Nim's grammar here only ever parses a call as `Ident LParen ...`, so this shape is not reachable through this parser's own call syntax at all) | `SynExpr::Call`'s `c.func.as_ref()` non-`Path` arm | No dedicated test (a direct, minimal repro of this exact shape from stable Rust surface syntax is awkward -- `(f)(x)` still parses as `SynExpr::Path`/paren-wrapped path, not a non-`Path` callee -- left open) |
| Call through a qualified path (`mod::f(x)`) | R | n/a | `p.path.get_ident()` is `None` arm | `rejects_a_call_through_a_qualified_path` |
| Parenthesized/grouped expression (`(x)`) | A | A | `SynExpr::Paren`/`SynExpr::Group` arms; `parse_atom_expr`'s `LParen` arm | Not directly asserted by a dedicated test, but present incidentally in several existing multi-operator test sources |
| Any other expression kind not named above (array/assign/async/await/block/break/cast/closure/continue/field/for/index/loop/macro/match/range/reference/repeat/struct-literal/try/try-block/tuple/unsafe-block/while) | R, each with its own named diagnostic string | R (falls through to `parse_atom_expr`'s final catch-all `UnsupportedConstruct`, since none of these have dedicated Nim syntax support to even reach a named branch) | `describe_expr_kind`'s full match (Rust names each kind individually); Nim's single catch-all | `rejects_a_match_expression_as_unsupported`, `rejects_a_while_loop_as_unsupported` (2 of the ~20 named Rust kinds explicitly tested; the rest share the same `unsupported(describe_expr_kind(other), ..)` call site and are not independently at risk of regressing per-kind) |

## Unknown-token / structural level (Nim only -- no Rust equivalent, `syn` owns tokenization there)

| Construct | Nim | Implementing code | Test |
|---|---|---|---|
| A character outside this subset's structural symbols/operator set/identifiers/digits, encountered *outside* a requested proc's body (array literals, string literals, `[`, `]`, ...) | I (opaque `Unknown` token, skipped by the top-level "not a proc" loop) | `Lexer::tokenize`'s final `else` arm; this module's own doc comment names this explicitly | `ignores_unrelated_trailing_driver_code` |
| The same, reached *while parsing* a requested proc's body | R | `parse_atom_expr`'s final catch-all `UnsupportedConstruct` | `rejects_an_unrecognized_character_inside_a_requested_procs_body` |
| Same-line command-call syntax (`funcName (x)`, space before `(`) | R (silently reinterpreted as the no-space call form, since this tokenizer never records inter-token whitespace -- documented as a deliberate restriction, not a bug: "This module only implements the no-space call form") | `parse_atom_expr`'s `Ident`/`LParen` lookahead, unconditional | No dedicated test (an intentional scope boundary, not a defect) |
| `elif` | R (falls through to `parse_body_block`'s "expected a let binding, if, or tail expression" `ParseError` once the enclosing `if`'s own then/else pair is already closed) | Not implemented at all -- documented exclusion in this module's own top-level doc comment | No dedicated test (an intentional scope boundary) |
| Same-line `if`/`else` (`if x: y else: z` on one line) | R (this subset's `parse_if` always calls `parse_body_block`, which requires `real_indent()` -- an indented line strictly deeper than the current block) | `parse_body_block`'s `real_indent()` check | No dedicated test (an intentional scope boundary, documented in this module's own doc comment) |

## What this table is not

- Not a claim that the ~35 "no dedicated test" rows above are bugs --
  several are structurally unreachable (Nim has no generics/attributes/
  async to even parse), several share a code path already exercised
  indirectly by a passing test, and several are genuinely open gaps
  (marked as such, not glossed over). Closing every one is a real,
  boundable follow-up task, not implied by this table's existence.
- Not a type system, a full name-resolution pass, or ownership/borrow
  checking -- this subset's only "type" is `i32`, and "type-consistency"
  above means exactly the boolean-vs-`i32` distinction `NotEqZero`
  enforces, nothing broader.
- Not a claim that `rust_frontend`/`nim_frontend` cover equivalent
  surface area -- several rows above name genuine, deliberate asymmetries
  between the two (the `==`-swap condition form, the call-shadowing
  check, `elif`) rather than silently presenting them as parallel.

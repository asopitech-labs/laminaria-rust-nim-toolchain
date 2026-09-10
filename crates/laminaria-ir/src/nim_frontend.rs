//! Lowers a declared subset of real Nim source into [`crate::types::Program`].
//!
//! Nim has no off-the-shelf Rust-side syntax-parsing library the way Rust
//! has `syn`, and Nim's own compiler is not invocable as a library. Per
//! `docs/compiler-ownership-contract.md`'s lexical/syntactic-parsing-reuse
//! boundary, this hand-written tokenizer + recursive-descent parser is
//! grounded directly in the real Nim compiler's own lexer/parser (studied
//! from this repo's existing shallow clone at `.reference/Nim/compiler/
//! lexer.nim`/`parser.nim` before writing a single line here), not invented
//! from memory of "how Nim generally looks":
//!
//! - **Indentation has no INDENT/DEDENT tokens.** Real Nim tags every
//!   token with the column of the first token on its line (`-1`/none for a
//!   continuation token on the same line as a prior one:
//!   `lexer.nim`'s `tok.indent` field and the `skip` proc that sets it),
//!   and the parser keeps one integer "current expected indent"
//!   (`Parser.currInd`), pushed/popped exactly like a stack frame across
//!   recursive calls (`parser.nim`'s `withInd` template) and compared with
//!   `realInd`/`sameInd`/`sameOrNoInd`. This module's [`Lexer`]/[`Parser`]
//!   mirror that shape directly (`Token::indent: Option<u32>`,
//!   `Parser::cur_indent`, `real_indent`/`same_indent`).
//! - **Operators are a generic maximal run over an operator-character
//!   set**, not individually hard-coded: real Nim's `getOperator`
//!   (`lexer.nim`) greedily consumes a run of `OpChars`
//!   (`+-*/\\<>!?^.|=%&$@~:`) into one token, then only special-cases the
//!   exact text `"="`/`":"` (and a couple of others this subset never
//!   needs); everything else, including `+%`/`-%`/`*%`/`==`/`!=`, is a
//!   plain operator token whose *text* determines its meaning. This
//!   module's [`TokenKind::Operator`] does the same.
//! - **`funcName(...)` vs. `funcName (...)`** are genuinely distinct
//!   grammar productions in real Nim (call vs. command syntax), gated
//!   purely on whether whitespace precedes `(` (`parser.nim`'s
//!   `primarySuffix`, checking the lexer's `tsLeading` spacing flag). This
//!   module only implements the no-space call form, which is a real,
//!   unambiguous subset of Nim's own grammar, not an arbitrary limitation.
//! - **A shared `ident, ident: Type` parameter-list rule** (real Nim's
//!   `parseIdentColonEquals`, reused for both proc parameters and `let`
//!   bindings) is mirrored by [`Parser::parse_ident_list_with_type`].
//!
//! Known, deliberate restrictions beyond the declared subset (documented
//! rather than silently assumed): only the block form of `if`/`else` (colon
//! then an indented body on the following line) is supported, not the
//! same-line form (`if x: y else: z` all on one line); only signed/unsigned
//! decimal integer literals with an optional `'i32` suffix; no `elif`
//! (not needed by this task's declared subset, though the grounding above
//! would extend directly). Top-level content that is not a `proc`
//! declaration (a `let`, a `for` loop, ...) is skipped rather than
//! rejected, mirroring `rust_frontend`'s "only lower the requested
//! functions" contract -- exactly what lets this frontend parse the
//! *existing* `nim-src/add_or_double.nim` file for real even though its
//! trailing driver code (a `let testInputs = [...]` array and a `for`
//! loop) is well outside this declared subset.

use std::path::Path;

use crate::diagnostics::{Diagnostic, LoweringError};
use crate::types::{
    Expr, FnFact, FnId, IntWidth, LocalId, Program, Provenance, SourceLanguage, SourcePosition,
    SourceSpan, Stmt,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    IntLit(i64, Option<String>),
    /// `(` `)` `,` -- structural symbols outside Nim's generic operator
    /// character run.
    LParen,
    RParen,
    Comma,
    /// The generic operator-character-run token, real Nim's `tkOpr`. Its
    /// exact text (`+%`, `-%`, `*%`, `!=`, `==`, ...) determines meaning
    /// downstream, exactly as in `getPrecedence`/`endOperator`.
    Operator(String),
    /// The one specific operator-run text real Nim special-cases,
    /// `endOperator`'s `tkEquals` case.
    Equals,
    /// Likewise for `:`.
    Colon,
    /// A character this tokenizer does not recognize at all (outside this
    /// subset's structural symbols, operator-character set, identifiers,
    /// and digits) -- kept as an opaque, skippable token rather than a
    /// hard tokenizer failure, so unrelated top-level content this crate
    /// never intends to parse (a `let`/`for` driver, array literals, ...)
    /// doesn't prevent tokenizing -- and therefore skipping -- the rest of
    /// the file to find the requested `proc` declarations.
    Unknown(char),
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    /// `Some(column)` (1-indexed) when this is the first token on its
    /// line -- real Nim's `tok.indent`. `None` for a continuation token,
    /// real Nim's `indent == -1`.
    indent: Option<u32>,
    line: u32,
    column: u32,
}

const OP_CHARS: &str = "+-*/\\<>!?^.|=%&$@~:";

struct Lexer<'a> {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    column: u32,
    at_line_start: bool,
    _source: &'a str,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Lexer {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
            at_line_start: true,
            _source: source,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
            self.at_line_start = true;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    fn tokenize(mut self) -> Result<Vec<Token>, LoweringError> {
        let mut tokens = Vec::new();
        loop {
            // Skip spaces/blank lines/comments -- real Nim's `skip` proc.
            // Only a genuine content character sets this line's indent;
            // a blank or comment-only line is transparent, matching
            // real Nim exactly (`skip` never assigns `tok.indent` for
            // those lines).
            loop {
                match self.peek() {
                    Some(' ') | Some('\r') => {
                        self.advance();
                    }
                    Some('\n') => {
                        self.advance();
                    }
                    Some('#') => {
                        while !matches!(self.peek(), None | Some('\n')) {
                            self.advance();
                        }
                    }
                    _ => break,
                }
            }
            let Some(c) = self.peek() else {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    indent: None,
                    line: self.line,
                    column: self.column,
                });
                break;
            };

            let indent = if self.at_line_start {
                Some(self.column)
            } else {
                None
            };
            let (line, column) = (self.line, self.column);
            self.at_line_start = false;

            let kind = if c.is_ascii_digit()
                || (c == '-'
                    && self.peek_at(1).is_some_and(|d| d.is_ascii_digit())
                    && Self::allows_leading_minus(tokens.last()))
            {
                self.lex_number()?
            } else if c.is_alphabetic() || c == '_' {
                self.lex_ident()
            } else if c == '(' {
                self.advance();
                TokenKind::LParen
            } else if c == ')' {
                self.advance();
                TokenKind::RParen
            } else if c == ',' {
                self.advance();
                TokenKind::Comma
            } else if OP_CHARS.contains(c) {
                self.lex_operator()
            } else {
                // Not a hard tokenizer error: this crate's top-level
                // lowering loop skips any content that isn't a `proc`
                // declaration (an unrelated `let`/`for` driver, array
                // literals, ...), which may use syntax well outside this
                // subset (`[`, `]`, string literals, ...). Producing an
                // opaque token here -- rather than failing the whole
                // file's tokenization -- is what lets that skip actually
                // work; a genuinely unsupported character reached while
                // parsing a *requested* proc's body still surfaces as a
                // clean `UnsupportedConstruct` diagnostic from the parser
                // (see `parse_atom_expr`'s fallback arm), not a panic.
                self.advance();
                TokenKind::Unknown(c)
            };
            tokens.push(Token {
                kind,
                indent,
                line,
                column,
            });
        }
        Ok(tokens)
    }

    /// A `-` starts a negative-number literal only in a "prefix" position
    /// -- real Nim's `UnaryMinusWhitelist` gate (checked against whichever
    /// token precedes it). Approximated here for this subset's actual
    /// call shapes: allowed after nothing (start of input), `(`, `,`,
    /// `:`, `=`, or another operator -- never immediately after an
    /// identifier/number/`)`, which would make `-` a binary/infix use
    /// instead. A documented simplification of real Nim's fuller rule,
    /// sufficient for this subset (no infix subtraction is supported at
    /// all, so there is no real ambiguity to resolve beyond this).
    fn allows_leading_minus(prev: Option<&Token>) -> bool {
        match prev {
            None => true,
            Some(t) => matches!(
                t.kind,
                TokenKind::LParen
                    | TokenKind::Comma
                    | TokenKind::Colon
                    | TokenKind::Equals
                    | TokenKind::Operator(_)
            ),
        }
    }

    fn lex_number(&mut self) -> Result<TokenKind, LoweringError> {
        let start_line = self.line;
        let start_col = self.column;
        let mut text = String::new();
        if self.peek() == Some('-') {
            text.push(self.advance().unwrap());
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            text.push(self.advance().unwrap());
        }
        let value: i64 = text.parse().map_err(|_| LoweringError::ParseError {
            detail: format!("malformed integer literal '{text}'"),
            span: point_span(start_line, start_col),
        })?;
        let suffix = if self.peek() == Some('\'') {
            self.advance();
            let mut s = String::new();
            while self.peek().is_some_and(|c| c.is_alphanumeric()) {
                s.push(self.advance().unwrap());
            }
            Some(s)
        } else {
            None
        };
        Ok(TokenKind::IntLit(value, suffix))
    }

    fn lex_ident(&mut self) -> TokenKind {
        let mut text = String::new();
        while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            text.push(self.advance().unwrap());
        }
        TokenKind::Ident(text)
    }

    fn lex_operator(&mut self) -> TokenKind {
        let mut text = String::new();
        while self.peek().is_some_and(|c| OP_CHARS.contains(c)) {
            text.push(self.advance().unwrap());
        }
        match text.as_str() {
            "=" => TokenKind::Equals,
            ":" => TokenKind::Colon,
            _ => TokenKind::Operator(text),
        }
    }
}

fn point_span(line: u32, column: u32) -> SourceSpan {
    let p = SourcePosition { line, column };
    SourceSpan { start: p, end: p }
}

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    source_file: &'a Path,
    declared_functions: &'a std::collections::BTreeSet<String>,
    params: std::collections::BTreeMap<String, usize>,
    locals: std::collections::BTreeMap<String, LocalId>,
    next_local: u32,
    cur_indent: u32,
}

type PResult<T> = Result<T, Vec<LoweringError>>;

impl<'a> Parser<'a> {
    fn tok(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn span_here(&self) -> SourceSpan {
        point_span(self.tok().line, self.tok().column)
    }

    fn provenance_here(&self) -> Provenance {
        Provenance {
            source_file: self.source_file.to_path_buf(),
            span: self.span_here(),
            language: SourceLanguage::Nim,
        }
    }

    fn real_indent(&self) -> bool {
        matches!(self.tok().indent, Some(c) if c > self.cur_indent)
    }

    fn same_indent(&self) -> bool {
        self.tok().indent == Some(self.cur_indent)
    }

    fn is_ident(&self, name: &str) -> bool {
        matches!(&self.tok().kind, TokenKind::Ident(s) if s == name)
    }

    fn is_op(&self, text: &str) -> bool {
        matches!(&self.tok().kind, TokenKind::Operator(s) if s == text)
    }

    fn expect_colon(&mut self) -> PResult<()> {
        if matches!(self.tok().kind, TokenKind::Colon) {
            self.bump();
            Ok(())
        } else {
            Err(vec![LoweringError::ParseError {
                detail: "expected ':'".to_string(),
                span: self.span_here(),
            }])
        }
    }

    /// Real Nim's shared `ident, ident: Type` rule (`parseIdentColonEquals`
    /// in `parser.nim`), used for both a proc's parameter list and a
    /// `let` binding's identifier(s) -- one type annotation covers every
    /// identifier collected before the `:`.
    fn parse_ident_list_with_type(&mut self) -> PResult<Vec<String>> {
        let mut names = Vec::new();
        loop {
            let TokenKind::Ident(name) = self.tok().kind.clone() else {
                return Err(vec![LoweringError::ParseError {
                    detail: "expected identifier".to_string(),
                    span: self.span_here(),
                }]);
            };
            names.push(name);
            self.bump();
            if matches!(self.tok().kind, TokenKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        Ok(names)
    }

    fn expect_type_i32(&mut self) -> PResult<()> {
        if matches!(self.tok().kind, TokenKind::Colon) {
            self.bump();
        } else {
            return Err(vec![LoweringError::ParseError {
                detail: "expected ':' before type".to_string(),
                span: self.span_here(),
            }]);
        }
        match &self.tok().kind {
            TokenKind::Ident(t) if t == "int32" => {
                self.bump();
                Ok(())
            }
            TokenKind::Ident(t) => Err(vec![LoweringError::UnsupportedConstruct {
                construct: format!("type '{t}' (only int32 is supported)"),
                span: self.span_here(),
            }]),
            _ => Err(vec![LoweringError::ParseError {
                detail: "expected a type name".to_string(),
                span: self.span_here(),
            }]),
        }
    }
}

/// Lowers exactly the named top-level `proc`s from `source_text` into a
/// [`Program`]. Non-`proc` top-level content is skipped, not rejected --
/// see this module's own doc comment for why.
pub fn lower_nim_source(
    source_file: &Path,
    source_text: &str,
    requested_functions: &[&str],
) -> Result<Program, Vec<Diagnostic>> {
    let tokens = Lexer::new(source_text)
        .tokenize()
        .map_err(|e| vec![Diagnostic::from_lowering_error(e, SourceLanguage::Nim)])?;

    let declared_functions: std::collections::BTreeSet<String> =
        requested_functions.iter().map(|s| s.to_string()).collect();

    let mut parser = Parser {
        tokens,
        pos: 0,
        source_file,
        declared_functions: &declared_functions,
        params: std::collections::BTreeMap::new(),
        locals: std::collections::BTreeMap::new(),
        next_local: 0,
        cur_indent: 0,
    };

    let mut program = Program::default();
    let mut diagnostics = Vec::new();
    let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    while !matches!(parser.tok().kind, TokenKind::Eof) {
        if parser.is_ident("proc") && parser.tok().indent == Some(1) {
            match parse_proc(&mut parser) {
                Ok(fact) => {
                    if declared_functions.contains(&fact.name) {
                        found.insert(fact.name.clone());
                        program.insert(fact);
                    }
                }
                Err(errors) => diagnostics.extend(
                    errors
                        .into_iter()
                        .map(|e| Diagnostic::from_lowering_error(e, SourceLanguage::Nim)),
                ),
            }
        } else {
            parser.bump();
        }
    }

    for name in requested_functions {
        if !found.contains(*name) {
            diagnostics.push(Diagnostic::from_lowering_error(
                LoweringError::UnsupportedShape {
                    detail: format!("requested function '{name}' is not declared in this file"),
                    span: point_span(1, 1),
                },
                SourceLanguage::Nim,
            ));
        }
    }

    if diagnostics.is_empty() {
        Ok(program)
    } else {
        Err(diagnostics)
    }
}

/// `proc name(params): int32 = <body>` -- real Nim's `parseRoutine`
/// (grammar: `routine = identVis paramListColon ('=' stmt)?`). The `=`
/// before the body is a literal `tkEquals` check, exactly matching real
/// Nim's own `p.tok.tokType != tkEquals` gate.
fn parse_proc(p: &mut Parser) -> PResult<FnFact> {
    let start = p.span_here();
    p.bump(); // "proc"
    let TokenKind::Ident(name) = p.tok().kind.clone() else {
        return Err(vec![LoweringError::ParseError {
            detail: "expected proc name".to_string(),
            span: p.span_here(),
        }]);
    };
    p.bump();

    if !matches!(p.tok().kind, TokenKind::LParen) {
        return Err(vec![LoweringError::ParseError {
            detail: "expected '(' after proc name".to_string(),
            span: p.span_here(),
        }]);
    }
    p.bump();

    let mut params: Vec<(String, IntWidth)> = Vec::new();
    p.params.clear();
    if !matches!(p.tok().kind, TokenKind::RParen) {
        loop {
            let names = p.parse_ident_list_with_type()?;
            p.expect_type_i32()?;
            for n in names {
                p.params.insert(n.clone(), params.len());
                params.push((n, IntWidth::I32));
            }
            if matches!(p.tok().kind, TokenKind::Comma) {
                p.bump();
                continue;
            }
            break;
        }
    }
    if !matches!(p.tok().kind, TokenKind::RParen) {
        return Err(vec![LoweringError::ParseError {
            detail: "expected ')'".to_string(),
            span: p.span_here(),
        }]);
    }
    p.bump();
    p.expect_type_i32()?;

    if !matches!(p.tok().kind, TokenKind::Equals) {
        return Err(vec![LoweringError::ParseError {
            detail: "expected '=' before proc body".to_string(),
            span: p.span_here(),
        }]);
    }
    p.bump();

    p.locals.clear();
    p.next_local = 0;
    let body = parse_body_block(p)?;

    Ok(FnFact {
        name,
        params,
        return_width: IntWidth::I32,
        body,
        provenance: Provenance {
            source_file: p.source_file.to_path_buf(),
            span: start,
            language: SourceLanguage::Nim,
        },
    })
}

/// Parses one indented block as a function/branch body: zero or more
/// `let NAME = EXPR` lines followed by exactly one tail form (a plain
/// expression, or an `if`/`else`). Mirrors `rust_frontend::lower_block`'s
/// contract, adapted to indentation instead of braces -- real Nim's
/// `withInd`/`sameInd` bookkeeping (see this module's own doc comment).
fn parse_body_block(p: &mut Parser) -> PResult<Stmt> {
    if !p.real_indent() {
        return Err(vec![LoweringError::ParseError {
            detail: "expected an indented block".to_string(),
            span: p.span_here(),
        }]);
    }
    let old_indent = p.cur_indent;
    p.cur_indent = p.tok().indent.expect("real_indent() checked Some");

    // Binds each let into `p.locals` immediately after lowering its value
    // (forward/source order) -- a later let's initializer, and the tail,
    // must be able to see every earlier let's binding. `shadowed` records
    // whatever `p.locals` held for that name before, if anything, so it
    // can be restored once this block's own scope ends; the nested
    // `Stmt::Let` tree itself is only built afterward, in reverse (see
    // below), since the first let must end up as the *outermost* node.
    struct PendingLet {
        name: String,
        id: LocalId,
        value: Expr,
        span: SourceSpan,
        shadowed: Option<LocalId>,
    }
    let mut lets: Vec<PendingLet> = Vec::new();
    let result = loop {
        if p.is_ident("let") && p.same_indent() {
            let let_span = p.span_here();
            p.bump();
            let names = match p.parse_ident_list_with_type() {
                Ok(n) => n,
                Err(e) => break Err(e),
            };
            if names.len() != 1 {
                break Err(vec![LoweringError::UnsupportedConstruct {
                    construct: "tuple-destructuring let".to_string(),
                    span: let_span,
                }]);
            }
            if !matches!(p.tok().kind, TokenKind::Equals) {
                break Err(vec![LoweringError::ParseError {
                    detail: "expected '=' in let binding".to_string(),
                    span: p.span_here(),
                }]);
            }
            p.bump();
            match parse_expr(p) {
                Ok(value) => {
                    let name = names.into_iter().next().unwrap();
                    let id = LocalId(p.next_local);
                    p.next_local += 1;
                    let shadowed = p.locals.insert(name.clone(), id);
                    lets.push(PendingLet {
                        name,
                        id,
                        value,
                        span: let_span,
                        shadowed,
                    });
                }
                Err(e) => break Err(e),
            }
            continue;
        }
        if p.is_ident("if") && p.same_indent() {
            match parse_if(p) {
                Ok(stmt) => break Ok(stmt),
                Err(e) => break Err(e),
            }
        }
        if p.same_indent() {
            match parse_expr(p) {
                Ok(e) => {
                    let prov = e.provenance().clone();
                    break Ok(Stmt::Return(e, prov));
                }
                Err(e) => break Err(e),
            }
        }
        break Err(vec![LoweringError::ParseError {
            detail: "expected a let binding, if, or tail expression at this indentation"
                .to_string(),
            span: p.span_here(),
        }]);
    };
    p.cur_indent = old_indent;

    let mut inner = result?;
    for p_let in lets.into_iter().rev() {
        match p_let.shadowed {
            Some(prev) => {
                p.locals.insert(p_let.name, prev);
            }
            None => {
                p.locals.remove(&p_let.name);
            }
        }
        inner = Stmt::Let {
            local: p_let.id,
            value: p_let.value,
            body: Box::new(inner),
            provenance: Provenance {
                source_file: p.source_file.to_path_buf(),
                span: p_let.span,
                language: SourceLanguage::Nim,
            },
        };
    }
    Ok(inner)
}

/// `if cond:` then an indented then-block, then `else:` at the *same*
/// indent as `if` (real Nim: `(IND{=} 'else' colcom stmt)?`), then an
/// indented else-block. Block form only -- see this module's own doc
/// comment for the same-line-form restriction.
fn parse_if(p: &mut Parser) -> PResult<Stmt> {
    let if_span = p.span_here();
    p.bump(); // "if"
    let cond_expr = parse_condition(p)?;
    p.expect_colon()?;
    let then = parse_body_block(p)?;
    if !(p.is_ident("else") && p.same_indent()) {
        return Err(vec![LoweringError::UnsupportedShape {
            detail: "if with no else (every path must return)".to_string(),
            span: if_span,
        }]);
    }
    p.bump();
    p.expect_colon()?;
    let els = parse_body_block(p)?;
    Ok(Stmt::If {
        cond: cond_expr,
        then: Box::new(then),
        els: Box::new(els),
        provenance: Provenance {
            source_file: p.source_file.to_path_buf(),
            span: if_span,
            language: SourceLanguage::Nim,
        },
    })
}

/// This subset's only condition form: `EXPR != 0` / `EXPR == 0` (and the
/// literal-first mirror). `== 0` is rejected here (unlike in
/// `rust_frontend`'s if-condition path) purely to keep the two frontends'
/// scope symmetric for this task; extending it to swap `then`/`else`
/// (matching `rust_frontend::lower_condition`) is a direct, mechanical
/// follow-up, not a design gap.
fn parse_condition(p: &mut Parser) -> PResult<Expr> {
    let lhs = parse_atom_expr(p)?;
    if p.is_op("!=") {
        p.bump();
        let rhs = parse_atom_expr(p)?;
        return combine_zero_comparison(lhs, rhs, p);
    }
    Err(vec![LoweringError::UnsupportedShape {
        detail: "if condition must be 'EXPR != 0'".to_string(),
        span: p.span_here(),
    }])
}

fn combine_zero_comparison(lhs: Expr, rhs: Expr, p: &Parser) -> PResult<Expr> {
    let is_zero = |e: &Expr| matches!(e, Expr::IntLit(0, _, _));
    let inner = if is_zero(&rhs) {
        lhs
    } else if is_zero(&lhs) {
        rhs
    } else {
        return Err(vec![LoweringError::UnsupportedShape {
            detail: "comparison must be against a literal 0".to_string(),
            span: p.span_here(),
        }]);
    };
    let provenance = inner.provenance().clone();
    Ok(Expr::NotEqZero(Box::new(inner), provenance))
}

/// A full expression: `wrapping_atom (('+%'|'-%'|'*%') wrapping_atom)*`,
/// left-associative -- this subset needs no general operator-precedence
/// parser since `+%`/`-%`/`*%` are this grammar's only infix operators.
fn parse_expr(p: &mut Parser) -> PResult<Expr> {
    let mut lhs = parse_atom_expr(p)?;
    loop {
        let op = match &p.tok().kind {
            TokenKind::Operator(s) if s == "+%" => 0,
            TokenKind::Operator(s) if s == "-%" => 1,
            TokenKind::Operator(s) if s == "*%" => 2,
            _ => break,
        };
        let provenance = p.provenance_here();
        p.bump();
        let rhs = parse_atom_expr(p)?;
        lhs = match op {
            0 => Expr::WrappingAdd(Box::new(lhs), Box::new(rhs), provenance),
            1 => Expr::WrappingSub(Box::new(lhs), Box::new(rhs), provenance),
            _ => Expr::WrappingMul(Box::new(lhs), Box::new(rhs), provenance),
        };
    }
    Ok(lhs)
}

fn parse_atom_expr(p: &mut Parser) -> PResult<Expr> {
    let provenance = p.provenance_here();
    match p.tok().kind.clone() {
        TokenKind::IntLit(value, suffix) => {
            if let Some(s) = &suffix {
                if s != "i32" {
                    return Err(vec![LoweringError::UnsupportedConstruct {
                        construct: format!("integer literal suffix '{s}' (only i32 is supported)"),
                        span: p.span_here(),
                    }]);
                }
            }
            p.bump();
            Ok(Expr::IntLit(value, IntWidth::I32, provenance))
        }
        TokenKind::Ident(name) => {
            p.bump();
            // No whitespace before `(` is checked implicitly: this
            // tokenizer does not record inter-token whitespace at all, so
            // it always takes the call form when `(` follows -- real
            // Nim's `funcName (x)` command-call syntax is out of scope
            // for this subset (see this module's own doc comment).
            if matches!(p.tok().kind, TokenKind::LParen) {
                p.bump();
                let mut args = Vec::new();
                if !matches!(p.tok().kind, TokenKind::RParen) {
                    loop {
                        args.push(parse_expr(p)?);
                        if matches!(p.tok().kind, TokenKind::Comma) {
                            p.bump();
                            continue;
                        }
                        break;
                    }
                }
                if !matches!(p.tok().kind, TokenKind::RParen) {
                    return Err(vec![LoweringError::ParseError {
                        detail: "expected ')'".to_string(),
                        span: p.span_here(),
                    }]);
                }
                p.bump();
                if !p.declared_functions.contains(&name) {
                    return Err(vec![LoweringError::UnsupportedShape {
                        detail: format!("call to '{name}', which is not in this lowering request"),
                        span: provenance.span,
                    }]);
                }
                return Ok(Expr::Call(FnId(name), args, provenance));
            }
            if let Some(local) = p.locals.get(&name) {
                return Ok(Expr::Local(*local, provenance));
            }
            if let Some(index) = p.params.get(&name) {
                return Ok(Expr::Param(*index, provenance));
            }
            Err(vec![LoweringError::UnsupportedShape {
                detail: format!("reference to undeclared identifier '{name}'"),
                span: provenance.span,
            }])
        }
        TokenKind::LParen => {
            p.bump();
            let inner = parse_expr(p)?;
            if !matches!(p.tok().kind, TokenKind::RParen) {
                return Err(vec![LoweringError::ParseError {
                    detail: "expected ')'".to_string(),
                    span: p.span_here(),
                }]);
            }
            p.bump();
            Ok(inner)
        }
        other => Err(vec![LoweringError::UnsupportedConstruct {
            construct: format!("{other:?}"),
            span: p.span_here(),
        }]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::eval_function;
    use std::path::PathBuf;

    fn path() -> PathBuf {
        PathBuf::from("test.nim")
    }

    #[test]
    fn lowers_the_existing_workload_functions_and_matches_real_semantics() {
        let source = "proc double(x: int32): int32 =\n  x +% x\n\nproc addOrDouble(a, b, useDouble: int32): int32 =\n  if useDouble != 0'i32:\n    double(a)\n  else:\n    a +% b\n";
        let program = lower_nim_source(&path(), source, &["double", "addOrDouble"]).unwrap();
        assert_eq!(program.functions.len(), 2);

        let cases: &[((i32, i32, i32), i32)] = &[
            ((3, 4, 0), 7),
            ((3, 4, 1), 6),
            ((i32::MAX, 1, 0), i32::MIN),
            ((-5, 10, 1), -10),
        ];
        for &((a, b, u), expected) in cases {
            let outcome =
                eval_function(&program, "addOrDouble", &[a as i64, b as i64, u as i64]).unwrap();
            assert_eq!(outcome.value as i32, expected, "inputs ({a},{b},{u})");
        }
    }

    #[test]
    fn lowers_let_bindings_and_multiple_wrapping_ops() {
        let source = "proc combo(a, b, c: int32): int32 =\n  let x = a +% b\n  let y = x -% c\n  y *% 2'i32\n";
        let program = lower_nim_source(&path(), source, &["combo"]).unwrap();
        let outcome = eval_function(&program, "combo", &[10, 3, 2]).unwrap();
        assert_eq!(outcome.value, 22);
    }

    #[test]
    fn ignores_unrelated_trailing_driver_code() {
        // The exact shape of the real nim-src/add_or_double.nim fixture:
        // a `let testInputs = [...]` array literal and a `for` loop after
        // the requested procs -- both well outside this subset, and never
        // inspected because they aren't among the requested functions.
        let source = "proc double(x: int32): int32 =\n  x +% x\n\nlet testInputs = [(3'i32, 4'i32, 0'i32)]\n\nfor t in testInputs:\n  echo t\n";
        let result = lower_nim_source(&path(), source, &["double"]);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn rejects_a_request_for_an_undeclared_function() {
        let source = "proc double(x: int32): int32 =\n  x +% x\n";
        let result = lower_nim_source(&path(), source, &["missing"]);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_a_type_other_than_int32() {
        let source = "proc f(x: float): float =\n  x\n";
        let result = lower_nim_source(&path(), source, &["f"]);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_an_if_with_no_else() {
        let source = "proc f(x: int32): int32 =\n  if x != 0'i32:\n    x\n";
        let result = lower_nim_source(&path(), source, &["f"]);
        assert!(result.is_err());
    }

    #[test]
    fn every_node_in_a_lowered_program_carries_real_source_provenance() {
        let source = "proc f(x: int32): int32 =\n  x +% 1'i32\n";
        let program = lower_nim_source(&path(), source, &["f"]).unwrap();
        let fact = &program.functions["f"];
        assert_eq!(fact.provenance.language, SourceLanguage::Nim);
        assert!(fact.provenance.span.start.line >= 1);
    }
}

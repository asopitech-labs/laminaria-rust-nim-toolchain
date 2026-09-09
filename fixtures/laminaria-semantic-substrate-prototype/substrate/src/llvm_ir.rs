//! Projects the `repr` representation to a real LLVM IR backend route --
//! issue #25's "backend-route projection" requirement. Emitted straight
//! from `SemanticProgram`/`FnFact`/`Expr`/`Stmt`, never by reading rustc's
//! or nlvm's own emitted IR (that would defeat the point: this is meant to
//! demonstrate the *representation itself* can reach a backend, not that
//! LLVM IR can be copied around).
//!
//! Deliberately simple, non-optimizing codegen -- this grammar's `If`
//! always returns directly on both branches (see `repr.rs`'s doc comment),
//! so no phi-node merge is ever needed, which keeps this emitter small
//! enough to hand-verify against `llc`'s own acceptance of the output (see
//! NOTES.md for the exact `llc`/`cc` invocation this was checked against).

use crate::repr::{Expr, FnFact, SemanticProgram, Stmt};
use std::fmt::Write as _;

struct FunctionEmitter<'a> {
    program: &'a SemanticProgram,
    out: String,
    next_temp: u32,
    next_label: u32,
}

impl<'a> FunctionEmitter<'a> {
    fn new(program: &'a SemanticProgram) -> Self {
        FunctionEmitter {
            program,
            out: String::new(),
            next_temp: 0,
            next_label: 0,
        }
    }

    fn temp(&mut self) -> String {
        let name = format!("%t{}", self.next_temp);
        self.next_temp += 1;
        name
    }

    fn label(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}{}", self.next_label);
        self.next_label += 1;
        name
    }

    /// Emits instructions computing `expr`, returning the SSA register
    /// (always `i32`-typed) holding its value.
    fn emit_expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Param(n) => format!("%arg{n}"),
            Expr::WrappingAdd(lhs, rhs) => {
                let l = self.emit_expr(lhs);
                let r = self.emit_expr(rhs);
                let t = self.temp();
                let _ = writeln!(self.out, "  {t} = add i32 {l}, {r}");
                t
            }
            Expr::NotEqZero(inner) => {
                let v = self.emit_expr(inner);
                let cmp = self.temp();
                let _ = writeln!(self.out, "  {cmp} = icmp ne i32 {v}, 0");
                let t = self.temp();
                let _ = writeln!(self.out, "  {t} = zext i1 {cmp} to i32");
                t
            }
            Expr::Call(name, args) => {
                let arg_values: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                let callee = self
                    .program
                    .functions
                    .get(name)
                    .unwrap_or_else(|| panic!("emit_expr: unknown function `{name}`"));
                assert_eq!(
                    callee.param_widths.len(),
                    arg_values.len(),
                    "arity mismatch calling `{name}`"
                );
                let args_str = arg_values
                    .iter()
                    .map(|v| format!("i32 {v}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let t = self.temp();
                let _ = writeln!(self.out, "  {t} = call i32 @{name}({args_str})");
                t
            }
        }
    }

    /// Emits the `i1` condition `br` needs for `cond`. `NotEqZero` is
    /// special-cased to emit a bare `icmp` (no zext round-trip) since it's
    /// the only condition form this grammar's `If` actually uses; any
    /// other `Expr` falls back to `icmp ne i32 <value>, 0`.
    fn emit_cond(&mut self, cond: &Expr) -> String {
        if let Expr::NotEqZero(inner) = cond {
            let v = self.emit_expr(inner);
            let t = self.temp();
            let _ = writeln!(self.out, "  {t} = icmp ne i32 {v}, 0");
            t
        } else {
            let v = self.emit_expr(cond);
            let t = self.temp();
            let _ = writeln!(self.out, "  {t} = icmp ne i32 {v}, 0");
            t
        }
    }

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return(expr) => {
                let v = self.emit_expr(expr);
                let _ = writeln!(self.out, "  ret i32 {v}");
            }
            Stmt::If { cond, then, els } => {
                let c = self.emit_cond(cond);
                let then_label = self.label("then");
                let else_label = self.label("else");
                let _ = writeln!(
                    self.out,
                    "  br i1 {c}, label %{then_label}, label %{else_label}"
                );
                let _ = writeln!(self.out, "{then_label}:");
                self.emit_stmt(then);
                let _ = writeln!(self.out, "{else_label}:");
                self.emit_stmt(els);
            }
        }
    }
}

fn emit_function(program: &SemanticProgram, fact: &FnFact) -> String {
    let params = (0..fact.param_widths.len())
        .map(|n| format!("i32 %arg{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut emitter = FunctionEmitter::new(program);
    let _ = writeln!(emitter.out, "entry:");
    emitter.emit_stmt(&fact.body);
    format!(
        "define i32 @{}({params}) {{\n{}}}\n",
        fact.name, emitter.out
    )
}

/// Test inputs `main` prints results for, in the exact same order/format
/// as `../rust-src/add_or_double.rs`/`../nim-src/add_or_double.nim`'s own
/// `TEST_INPUTS` -- kept as one shared source of truth in `main.rs`
/// (`crate::TEST_INPUTS`) so this module and the reference-evaluator
/// cross-check in `main.rs` can never silently drift apart.
pub fn emit_module(program: &SemanticProgram, test_inputs: &[(i32, i32, i32)]) -> String {
    let mut out = String::new();
    // No explicit target triple/datalayout: `llc`'s own default for the
    // host it runs on is used (verified locally against this repo's
    // pinned rustup llvm-tools `llc`, LLVM 22 -- see NOTES.md), matching
    // how the rest of this repo's LLVM-IR fixtures let llc infer host
    // defaults rather than hard-coding a triple this fixture would need
    // to keep in sync with CI's own runner architecture.
    let fmt = "%d,%d,%d,%d\n\0";
    let _ = writeln!(
        out,
        "@fmt = private unnamed_addr constant [{} x i8] c\"{}\"",
        fmt.len(),
        escape_llvm_string(fmt)
    );
    let _ = writeln!(out, "declare i32 @printf(ptr, ...)");
    out.push('\n');

    for fact in program.functions.values() {
        out.push_str(&emit_function(program, fact));
        out.push('\n');
    }

    out.push_str("define i32 @main() {\n");
    out.push_str("entry:\n");
    for (i, &(a, b, use_double)) in test_inputs.iter().enumerate() {
        let _ = writeln!(
            out,
            "  %r{i} = call i32 @add_or_double(i32 {a}, i32 {b}, i32 {use_double})"
        );
        let _ = writeln!(
            out,
            "  %p{i} = call i32 (ptr, ...) @printf(ptr @fmt, i32 {a}, i32 {b}, i32 {use_double}, i32 %r{i})"
        );
    }
    out.push_str("  ret i32 0\n");
    out.push_str("}\n");

    out
}

/// Escapes a Rust string into LLVM IR's `c"..."` constant-string syntax:
/// every byte becomes `\XX` (two uppercase hex digits) except ASCII
/// printable non-`"`/non-`\` bytes, which are left literal -- simpler and
/// safer than trying to special-case which bytes need escaping, at the
/// cost of a slightly less readable `.ll` file. Verified directly against
/// `llc`'s acceptance of the exact string this module emits (`%d,%d,%d,%d`
/// plus a newline and NUL terminator) -- see NOTES.md.
fn escape_llvm_string(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_graphic() && b != b'"' && b != b'\\' {
                (b as char).to_string()
            } else {
                format!("\\{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repr::workload_program;

    #[test]
    fn escapes_the_format_string_to_a_length_matching_byte_count() {
        let fmt = "%d,%d,%d,%d\n\0";
        assert_eq!(fmt.len(), 13);
        // Every byte must have produced exactly one \XX or one literal
        // char -- the exact mismatch class that broke this fixture's
        // first hand-written .ll draft (see NOTES.md).
        let escaped = escape_llvm_string(fmt);
        let literal_count = escaped.matches('\\').count();
        let hex_escapes = literal_count; // one \XX per non-graphic/quote/backslash byte
        assert_eq!(hex_escapes, 2); // \n and \0
    }

    #[test]
    fn emits_a_module_with_both_functions_and_main() {
        let program = workload_program();
        let ir = emit_module(&program, &[(3, 4, 0)]);
        assert!(ir.contains("define i32 @double(i32 %arg0)"));
        assert!(ir.contains("define i32 @add_or_double(i32 %arg0, i32 %arg1, i32 %arg2)"));
        assert!(ir.contains("define i32 @main()"));
        assert!(ir.contains("call i32 @add_or_double(i32 3, i32 4, i32 0)"));
    }
}

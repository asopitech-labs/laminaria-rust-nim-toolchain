//! Structured diagnostics for every way a source file can fail to lower
//! into [`crate::types::Program`]. Every frontend function in this crate
//! returns `Result<Program, Vec<Diagnostic>>` -- never a panic, never a
//! partial/best-effort `Program`, and never a fallback to invoking
//! `rustc`/`nim` to get an answer some other way. This is a direct
//! requirement from `docs/compiler-ownership-contract.md`'s "Separate
//! roles" table: an unsupported construct or a malformed input is a
//! diagnostic, not a silently-degraded success or a delegated compile.

use crate::types::{SourceLanguage, SourceSpan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
}

/// The specific reason a source file was rejected. A closed enum (not a
/// bare string) so calling code -- including this crate's own tests -- can
/// assert on *which* rejection happened, not just that one did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweringError {
    /// The source text itself could not be tokenized/parsed at the syntax
    /// level (a genuine syntax error, or this frontend's own tokenizer
    /// choking on the input) -- distinct from `UnsupportedConstruct`, which
    /// means "this parsed fine as a syntax tree, but names a construct this
    /// subset does not support."
    ParseError { detail: String, span: SourceSpan },
    /// A real, recognized language construct that this task's declared
    /// subset does not cover (loops, `match`/`case`, structs/objects,
    /// generics, macros, closures, mutable/`&mut` state, command-call
    /// syntax, an integer width other than the one this task supports,
    /// ...). Named explicitly so a reader of a rejected-fixture test can
    /// see exactly which construct triggered it.
    UnsupportedConstruct { construct: String, span: SourceSpan },
    /// A construct this subset supports in general, but used in a shape
    /// this lowering does not (yet) handle -- e.g. a condition that isn't
    /// exactly `expr != 0`/`expr == 0`, or a call to a function this
    /// program never declares.
    UnsupportedShape { detail: String, span: SourceSpan },
    /// A frontend's own lowering produced a `Program` that fails
    /// `validate::validate_program` -- a live postcondition (issue #27
    /// A2: "検証はlowering後...で行う"), never expected to actually fire
    /// for a correctly-implemented frontend, but a genuine gate rather
    /// than a comment claiming the output is well-formed. Never has a
    /// meaningful per-node span (the violation is structural, not tied to
    /// one token), so `span` is a placeholder pointing at the start of
    /// the file.
    PostconditionViolated { detail: String, span: SourceSpan },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: SourceSpan,
    pub language: SourceLanguage,
}

impl Diagnostic {
    pub fn from_lowering_error(error: LoweringError, language: SourceLanguage) -> Self {
        let (message, span) = match error {
            LoweringError::ParseError { detail, span } => (format!("parse error: {detail}"), span),
            LoweringError::UnsupportedConstruct { construct, span } => (
                format!("unsupported construct: {construct} is not in the declared subset"),
                span,
            ),
            LoweringError::UnsupportedShape { detail, span } => {
                (format!("unsupported shape: {detail}"), span)
            }
            LoweringError::PostconditionViolated { detail, span } => (
                format!("internal error: lowering produced an invalid Program: {detail}"),
                span,
            ),
        };
        Diagnostic {
            severity: Severity::Error,
            message,
            span,
            language,
        }
    }
}

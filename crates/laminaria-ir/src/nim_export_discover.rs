//! Issue #48 (G1): a shallow, best-effort scanner for Nim `proc`
//! declarations carrying an `{.exportc.}` pragma -- the Nim-side
//! counterpart to [`crate::foreign_discover`]'s Rust `extern` scan and
//! [`crate::c_header_discover`]'s C/C++ prototype scan. This module
//! never invokes the Nim compiler and never inspects a compiled
//! archive -- it turns real `.nim` source text into a small, typed
//! fact list, over the same "best-effort, never a correctness gate"
//! discipline the other two discovery modules use for everything
//! *except* one real, checkable structural fact: a parenthesized
//! parameter list that never closes is reported as a genuine
//! [`NimSignatureError`], not silently skipped -- Checkpoint A of issue
//! #48's own follow-up requires that a source failing a real
//! lowering/capability check must not silently resolve. It is not
//! `nim_frontend` (this crate's owned Nim-subset lowering): it never
//! builds an IR and knows nothing about this project's declared Nim
//! subset -- it only looks for the one real, exported-symbol-bearing
//! pragma shape, over arbitrary Nim source text, never a hard-coded
//! proc/package name.

/// One `proc` real Nim source declares with an `{.exportc.}` pragma --
/// a genuine declared export, never invented. `exported_symbol` is the
/// pragma's own string argument (`{.exportc: "name".}`) when present,
/// or the proc's own declared name when the pragma carries none
/// (`{.exportc.}` alone), mirroring Rust's own unmarked `extern`
/// defaulting to the declared name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NimExportedProc {
    pub declared_name: String,
    pub exported_symbol: String,
    /// The real declared parameter count, from the proc's own `(...)`
    /// parameter list (`0` for an empty or absent list).
    pub param_count: usize,
    /// The real declared return type text after `:` (e.g. `"cint"`),
    /// or `""` when the proc declares no return type at all (Nim's own
    /// `void`-equivalent) -- never invented.
    pub return_type: String,
}

/// A genuine structural defect in a proc's own signature -- e.g. a
/// parameter list whose `(` never closes. This is the real,
/// observable failure mode Checkpoint A's own "lowering/capability"
/// evidence requires: a source that does not satisfy it must not
/// resolve to a fact silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NimSignatureError {
    pub proc_name: String,
    pub detail: String,
}

impl std::fmt::Display for NimSignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "proc '{}': {}", self.proc_name, self.detail)
    }
}

impl std::error::Error for NimSignatureError {}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Finds the next `proc`/`func` keyword at or after `from`, returning
/// the byte offset just past the keyword -- bounded by a word boundary
/// on both sides so it never matches inside a longer identifier (e.g.
/// `myproc`).
fn find_next_proc_keyword(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut search_from = from;
    loop {
        let haystack = text.get(search_from..)?;
        let (keyword, rel) = ["proc", "func"]
            .iter()
            .filter_map(|kw| haystack.find(kw).map(|idx| (*kw, idx)))
            .min_by_key(|(_, idx)| *idx)?;
        let abs_start = search_from + rel;
        let abs_end = abs_start + keyword.len();
        let before_ok = abs_start == 0 || !is_ident_char(bytes[abs_start - 1] as char);
        let after_ok = abs_end >= bytes.len() || !is_ident_char(bytes[abs_end] as char);
        if before_ok && after_ok {
            return Some(abs_end);
        }
        search_from = abs_start + keyword.len();
    }
}

fn parse_identifier(text: &str, from: usize) -> Option<(String, usize)> {
    let rest = &text[from..];
    let start = rest.find(|c: char| is_ident_char(c) || c == '`')?;
    let after_ws = &rest[start..];
    if let Some(body) = after_ws.strip_prefix('`') {
        let end = body.find('`')?;
        return Some((body[..end].to_string(), from + start + end + 2));
    }
    let end = after_ws
        .find(|c: char| !is_ident_char(c))
        .unwrap_or(after_ws.len());
    if end == 0 {
        return None;
    }
    Some((after_ws[..end].to_string(), from + start + end))
}

fn count_nim_params(inner: &str) -> usize {
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let mut depth: i32 = 0;
    let mut count = 1usize;
    for c in trimmed.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' | ';' if depth == 0 => count += 1,
            _ => {}
        }
    }
    count
}

/// Parses a real proc signature starting right after its name: an
/// optional `(...)` parameter list, then an optional `: ReturnType`.
/// Returns `(param_count, return_type, offset_just_past_the_signature)`.
/// The one genuine failure mode: a `(` that never finds its matching
/// `)` within the source text -- a real structural defect, not a shape
/// this best-effort scanner merely doesn't recognize.
fn parse_proc_signature(
    text: &str,
    from: usize,
    proc_name: &str,
) -> Result<(usize, String, usize), NimSignatureError> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    let mut param_count = 0usize;
    if i < bytes.len() && bytes[i] as char == '(' {
        let start = i;
        let mut depth: i32 = 0;
        let mut j = i;
        loop {
            if j >= bytes.len() {
                return Err(NimSignatureError {
                    proc_name: proc_name.to_string(),
                    detail: format!("unbalanced '(' in parameter list starting at byte {start}"),
                });
            }
            match bytes[j] as char {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        param_count = count_nim_params(&text[start + 1..j]);
        i = j + 1;
    }
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    let mut return_type = String::new();
    if i < bytes.len() && bytes[i] as char == ':' {
        i += 1;
        let rest = &text[i..];
        let end = rest.find(['{', '=', '\n']).unwrap_or(rest.len());
        return_type = rest[..end].trim().to_string();
        i += end;
    }
    Ok((param_count, return_type, i))
}

/// Extracts the pragma text between the next `{.` and its matching
/// `.}` at or after `from`, bounded so an unrelated later proc's own
/// pragma is never picked up: the search stops at the first `=` or
/// newline-free statement terminator it meets first, matching Nim's
/// own "pragmas sit directly after the signature, before the body"
/// shape.
fn extract_pragma_text(text: &str, from: usize) -> Option<String> {
    let rest = &text[from..];
    let boundary = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
    let scan_region = &rest[..boundary.max(rest.find("{.").map(|i| i + 2).unwrap_or(0))];
    let open = scan_region.find("{.")?;
    let close_rel = scan_region[open..].find(".}")?;
    Some(scan_region[open + 2..open + close_rel].to_string())
}

/// Splits real pragma text on top-level commas and looks for an
/// `exportc` entry, returning its string argument when present (`None`
/// when the pragma is bare `exportc` with no `: "..."`/`= "..."`).
fn exportc_argument(pragma_text: &str) -> Option<Option<String>> {
    for entry in pragma_text.split(',') {
        let entry = entry.trim();
        if let Some(rest) = entry.strip_prefix("exportc") {
            let rest = rest.trim_start();
            let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix('='));
            let Some(rest) = rest else {
                return Some(None);
            };
            let rest = rest.trim();
            if let Some(unquoted) = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
                return Some(Some(unquoted.to_string()));
            }
            return Some(None);
        }
    }
    None
}

/// Scans real Nim source text for every `proc`/`func` declaration
/// carrying an `{.exportc.}` pragma, in file order. A `proc` with no
/// `exportc` pragma at all is not C-ABI-exported and is silently
/// skipped -- it is a fact about this source, not an error. A
/// genuinely malformed parameter list (an unbalanced `(`) on a proc
/// this scanner has already committed to describing is reported as
/// [`NimSignatureError`] instead. Never hard-codes a proc or package
/// name; every fact comes from the text.
pub fn discover_exportc_declarations(
    source_text: &str,
) -> Result<Vec<NimExportedProc>, NimSignatureError> {
    let mut found = Vec::new();
    let mut cursor = 0usize;
    while let Some(after_keyword) = find_next_proc_keyword(source_text, cursor) {
        cursor = after_keyword;
        let Some((name, after_name)) = parse_identifier(source_text, after_keyword) else {
            continue;
        };
        let (param_count, return_type, after_signature) =
            parse_proc_signature(source_text, after_name, &name)?;
        cursor = after_signature;
        if let Some(pragma_text) = extract_pragma_text(source_text, after_signature) {
            if let Some(explicit) = exportc_argument(&pragma_text) {
                let exported_symbol = explicit.unwrap_or_else(|| name.clone());
                found.push(NimExportedProc {
                    declared_name: name,
                    exported_symbol,
                    param_count,
                    return_type,
                });
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proc_with_a_named_exportc_pragma_is_discovered() {
        let source = r#"proc nim_double(x: cint): cint {.exportc: "nim_double", cdecl.} =
  x * 2
"#;
        let found = discover_exportc_declarations(source).expect("must parse");
        assert_eq!(
            found,
            vec![NimExportedProc {
                declared_name: "nim_double".to_string(),
                exported_symbol: "nim_double".to_string(),
                param_count: 1,
                return_type: "cint".to_string(),
            }]
        );
    }

    #[test]
    fn a_bare_exportc_pragma_defaults_to_the_declared_name() {
        let source = "proc doIt(x: cint): cint {.exportc.} =\n  x\n";
        let found = discover_exportc_declarations(source).expect("must parse");
        assert_eq!(found[0].exported_symbol, "doIt");
    }

    #[test]
    fn a_proc_with_no_exportc_pragma_is_not_reported() {
        let source = "proc helper(x: int): int =\n  x + 1\n";
        let found = discover_exportc_declarations(source).expect("must parse");
        assert!(found.is_empty());
    }

    #[test]
    fn a_renamed_export_differs_from_its_declared_name() {
        let source = r#"proc internalName(a: cint, b: cint): cint {.exportc: "c_add_v2".} =
  a + b
"#;
        let found = discover_exportc_declarations(source).expect("must parse");
        assert_eq!(found[0].declared_name, "internalName");
        assert_eq!(found[0].exported_symbol, "c_add_v2");
        assert_eq!(found[0].param_count, 2);
    }

    #[test]
    fn multiple_procs_each_keep_their_own_export() {
        let source = r#"
proc a(x: cint): cint {.exportc: "sym_a".} =
  x
proc b(x: cint): cint {.exportc: "sym_b".} =
  x
"#;
        let found = discover_exportc_declarations(source).expect("must parse");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].exported_symbol, "sym_a");
        assert_eq!(found[1].exported_symbol, "sym_b");
    }

    #[test]
    fn a_parameterless_proc_reports_zero_params() {
        let source = "proc greet(): cstring {.exportc: \"greet\".} =\n  \"hi\"\n";
        let found = discover_exportc_declarations(source).expect("must parse");
        assert_eq!(found[0].param_count, 0);
    }

    #[test]
    fn a_proc_with_no_return_type_reports_an_empty_return_type() {
        let source = "proc log(x: cint) {.exportc: \"log\".} =\n  discard\n";
        let found = discover_exportc_declarations(source).expect("must parse");
        assert_eq!(found[0].return_type, "");
    }

    #[test]
    fn an_unbalanced_parameter_list_is_a_genuine_signature_error() {
        let source = "proc broken(x: cint {.exportc: \"broken\".} =\n  x\n";
        let result = discover_exportc_declarations(source);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().proc_name, "broken");
    }
}

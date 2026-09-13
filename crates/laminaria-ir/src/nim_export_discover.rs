//! Issue #48 (G1): a shallow, best-effort scanner for Nim `proc`
//! declarations carrying an `{.exportc.}` pragma -- the Nim-side
//! counterpart to [`crate::foreign_discover`]'s Rust `extern` scan and
//! [`crate::c_header_discover`]'s C/C++ prototype scan. This module
//! never invokes the Nim compiler and never inspects a compiled
//! archive -- it turns real `.nim` source text into a small, typed
//! fact list, over the same "best-effort, never a correctness gate"
//! discipline the other two discovery modules use. It is not
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
}

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
/// skipped -- it is a fact about this source, not an error. Never
/// hard-codes a proc or package name; every fact comes from the text.
pub fn discover_exportc_declarations(source_text: &str) -> Vec<NimExportedProc> {
    let mut found = Vec::new();
    let mut cursor = 0usize;
    while let Some(after_keyword) = find_next_proc_keyword(source_text, cursor) {
        cursor = after_keyword;
        let Some((name, after_name)) = parse_identifier(source_text, after_keyword) else {
            continue;
        };
        cursor = after_name;
        if let Some(pragma_text) = extract_pragma_text(source_text, after_name) {
            if let Some(explicit) = exportc_argument(&pragma_text) {
                let exported_symbol = explicit.unwrap_or_else(|| name.clone());
                found.push(NimExportedProc {
                    declared_name: name,
                    exported_symbol,
                });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proc_with_a_named_exportc_pragma_is_discovered() {
        let source = r#"proc nim_double(x: cint): cint {.exportc: "nim_double", cdecl.} =
  x * 2
"#;
        let found = discover_exportc_declarations(source);
        assert_eq!(
            found,
            vec![NimExportedProc {
                declared_name: "nim_double".to_string(),
                exported_symbol: "nim_double".to_string(),
            }]
        );
    }

    #[test]
    fn a_bare_exportc_pragma_defaults_to_the_declared_name() {
        let source = "proc doIt(x: cint): cint {.exportc.} =\n  x\n";
        let found = discover_exportc_declarations(source);
        assert_eq!(found[0].exported_symbol, "doIt");
    }

    #[test]
    fn a_proc_with_no_exportc_pragma_is_not_reported() {
        let source = "proc helper(x: int): int =\n  x + 1\n";
        let found = discover_exportc_declarations(source);
        assert!(found.is_empty());
    }

    #[test]
    fn a_renamed_export_differs_from_its_declared_name() {
        let source = r#"proc internalName(a: cint, b: cint): cint {.exportc: "c_add_v2".} =
  a + b
"#;
        let found = discover_exportc_declarations(source);
        assert_eq!(found[0].declared_name, "internalName");
        assert_eq!(found[0].exported_symbol, "c_add_v2");
    }

    #[test]
    fn multiple_procs_each_keep_their_own_export() {
        let source = r#"
proc a(x: cint): cint {.exportc: "sym_a".} =
  x
proc b(x: cint): cint {.exportc: "sym_b".} =
  x
"#;
        let found = discover_exportc_declarations(source);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].exported_symbol, "sym_a");
        assert_eq!(found[1].exported_symbol, "sym_b");
    }
}

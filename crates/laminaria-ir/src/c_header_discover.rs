//! Issue #48 (G1): a shallow, best-effort scanner for C/C++ function
//! *declarations* (prototypes), the C/C++-side counterpart to
//! [`crate::foreign_discover::discover_foreign_function_requirements`]'s
//! Rust-side `extern` scan. This module never invokes a compiler and
//! never inspects a compiled object/archive (no `nm`, no `cc`) -- it
//! turns real header source text into a small, typed fact list, the
//! same "best-effort fact finder over whatever syntax the file actually
//! contains, never a correctness gate" discipline
//! [`crate::discover::discover_called_functions`] already established.
//!
//! C and C++ have no single formal grammar this crate owns (unlike the
//! declared Rust/Nim subset `rust_frontend`/`nim_frontend` lower), so
//! this is deliberately not a real C/C++ parser: it recognizes the one
//! shape every plain function prototype shares -- some declarator text,
//! an identifier, a parenthesized parameter list, then `;` -- and
//! ignores everything else (macros, typedefs, structs, `#include`/other
//! preprocessor directives, `extern "C" {` / `}` wrapper tokens, which
//! fall out of the parenthesis-and-semicolon shape on their own). It
//! never hard-codes a package or symbol name; every fact it reports
//! comes from the input text.

/// One function declaration (prototype) found in real C/C++ header
/// text -- a genuine *declared export*, never invented. `param_count`
/// counts comma-separated parameter entries, `void` and an empty
/// parameter list both counting as zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CDeclaredFunction {
    pub name: String,
    pub param_count: usize,
    /// The declared return type text (e.g. `"int"`), a direct
    /// substring of the real declaration -- never invented.
    pub return_type: String,
}

/// Strips `/* ... */` and `// ...` comments from real C/C++ source
/// text, preserving every other byte (including newlines, so any
/// caller reporting line numbers over the result stays aligned) --
/// deliberately unaware of string/char literals containing `//`/`/*`,
/// since none of this project's own fixture headers ever need that.
fn strip_comments(source_text: &str) -> String {
    let mut out = String::with_capacity(source_text.len());
    let mut chars = source_text.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '/' {
            match chars.peek() {
                Some((_, '/')) => {
                    while let Some((_, c2)) = chars.peek() {
                        if *c2 == '\n' {
                            break;
                        }
                        chars.next();
                    }
                    continue;
                }
                Some((_, '*')) => {
                    chars.next();
                    let mut prev = ' ';
                    for (_, c2) in chars.by_ref() {
                        if prev == '*' && c2 == '/' {
                            break;
                        }
                        if c2 == '\n' {
                            out.push('\n');
                        }
                        prev = c2;
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The last maximal identifier run in `declarator_text`, and its start
/// byte offset -- for `extern "C" {\nint c_add`, that is `c_add` (the
/// return type, any `extern`/`static`/linkage-string tokens, and any
/// wrapping `extern "C" {` are all *earlier* identifier runs, so taking
/// the *last* one is what makes this immune to that wrapper without
/// special-casing it). The start offset lets a caller recover
/// everything *before* the name as the declared return type.
fn last_identifier_with_start(declarator_text: &str) -> Option<(String, usize)> {
    let mut best: Option<(String, usize)> = None;
    let mut current = String::new();
    let mut current_start = 0usize;
    for (idx, c) in declarator_text.char_indices() {
        if is_ident_char(c) {
            if current.is_empty() {
                current_start = idx;
            }
            current.push(c);
        } else if !current.is_empty() {
            best = Some((std::mem::take(&mut current), current_start));
        }
    }
    if !current.is_empty() {
        best = Some((current, current_start));
    }
    best
}

/// The declared return type text: `declarator_text` with the trailing
/// name occurrence removed, whitespace-collapsed and trimmed. Never
/// invented -- a direct substring of the real declaration.
fn return_type_text(declarator_text: &str, name_start: usize) -> String {
    declarator_text[..name_start]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Replaces every real `extern "C" {`/`extern "C++" {` opener with
/// equal-length whitespace (preserving every other byte's position, so
/// line/column bookkeeping stays aligned) -- pure noise-removal, never
/// a structural C++ parse.
fn strip_extern_block_openers(text: &str) -> String {
    const PATTERNS: [&str; 2] = ["extern \"C++\"", "extern \"C\""];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let mut best: Option<(usize, usize)> = None; // (pos, total_len_including_brace)
        for pattern in PATTERNS {
            let Some(pos) = rest.find(pattern) else {
                continue;
            };
            let after = &rest[pos + pattern.len()..];
            let ws_len = after.len() - after.trim_start().len();
            if !after[ws_len..].starts_with('{') {
                continue;
            }
            let total_len = pattern.len() + ws_len + 1;
            let is_better = match best {
                None => true,
                Some((best_pos, _)) => pos < best_pos,
            };
            if is_better {
                best = Some((pos, total_len));
            }
        }
        match best {
            Some((pos, total_len)) => {
                out.push_str(&rest[..pos]);
                out.push_str(&" ".repeat(total_len));
                rest = &rest[pos + total_len..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

fn count_params(params_text: &str) -> usize {
    let trimmed = params_text.trim();
    if trimmed.is_empty() || trimmed == "void" {
        return 0;
    }
    let mut depth: i32 = 0;
    let mut count = 1usize;
    for c in trimmed.chars() {
        match c {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth -= 1,
            ',' if depth == 0 => count += 1,
            _ => {}
        }
    }
    count
}

/// Scans real C/C++ header (or source) text for every plain function
/// prototype (`<declarator> <name> ( <params> ) ;`), in file order.
/// Never hard-codes a name -- every declaration the file's own text
/// contains is reported. Statements that don't end in `;` (an unclosed
/// `extern "C" {`, an `#include`, a macro without a trailing `;`) never
/// produce a fact, since this scanner only ever looks inside
/// `;`-terminated statements.
pub fn discover_c_declared_functions(source_text: &str) -> Vec<CDeclaredFunction> {
    let stripped = strip_comments(source_text);
    // Preprocessor directives (`#ifndef`, `#define`, `#include`, ...)
    // have no terminating `;`, so a directive immediately followed by a
    // real declaration would otherwise merge into that declaration's
    // own `;`-delimited statement; dropping each directive *line*
    // first keeps the later split-on-`;` pass looking at declaration
    // text only.
    let without_directives: String = stripped
        .lines()
        .map(|line| {
            if line.trim_start().starts_with('#') {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // An `extern "C" {`/`extern "C++" {` wrapper opener doesn't affect
    // *name* extraction (the name is always the last identifier before
    // `(`), but it would otherwise contaminate the *return-type* text
    // of the first declaration inside the block with its own leftover
    // tokens -- stripped here, once, rather than special-cased per
    // declaration.
    let without_extern_openers = strip_extern_block_openers(&without_directives);
    let mut found = Vec::new();
    for statement in without_extern_openers.split(';') {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }
        let Some(open) = statement.find('(') else {
            continue;
        };
        let Some(close) = statement.rfind(')') else {
            continue;
        };
        if close < open {
            continue;
        }
        let declarator = &statement[..open];
        let params = &statement[open + 1..close];
        // A function *definition* (`{ ... }` body) rather than a
        // prototype still parses as a declarator+params here since we
        // split on `;`, but a definition's body has no `;` inside this
        // statement's own slice unless the body itself is empty --
        // this module only ever runs over `.h` header text in this
        // project, where a body would be unusual; a stray body is
        // simply reported as a declaration too, which is harmless
        // (headers this project reads only ever declare, never define).
        let Some((name, name_start)) = last_identifier_with_start(declarator) else {
            continue;
        };
        found.push(CDeclaredFunction {
            return_type: return_type_text(declarator, name_start),
            name,
            param_count: count_params(params),
        });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simple_prototype_is_discovered() {
        let source = "int c_add(int a, int b);\n";
        let found = discover_c_declared_functions(source);
        assert_eq!(
            found,
            vec![CDeclaredFunction {
                name: "c_add".to_string(),
                param_count: 2,
                return_type: "int".to_string(),
            }]
        );
    }

    #[test]
    fn extern_c_wrapped_prototypes_are_each_discovered() {
        let source = r#"
            #ifndef CADD_H
            #define CADD_H
            extern "C" {
            int c_add(int a, int b);
            }
            #endif
        "#;
        let found = discover_c_declared_functions(source);
        assert_eq!(
            found,
            vec![CDeclaredFunction {
                name: "c_add".to_string(),
                param_count: 2,
                return_type: "int".to_string(),
            }]
        );
    }

    #[test]
    fn a_void_parameter_list_counts_as_zero_params() {
        let source = "int noop(void);";
        let found = discover_c_declared_functions(source);
        assert_eq!(found[0].param_count, 0);
    }

    #[test]
    fn comments_around_a_declaration_are_ignored() {
        let source = "/* doc */\nint cpp_max_i32(int a, int b); // trailing\n";
        let found = discover_c_declared_functions(source);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "cpp_max_i32");
    }

    #[test]
    fn a_file_with_no_declarations_discovers_nothing() {
        let source = "#define FOO 1\ntypedef int foo_t;\n";
        let found = discover_c_declared_functions(source);
        assert!(found.is_empty());
    }

    #[test]
    fn multiple_declarations_are_each_discovered_in_order() {
        let source = "int c_add_v2(int a, int b);\nint other(int x);";
        let found = discover_c_declared_functions(source);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "c_add_v2");
        assert_eq!(found[1].name, "other");
    }

    #[test]
    fn extern_c_wrapping_does_not_contaminate_the_return_type() {
        let source = r#"
            extern "C" {
            int c_add(int a, int b);
            }
        "#;
        let found = discover_c_declared_functions(source);
        assert_eq!(found[0].return_type, "int");
    }

    #[test]
    fn a_multi_word_return_type_is_captured() {
        let source = "unsigned long compute(int a);";
        let found = discover_c_declared_functions(source);
        assert_eq!(found[0].return_type, "unsigned long");
    }
}

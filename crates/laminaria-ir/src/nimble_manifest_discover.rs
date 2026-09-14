//! Issue #48 (G1) Checkpoint D: a pure-text parser for a real, explicit
//! **declared subset** of `.nimble` manifest syntax. A real `.nimble`
//! file is arbitrary NimScript -- this module never executes any of
//! it. It recognizes exactly two statement shapes:
//!
//! - `key = "value"` for a fixed set of known keys (`version`,
//!   `author`, `description`, `license`, `srcDir`, `binDir`,
//!   `backend`);
//! - `requires "<dependency spec>"` (one or more string arguments,
//!   comma-separated).
//!
//! Any other real statement (a `task` block, an `import`, a `when`/`if`
//! conditional, a variable, string interpolation, a bare expression) is
//! **not** guessed at or silently skipped -- it is reported as
//! [`UnsupportedManifestConstruct`], naming the exact line and text
//! that fell outside the supported subset, so a caller can reject it
//! with a structured diagnostic rather than resolve against an
//! incomplete or wrong picture of the manifest.

use std::collections::BTreeMap;

/// One `requires "..."` entry, itself parsed into `name`/`operator`/
/// `version` when the string has that shape (`"<name> <op> <version>"`
/// or bare `"<name>"`) -- never invented when the shape doesn't match;
/// `operator`/`version` are simply `None` and `raw` is always the
/// exact original string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NimbleRequirement {
    pub raw: String,
    pub name: String,
    pub operator: Option<String>,
    pub version: Option<String>,
}

fn parse_version_tuple(s: &str) -> Option<Vec<u64>> {
    s.split('.').map(|p| p.parse::<u64>().ok()).collect()
}

fn pad_to_equal_length(mut a: Vec<u64>, mut b: Vec<u64>) -> (Vec<u64>, Vec<u64>) {
    while a.len() < b.len() {
        a.push(0);
    }
    while b.len() < a.len() {
        b.push(0);
    }
    (a, b)
}

impl NimbleRequirement {
    /// Whether a real, already-resolved candidate version (e.g. a real
    /// `nimble.lock`-pinned version) satisfies this requirement's own
    /// operator/version constraint. `Some(true)`/`Some(false)` is a
    /// real, checked verdict; `None` means this requirement's own
    /// shape or the candidate version's own text falls outside the
    /// declared supported subset (a bare name requirement with no
    /// operator is always `Some(true)` -- any real candidate satisfies
    /// it -- but a non-numeric-dotted version, or an operator outside
    /// `<`/`<=`/`==`/`>=`/`>`, cannot be compared and must not be
    /// silently treated as satisfied).
    pub fn is_satisfied_by(&self, candidate_version: &str) -> Option<bool> {
        let (op, req_version) = match (self.operator.as_deref(), self.version.as_deref()) {
            (Some(op), Some(v)) => (op, v),
            _ => return Some(true),
        };
        let req_tuple = parse_version_tuple(req_version)?;
        let cand_tuple = parse_version_tuple(candidate_version)?;
        let (req_tuple, cand_tuple) = pad_to_equal_length(req_tuple, cand_tuple);
        Some(match op {
            "<" => cand_tuple < req_tuple,
            "<=" => cand_tuple <= req_tuple,
            "==" => cand_tuple == req_tuple,
            ">=" => cand_tuple >= req_tuple,
            ">" => cand_tuple > req_tuple,
            _ => return None,
        })
    }
}

fn parse_requirement(raw: &str) -> NimbleRequirement {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    match parts.as_slice() {
        [name] => NimbleRequirement {
            raw: raw.to_string(),
            name: name.to_string(),
            operator: None,
            version: None,
        },
        [name, op, version] if ["<", "<=", "==", ">=", ">"].contains(op) => NimbleRequirement {
            raw: raw.to_string(),
            name: name.to_string(),
            operator: Some((*op).to_string()),
            version: Some((*version).to_string()),
        },
        _ => NimbleRequirement {
            raw: raw.to_string(),
            name: parts.first().unwrap_or(&raw).to_string(),
            operator: None,
            version: None,
        },
    }
}

/// The real facts a `.nimble` manifest declares, within the supported
/// subset -- never a fully evaluated NimScript environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NimbleManifestFacts {
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub src_dir: Option<String>,
    pub bin_dir: Option<String>,
    pub backend: Option<String>,
    pub requires: Vec<NimbleRequirement>,
}

/// The one real statement this module could not place in the
/// supported subset -- a genuine structural rejection, never silently
/// dropped or guessed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedManifestConstruct {
    pub line: usize,
    pub text: String,
}

impl std::fmt::Display for UnsupportedManifestConstruct {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "line {}: unsupported .nimble construct outside the declared subset: {:?}",
            self.line, self.text
        )
    }
}

impl std::error::Error for UnsupportedManifestConstruct {}

/// Strips a real `#` line comment, respecting simple double-quoted
/// string literals (a `#` inside `"..."` does not start a comment) --
/// this manifest dialect never nests or escapes quotes, so a plain
/// in/out toggle is sufficient.
fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    for (idx, c) in line.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..idx],
            _ => {}
        }
    }
    line
}

/// Matches a real `key = "value"` statement for one specific known
/// key, returning the unquoted value.
fn match_key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=')?;
    let rest = rest.trim();
    let rest = rest.strip_prefix('"')?;
    rest.strip_suffix('"')
}

/// Splits `inner` on top-level commas (outside quotes) -- used for
/// `requires "a", "b"`.
fn split_top_level_commas(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_string = false;
    for (idx, c) in inner.char_indices() {
        match c {
            '"' => in_string = !in_string,
            ',' if !in_string => {
                parts.push(inner[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        parts.push(last);
    }
    parts
}

/// Matches a real `requires "<spec>"[, "<spec>", ...]` statement,
/// returning each parsed requirement.
fn match_requires(line: &str) -> Option<Vec<NimbleRequirement>> {
    let rest = line.strip_prefix("requires")?;
    let rest = rest.trim_start();
    if rest.is_empty() || !rest.starts_with('"') {
        return None;
    }
    let mut requirements = Vec::new();
    for part in split_top_level_commas(rest) {
        let literal = part.strip_prefix('"')?.strip_suffix('"')?;
        requirements.push(parse_requirement(literal));
    }
    Some(requirements)
}

/// Parses real `.nimble` manifest text within the declared subset this
/// module's own doc comment fixes. Every non-blank, non-comment line
/// must match a known `key = "value"` shape or `requires "..."`; the
/// first line that doesn't is reported as
/// [`UnsupportedManifestConstruct`], not silently skipped.
pub fn parse_nimble_manifest(
    text: &str,
) -> Result<NimbleManifestFacts, UnsupportedManifestConstruct> {
    let mut facts = NimbleManifestFacts::default();
    let known_keys: BTreeMap<&str, fn(&mut NimbleManifestFacts, String)> = BTreeMap::from([
        (
            "version",
            (|f: &mut NimbleManifestFacts, v: String| f.version = Some(v))
                as fn(&mut NimbleManifestFacts, String),
        ),
        (
            "author",
            (|f: &mut NimbleManifestFacts, v: String| f.author = Some(v))
                as fn(&mut NimbleManifestFacts, String),
        ),
        (
            "description",
            (|f: &mut NimbleManifestFacts, v: String| f.description = Some(v))
                as fn(&mut NimbleManifestFacts, String),
        ),
        (
            "license",
            (|f: &mut NimbleManifestFacts, v: String| f.license = Some(v))
                as fn(&mut NimbleManifestFacts, String),
        ),
        (
            "srcDir",
            (|f: &mut NimbleManifestFacts, v: String| f.src_dir = Some(v))
                as fn(&mut NimbleManifestFacts, String),
        ),
        (
            "binDir",
            (|f: &mut NimbleManifestFacts, v: String| f.bin_dir = Some(v))
                as fn(&mut NimbleManifestFacts, String),
        ),
        (
            "backend",
            (|f: &mut NimbleManifestFacts, v: String| f.backend = Some(v))
                as fn(&mut NimbleManifestFacts, String),
        ),
    ]);

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        let mut matched = false;
        for (key, setter) in &known_keys {
            if let Some(value) = match_key_value(line, key) {
                setter(&mut facts, value.to_string());
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        if let Some(reqs) = match_requires(line) {
            facts.requires.extend(reqs);
            continue;
        }
        return Err(UnsupportedManifestConstruct {
            line: line_no,
            text: line.to_string(),
        });
    }
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimal_real_manifest_is_parsed_fully() {
        let text = r#"
version       = "0.1.0"
author        = "LAMINARIA"
description   = "Issue #48 fixture: doubles an i32 via a real Nimble package."
license       = "MIT"
srcDir        = "src"

requires "nim >= 2.0.0"
"#;
        let facts = parse_nimble_manifest(text).expect("must parse");
        assert_eq!(facts.version.as_deref(), Some("0.1.0"));
        assert_eq!(facts.author.as_deref(), Some("LAMINARIA"));
        assert_eq!(facts.src_dir.as_deref(), Some("src"));
        assert_eq!(facts.requires.len(), 1);
        assert_eq!(facts.requires[0].name, "nim");
        assert_eq!(facts.requires[0].operator.as_deref(), Some(">="));
        assert_eq!(facts.requires[0].version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn a_comment_line_and_trailing_comment_are_ignored() {
        let text = "# a real comment\nversion = \"1.0.0\" # trailing note\n";
        let facts = parse_nimble_manifest(text).expect("must parse");
        assert_eq!(facts.version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn multiple_comma_separated_requires_are_each_parsed() {
        let text = r#"requires "nim >= 2.0.0", "somepkg >= 1.0.0""#;
        let facts = parse_nimble_manifest(text).expect("must parse");
        assert_eq!(facts.requires.len(), 2);
        assert_eq!(facts.requires[1].name, "somepkg");
    }

    #[test]
    fn a_bare_dependency_name_with_no_constraint_is_supported() {
        let text = r#"requires "somepkg""#;
        let facts = parse_nimble_manifest(text).expect("must parse");
        assert_eq!(facts.requires[0].name, "somepkg");
        assert_eq!(facts.requires[0].operator, None);
    }

    #[test]
    fn a_task_block_is_an_unsupported_construct() {
        let text =
            "version = \"1.0.0\"\ntask test, \"Run tests\":\n  exec \"nim c -r tests.nim\"\n";
        let result = parse_nimble_manifest(text);
        let err = result.expect_err("a task block is outside the declared subset");
        assert_eq!(err.line, 2);
        assert!(err.text.starts_with("task"));
    }

    #[test]
    fn a_when_conditional_is_an_unsupported_construct() {
        let text = "version = \"1.0.0\"\nwhen defined(release):\n  version = \"1.0.0-release\"\n";
        let result = parse_nimble_manifest(text);
        assert!(result.is_err());
    }

    #[test]
    fn an_import_statement_is_an_unsupported_construct() {
        let text = "import os\nversion = \"1.0.0\"\n";
        let result = parse_nimble_manifest(text);
        let err = result.expect_err("an import is outside the declared subset");
        assert_eq!(err.line, 1);
    }

    #[test]
    fn a_gte_requirement_is_satisfied_by_a_higher_pinned_version() {
        let req = parse_requirement("nim >= 2.0.0");
        assert_eq!(req.is_satisfied_by("2.2.10"), Some(true));
    }

    #[test]
    fn a_gte_requirement_is_not_satisfied_by_a_lower_pinned_version() {
        let req = parse_requirement("nim >= 2.0.0");
        assert_eq!(req.is_satisfied_by("1.9.9"), Some(false));
    }

    #[test]
    fn a_bare_name_requirement_is_satisfied_by_any_version() {
        let req = parse_requirement("somepkg");
        assert_eq!(req.is_satisfied_by("0.0.1"), Some(true));
    }

    #[test]
    fn a_non_numeric_pinned_version_cannot_be_compared() {
        let req = parse_requirement("nim >= 2.0.0");
        assert_eq!(req.is_satisfied_by("devel"), None);
    }

    #[test]
    fn version_tuples_of_different_length_compare_correctly() {
        let req = parse_requirement("nim >= 2.0");
        assert_eq!(req.is_satisfied_by("2.0.0"), Some(true));
    }
}

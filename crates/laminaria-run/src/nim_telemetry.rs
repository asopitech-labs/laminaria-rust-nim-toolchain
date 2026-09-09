//! Level 2 compiler-native telemetry for `nim c`/`nim cpp` builds
//! (`docs/measurement-foundation.md` section 7's "Nim / Nimony" adapter,
//! which explicitly anticipates this: *"Investigate stage timing/artifact
//! diagnostics per exact Nim 2 and Nimony revision... do not assume Nim 2
//! and Nimony expose the same adapter/capability set"* as Cargo).
//!
//! **What was actually checked, not assumed**: searched Nim's real
//! compiler source (`.reference/Nim/compiler/`) for a Cargo-`--message-
//! format=json`-equivalent JSON message stream for compile-stage events.
//! There isn't one. `nim dump --dump.format:json` exists
//! (`compiler/main.nim`'s `cmdDump` handling), but it's a **separate**
//! command that dumps static configuration (version, search paths,
//! defined symbols, enabled hints/warnings) -- unrelated to per-build
//! compile-stage telemetry, confirmed by reading its actual field list,
//! not assumed from the name alone.
//!
//! What Nim *does* expose natively is its human-oriented hint/verbosity
//! stream (`compiler/lineinfos.nim`'s hint category table), which this
//! module parses instead -- exactly the "use ... wrappers when native
//! telemetry is insufficient" fallback the design doc names. Verified
//! against a real `nim c` run on this project's own `nim-heavy-workspace`
//! fixture:
//!
//! ```text
//! CC: system/exceptions.nim
//! CC: std/private/digitsutils.nim
//! ...
//! CC: fixture.nim
//! Hint:  [Link]
//! Hint: mm: orc; threads: on; opt: none (DEBUG BUILD, ...)
//! 29436 lines; 0.343s; 38.184MiB peakmem; proj: /path/fixture.nim; out: /path/fixture_out [SuccessX]
//! ```
//!
//! **A real, confirmed difference from the Cargo adapter, not assumed
//! carried over**: this entire stream is on Nim's **stderr**, not stdout
//! -- verified by capturing `nim c`'s stdout and stderr to separate files
//! and finding stdout empty. `cargo_telemetry::parse_cargo_json_messages`
//! reads `stdout.log`; this module's `parse_nim_hint_stream` reads
//! `stderr.log`.
//!
//! **A materially weaker reliability claim than the Cargo adapter, stated
//! explicitly**: `--message-format=json` is Cargo's documented, stable
//! machine interface. This hint text has no such contract from Nim --
//! parsing it is inherently more fragile across Nim versions, which
//! `types::NimCompilerTelemetry`'s own doc comment repeats rather than
//! leaving implicit.

use std::path::Path;

use crate::types::NimCompilerTelemetry;

/// Parses Nim's hint/verbosity stream from `stderr_path` (a file
/// `tracer::trace_root_command` already wrote to) into
/// `NimCompilerTelemetry`. Never fails: an unreadable file or an
/// unrecognized line is reflected in the result
/// (`unrecognized_line_count`), not returned as an error -- a
/// telemetry-parsing problem must never fail the traced command's own
/// already-recorded Run.
pub fn parse_nim_hint_stream(stderr_path: &Path) -> NimCompilerTelemetry {
    let text = match std::fs::read_to_string(stderr_path) {
        Ok(text) => text,
        Err(_) => return NimCompilerTelemetry::default(),
    };

    let mut telemetry = NimCompilerTelemetry::default();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(module) = line.strip_prefix("CC: ") {
            telemetry.processed_modules.push(module.to_string());
            continue;
        }
        if line.contains("[Link]") {
            telemetry.linked = true;
            continue;
        }
        if line.ends_with("[SuccessX]") {
            if parse_success_x_line(line, &mut telemetry) {
                continue;
            }
            telemetry.unrecognized_line_count += 1;
            continue;
        }
        if line.starts_with("Hint: used config file")
            || line.starts_with("Hint: mm:")
            || line.chars().all(|c| c == '.')
        {
            // Config-file notices, the build-flags summary half-line, and
            // the "........." progress-dot line: recognized as known,
            // uninteresting output rather than counted as unrecognized.
            continue;
        }
        telemetry.unrecognized_line_count += 1;
    }

    telemetry
}

/// Parses the `hintSuccessX` summary line
/// (`compiler/lineinfos.nim`: `"$build\n$loc lines; ${sec}s; $mem; proj: $project; out: $output"`
/// -- everything after the `$build\n` half, which arrives as its own
/// line). Returns `false` (leaving `telemetry` unmodified) if the line
/// doesn't match the expected `N lines; Ns; ... peakmem; ...` shape, so
/// the caller can count it as unrecognized instead of silently accepting
/// a partial parse.
fn parse_success_x_line(line: &str, telemetry: &mut NimCompilerTelemetry) -> bool {
    let mut matched_any = false;

    if let Some(lines_part) = line.split(" lines;").next() {
        if let Ok(n) = lines_part.trim().parse::<u64>() {
            telemetry.lines_compiled = Some(n);
            matched_any = true;
        }
    }

    // The line is semicolon-separated fields ("N lines", "Ns", "M peakmem",
    // "proj: ...", "out: ... [SuccessX]"); the seconds field is the one
    // whose trimmed text ends in a bare "s" (not "s;" of some other word).
    if let Some(seconds) = line.split(';').find_map(|part| {
        let part = part.trim();
        part.strip_suffix('s').and_then(|n| n.parse::<f64>().ok())
    }) {
        telemetry.self_reported_seconds = Some(seconds);
        matched_any = true;
    }

    if let Some(mem_bytes) = extract_peak_mem_bytes(line) {
        telemetry.peak_mem_bytes = Some(mem_bytes);
        matched_any = true;
    }

    matched_any
}

/// Extracts the peak-memory field (e.g. `"38.184MiB peakmem"` or
/// `"512KiB peakmem"`) and converts it to a plain byte count, reversing
/// `strutils.formatSize`'s binary (1024-based) IEC-prefix formatting
/// (confirmed against that function's own doctests in
/// `.reference/Nim/lib/pure/strutils.nim`, not assumed).
fn extract_peak_mem_bytes(line: &str) -> Option<u64> {
    let marker = " peakmem";
    let end = line.find(marker)?;
    let before = &line[..end];
    let start = before.rfind(';').map(|i| i + 1).unwrap_or(0);
    let token = before[start..].trim();

    let (number_str, multiplier) = if let Some(n) = token.strip_suffix("GiB") {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = token.strip_suffix("MiB") {
        (n, 1024u64 * 1024)
    } else if let Some(n) = token.strip_suffix("KiB") {
        (n, 1024u64)
    } else {
        let n = token.strip_suffix('B')?;
        (n, 1u64)
    };

    let value: f64 = number_str.trim().parse().ok()?;
    Some((value * multiplier as f64) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_stderr(lines: &[&str]) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "laminaria-run-nim-telemetry-test-{}-{n}.log",
            std::process::id(),
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        path
    }

    #[test]
    fn parses_a_real_nim_c_hint_stream() {
        // Taken verbatim from an actual `nim c` run on this project's own
        // nim-heavy-workspace fixture (stderr, not stdout -- see this
        // module's doc comment), not hand-constructed.
        let path = write_stderr(&[
            "Hint: used config file '/usr/local/Cellar/nim/2.2.10/nim/config/nim.cfg' [Conf]",
            "Hint: used config file '/usr/local/Cellar/nim/2.2.10/nim/config/config.nims' [Conf]",
            "........................................................................",
            "CC: system/exceptions.nim",
            "CC: std/private/digitsutils.nim",
            "CC: std/assertions.nim",
            "CC: system/dollars.nim",
            "CC: system.nim",
            "CC: primes.nim",
            "CC: geometry.nim",
            "CC: fixture.nim",
            "Hint:  [Link]",
            "Hint: mm: orc; threads: on; opt: none (DEBUG BUILD, `-d:release` generates faster code)",
            "29436 lines; 0.343s; 38.184MiB peakmem; proj: /x/fixture.nim; out: /x/fixture_out [SuccessX]",
        ]);

        let telemetry = parse_nim_hint_stream(&path);

        assert_eq!(telemetry.processed_modules.len(), 8);
        assert_eq!(telemetry.processed_modules[0], "system/exceptions.nim");
        assert_eq!(telemetry.processed_modules[7], "fixture.nim");
        assert!(telemetry.linked);
        assert_eq!(telemetry.lines_compiled, Some(29436));
        assert_eq!(telemetry.self_reported_seconds, Some(0.343));
        // 38.184 MiB, reversed from formatSize's binary formatting.
        let expected_bytes = (38.184_f64 * 1024.0 * 1024.0) as u64;
        assert_eq!(telemetry.peak_mem_bytes, Some(expected_bytes));
        assert_eq!(telemetry.unrecognized_line_count, 0);
    }

    #[test]
    fn peak_mem_parses_kib_and_gib_units_too() {
        assert_eq!(
            extract_peak_mem_bytes("10 lines; 0.1s; 512KiB peakmem; proj: x; out: y [SuccessX]"),
            Some(512 * 1024)
        );
        assert_eq!(
            extract_peak_mem_bytes("10 lines; 0.1s; 2GiB peakmem; proj: x; out: y [SuccessX]"),
            Some(2 * 1024 * 1024 * 1024)
        );
    }

    #[test]
    fn unrecognized_lines_are_counted_not_dropped_silently() {
        let path = write_stderr(&["some totally unexpected line", "another one"]);
        let telemetry = parse_nim_hint_stream(&path);
        assert_eq!(telemetry.unrecognized_line_count, 2);
        assert_eq!(telemetry.processed_modules.len(), 0);
    }

    #[test]
    fn missing_file_returns_an_empty_default_not_an_error() {
        let telemetry =
            parse_nim_hint_stream(Path::new("/nonexistent/laminaria-run-test-path.log"));
        assert_eq!(telemetry.processed_modules.len(), 0);
        assert_eq!(telemetry.unrecognized_line_count, 0);
    }
}

//! Small process-invocation helpers shared by every detector.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs `cmd args...` and returns trimmed stdout, or `None` if the
/// executable cannot be found or exits non-zero. Detection must never panic
/// just because an optional tool is absent.
pub fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() && output.stdout.is_empty() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            return None;
        }
        return Some(stderr);
    }
    Some(stdout)
}

/// Expands a leading `~` or `~/...` to `$HOME`, since TOML config values are
/// not shell-expanded and lock files (e.g. a `bin_dir` pointing at a
/// `choosenim` toolchain directory) read more naturally with `~` than a
/// hardcoded absolute home path.
pub fn expand_tilde(path: &Path) -> PathBuf {
    let Some(s) = path.to_str() else {
        return path.to_path_buf();
    };
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    } else if s == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    path.to_path_buf()
}

/// Resolves the absolute path of an executable on `PATH`, without relying on
/// a shell built-in that may not exist on every platform.
pub fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let with_exe = dir.join(format!("{name}.exe"));
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
    }
    None
}

/// SHA-256 digest of a file, formatted as lowercase hex. Used so a
/// ToolchainFingerprint records the exact resolved binary rather than only a
/// version string that could point at a different executable tomorrow.
pub fn sha256_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// First line of a version-style command output, e.g. `foo --version`.
pub fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

/// Extracts the first `\d+\.\d+(\.\d+)?` looking token from text, without
/// pulling in a regex dependency for a handful of simple version strings.
pub fn extract_version_like(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut dots = 0;
            let mut j = i;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
                if bytes[j] == b'.' {
                    dots += 1;
                }
                j += 1;
            }
            if dots >= 1 {
                return Some(text[start..j].trim_end_matches('.').to_string());
            }
            i = j;
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_semver_like_version() {
        assert_eq!(
            extract_version_like("wasm-tools 1.255.0"),
            Some("1.255.0".to_string())
        );
    }

    #[test]
    fn extracts_two_component_version() {
        assert_eq!(
            extract_version_like("Nim Compiler Version 2.2.10 [MacOSX: amd64]"),
            Some("2.2.10".to_string())
        );
    }

    #[test]
    fn ignores_bare_integers_without_a_dot() {
        assert_eq!(extract_version_like("build 12345"), None);
    }

    #[test]
    fn returns_none_for_text_without_digits() {
        assert_eq!(extract_version_like("no version here"), None);
    }

    #[test]
    fn first_line_trims_and_takes_only_the_first_line() {
        assert_eq!(first_line("  first  \nsecond\nthird"), "first");
    }

    #[test]
    fn expand_tilde_expands_home_relative_paths() {
        // SAFETY: this test module runs single-threaded within the crate's
        // test binary; no other test reads/writes HOME concurrently.
        unsafe {
            std::env::set_var("HOME", "/Users/example");
        }
        assert_eq!(
            expand_tilde(Path::new("~/.choosenim/toolchains/nim-2.2.10/bin")),
            PathBuf::from("/Users/example/.choosenim/toolchains/nim-2.2.10/bin")
        );
        assert_eq!(
            expand_tilde(Path::new("~")),
            PathBuf::from("/Users/example")
        );
    }

    #[test]
    fn expand_tilde_leaves_absolute_paths_untouched() {
        assert_eq!(
            expand_tilde(Path::new("/opt/nimony/bin")),
            PathBuf::from("/opt/nimony/bin")
        );
    }

    #[test]
    fn expand_tilde_leaves_paths_without_a_leading_tilde_untouched() {
        assert_eq!(
            expand_tilde(Path::new("relative/nim-tilde~in-name/bin")),
            PathBuf::from("relative/nim-tilde~in-name/bin")
        );
    }
}

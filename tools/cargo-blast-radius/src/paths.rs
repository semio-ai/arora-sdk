//! Lexical path handling. Paths are compared as `/`-separated strings relative
//! to the workspace root, so that globs, git output and cargo's absolute paths
//! all meet in one form.

use std::path::{Component, Path, PathBuf};

/// Resolves `.` and `..` without touching the filesystem. Leading `..` that
/// climb above the start are kept.
pub fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if out.last().is_some_and(|l| *l != "..") => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out.join("/")
}

/// Lexically normalizes an absolute path (`..` and `.` removed).
pub fn normalize_abs(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// `path` relative to `base`, both absolute, with `/` separators; climbs out
/// of `base` with `..` when needed. Empty when they are equal.
pub fn relative_to(path: &Path, base: &Path) -> String {
    let path = normalize_abs(path);
    let base = normalize_abs(base);
    let p: Vec<_> = path.components().collect();
    let b: Vec<_> = base.components().collect();
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".into(); b.len() - common];
    parts.extend(
        p[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize("modules/a/../../libs/cpp/"), "libs/cpp");
        assert_eq!(normalize("./a/./b"), "a/b");
        assert_eq!(normalize("../x/../../y"), "../../y");
    }

    #[test]
    fn relates() {
        let root = Path::new("/w/repo");
        assert_eq!(
            relative_to(Path::new("/w/repo/crates/a/src/lib.rs"), root),
            "crates/a/src/lib.rs"
        );
        assert_eq!(relative_to(Path::new("/w/other/x"), root), "../other/x");
        assert_eq!(relative_to(Path::new("/w/repo"), root), "");
        assert_eq!(relative_to(Path::new("/w/repo/a/../b"), root), "b");
    }
}

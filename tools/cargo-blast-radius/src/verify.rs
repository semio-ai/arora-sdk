//! Checks the file → package mapping against what cargo itself tracks.
//!
//! After a build, the target directory holds, for every compiled target,
//! rustc's dep-info (`*.d`: every source file, `include_str!`/`include_bytes!`
//! file and tracked proc-macro input that went into it) and, for every build
//! script run, the `rerun-if-changed` paths it declared. Those are the inputs
//! cargo uses to decide what to rebuild. If each of them, changed, would mark
//! the package that read it as affected, the selection can only skip what
//! cargo itself would not rebuild.
//!
//! Inputs cargo does not track (a build script that reads a file without
//! declaring it, a test that opens a fixture at run time) stay invisible here;
//! files outside every package count as global by default to cover them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::impact::{Impact, Reason};
use crate::paths::{normalize_abs, relative_to};
use crate::workspace::{FileClass, Workspace};

/// One file a package's target read, which a change to would not select it.
#[derive(Debug)]
pub struct Violation {
    pub package: usize,
    pub file: String,
    pub why: String,
}

#[derive(Default)]
pub struct Report {
    pub depinfo_files: usize,
    pub build_script_outputs: usize,
    pub inputs_checked: usize,
    pub violations: Vec<Violation>,
}

/// An input: the package that read it, whether a consumed artifact (lib, bin,
/// build script) depends on it or only tests/examples, and the path.
type Input = (usize, bool, PathBuf);

pub fn verify(ws: &Workspace, repo: &Path) -> Result<Report> {
    let mut report = Report::default();
    let mut inputs: Vec<Input> = Vec::new();
    for dir in &ws.target_dirs {
        walk(ws, dir, &mut inputs, &mut report)?;
    }

    let targets: Vec<PathBuf> = ws.target_dirs.iter().map(|d| normalize_abs(d)).collect();
    let repo = normalize_abs(repo);
    let mut seen = BTreeSet::new();
    let mut memo: HashMap<Vec<usize>, Impact> = HashMap::new();
    let mut violations = BTreeMap::new();
    for (package, consumed, path) in inputs {
        let path = normalize_abs(&path);
        // Generated files and the toolchain/registry are not ours to map.
        if !path.starts_with(&repo) || targets.iter().any(|t| path.starts_with(t)) {
            continue;
        }
        let rel = relative_to(&path, &ws.root);
        if !seen.insert((package, consumed, rel.clone())) {
            continue;
        }
        report.inputs_checked += 1;
        let why = match ws.classify(&rel) {
            FileClass::Global(_) => continue,
            FileClass::Ignored => "it matches an `ignore` pattern".to_owned(),
            FileClass::Owned(owners) => {
                let impact = memo.entry(owners.clone()).or_insert_with(|| {
                    let seeds = owners
                        .iter()
                        .map(|&o| (o, Reason::File(rel.clone())))
                        .collect();
                    Impact::propagate(ws, seeds)
                });
                let reached = if consumed {
                    impact.build[package].is_some()
                } else {
                    impact.test[package].is_some()
                };
                if reached {
                    continue;
                }
                let names: Vec<_> = owners
                    .iter()
                    .map(|&o| ws.packages[o].name.as_str())
                    .collect();
                format!(
                    "it belongs to {}, which {} does not depend on",
                    names.join(", "),
                    ws.packages[package].name
                )
            }
        };
        violations
            .entry((package, rel.clone()))
            .or_insert(Violation {
                package,
                file: rel,
                why,
            });
    }
    report.violations = violations.into_values().collect();
    Ok(report)
}

fn walk(ws: &Workspace, dir: &Path, inputs: &mut Vec<Input>, report: &mut Report) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(ty) = entry.file_type() else { continue };
        if ty.is_dir() {
            // Incremental caches, fingerprints and docs hold no dep-info we
            // need; a build script's OUT_DIR holds foreign `.d` files (cmake,
            // cc) in a different dialect.
            if !matches!(
                name.as_ref(),
                "incremental" | ".fingerprint" | "doc" | "out"
            ) {
                walk(ws, &path, inputs, report)?;
            }
        } else if name.ends_with(".d") {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(deps) = parse_depinfo(&text) {
                let resolved: Vec<PathBuf> = deps.iter().map(|d| ws.root.join(d)).collect();
                // The crate root comes first; it names the target.
                if let Some(&(package, consumed)) = resolved
                    .first()
                    .and_then(|r| ws.target_roots.get(&normalize_abs(r)))
                {
                    report.depinfo_files += 1;
                    inputs.extend(resolved.into_iter().map(|p| (package, consumed, p)));
                }
            }
        } else if name == "output" && path.with_file_name("root-output").exists() {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let paths = rerun_if_changed(&text);
            for package in build_script_package(ws, &path) {
                let dir = ws
                    .root
                    .join(ws.packages[package].dir.as_deref().unwrap_or(""));
                report.build_script_outputs += 1;
                inputs.extend(paths.iter().map(|p| (package, true, dir.join(p))));
            }
        }
    }
    Ok(())
}

/// The dependencies listed in a rustc dep-info file, crate root first.
///
/// rustc's own file (in `deps/`, `build/…/`) opens with a rule for the `.d`
/// itself. Cargo also writes a merged copy next to each uplifted artifact,
/// listing the sources of every path dependency in sorted order; its rule is
/// for the artifact, and it is skipped: it adds nothing and has no root.
pub fn parse_depinfo(text: &str) -> Option<Vec<String>> {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with('#'))?;
    let colon = line
        .find(": ")
        .or_else(|| line.ends_with(':').then(|| line.len() - 1))?;
    if !line[..colon].ends_with(".d") {
        return None;
    }
    let mut deps = Vec::new();
    let mut cur = String::new();
    let mut chars = line[colon + 1..].chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars
                .peek()
                .is_some_and(|n| *n == ' ' || *n == '\\' || *n == '#') =>
            {
                cur.push(chars.next().unwrap())
            }
            ' ' => {
                if !cur.is_empty() {
                    deps.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        deps.push(cur);
    }
    Some(deps)
}

/// Paths from `cargo:rerun-if-changed=` / `cargo::rerun-if-changed=` lines.
pub fn rerun_if_changed(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|l| {
            l.strip_prefix("cargo::rerun-if-changed=")
                .or_else(|| l.strip_prefix("cargo:rerun-if-changed="))
        })
        .map(str::to_owned)
        .collect()
}

/// The local packages a build script's run directory may belong to:
/// `build/<name>-<hash>/output`, or `build/<name>/<hash>/output` in cargo's
/// newer build-dir layout.
fn build_script_package(ws: &Workspace, output: &Path) -> Vec<usize> {
    let dir = output.parent();
    let candidates = [
        dir.and_then(|d| d.file_name())
            .and_then(|n| n.to_str())
            .and_then(|n| n.rsplit_once('-'))
            .map(|(name, _)| name.to_owned()),
        dir.and_then(|d| d.parent())
            .and_then(|d| d.file_name())
            .and_then(|n| n.to_str())
            .map(str::to_owned),
    ];
    for name in candidates.into_iter().flatten() {
        let found: Vec<usize> = (0..ws.packages.len())
            .filter(|&i| ws.packages[i].dir.is_some() && ws.packages[i].name == name)
            .collect();
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rustc_depinfo() {
        let uplifted = "/t/debug/foo: crates/bar/src/a.rs crates/foo/src/lib.rs\n";
        assert_eq!(parse_depinfo(uplifted), None);
        let text = "/t/deps/foo-1.d: crates/foo/src/lib.rs crates/foo/My\\ File.txt /abs/x.rs\n\
                    \n\
                    crates/foo/src/lib.rs:\n\
                    # env-dep:CARGO_PKG_NAME=foo\n";
        assert_eq!(
            parse_depinfo(text).unwrap(),
            vec![
                "crates/foo/src/lib.rs",
                "crates/foo/My File.txt",
                "/abs/x.rs"
            ]
        );
    }

    #[test]
    fn reads_both_rerun_spellings() {
        let out =
            "cargo:rerun-if-changed=src\ncargo:warning=x\ncargo::rerun-if-changed=../../libs/cpp\n";
        assert_eq!(rerun_if_changed(out), vec!["src", "../../libs/cpp"]);
    }
}

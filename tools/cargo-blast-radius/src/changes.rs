//! What changed: files from git (or a list), turned into seed packages.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::impact::{Impact, Reason};
use crate::paths::relative_to;
use crate::workspace::{FileClass, Workspace};

/// Where the changed files come from.
pub enum Source {
    /// `git diff` between the merge base of `base` and `head`, or the working
    /// tree (untracked files included) when `head` is `None`.
    Git { base: String, head: Option<String> },
    /// Paths relative to the repository root (or the workspace root outside
    /// git), as a CI step listed them. `Cargo.lock` cannot be diffed then.
    List(Vec<String>),
}

pub struct Outcome {
    pub changed_files: Vec<String>,
    pub ignored_files: Vec<String>,
    pub impact: Impact,
}

pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("running git")?;
    if !out.status.success() {
        bail!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?)
}

pub fn repo_root(ws: &Workspace) -> PathBuf {
    git(&ws.root, &["rev-parse", "--show-toplevel"])
        .map(|s| PathBuf::from(s.trim()))
        .unwrap_or_else(|_| ws.root.clone())
}

fn nul_separated(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split('\0').filter(|p| !p.is_empty()).map(str::to_owned)
}

pub fn analyze(ws: &Workspace, source: Source) -> Result<Outcome> {
    let repo = repo_root(ws);
    let (files, revs) = match source {
        Source::Git { base, head } => {
            let head_rev = head.as_deref().unwrap_or("HEAD");
            let merge_base = git(&repo, &["merge-base", &base, head_rev])
                .with_context(|| {
                    format!(
                        "no merge base between `{base}` and `{head_rev}` (is the history fetched?)"
                    )
                })?
                .trim()
                .to_owned();
            let mut args = vec!["diff", "--name-only", "--no-renames", "-z", &merge_base];
            if let Some(h) = &head {
                args.push(h);
            }
            let mut files: Vec<String> = nul_separated(&git(&repo, &args)?).collect();
            if head.is_none() {
                files.extend(nul_separated(&git(
                    &repo,
                    &[
                        "ls-files",
                        "--others",
                        "--exclude-standard",
                        "--full-name",
                        "-z",
                    ],
                )?));
            }
            (files, Some((merge_base, head)))
        }
        Source::List(files) => (files, None),
    };

    let lockfile = relative_to(&ws.root.join("Cargo.lock"), &repo);
    let mut seeds = Vec::new();
    let mut ignored = Vec::new();
    let mut changed = Vec::new();
    for file in files {
        let rel = relative_to(&repo.join(&file), &ws.root);
        if file == lockfile {
            let Some((merge_base, head)) = &revs else {
                return Ok(everything(
                    ws,
                    file,
                    "Cargo.lock changed (no git base to diff it against)",
                ));
            };
            match lock_seeds(ws, &repo, &lockfile, merge_base, head.as_deref()) {
                Ok(s) => seeds.extend(s),
                Err(e) => {
                    return Ok(everything(
                        ws,
                        file,
                        &format!("Cargo.lock changed and could not be diffed: {e:#}"),
                    ))
                }
            }
            changed.push(file);
            continue;
        }
        match ws.classify(&rel) {
            FileClass::Ignored => ignored.push(file),
            FileClass::Global(why) => {
                return Ok(everything(ws, file.clone(), &format!("{why}: {file}")))
            }
            FileClass::Owned(owners) => {
                seeds.extend(owners.into_iter().map(|o| (o, Reason::File(file.clone()))));
                changed.push(file);
            }
        }
    }
    Ok(Outcome {
        changed_files: changed,
        ignored_files: ignored,
        impact: Impact::propagate(ws, seeds),
    })
}

fn everything(ws: &Workspace, file: String, why: &str) -> Outcome {
    Outcome {
        changed_files: vec![file],
        ignored_files: Vec::new(),
        impact: Impact::everything(ws, why),
    }
}

/// Packages whose `Cargo.lock` entry differs between the two revisions.
fn lock_seeds(
    ws: &Workspace,
    repo: &Path,
    lockfile: &str,
    merge_base: &str,
    head: Option<&str>,
) -> Result<Vec<(usize, Reason)>> {
    let old = git(repo, &["show", &format!("{merge_base}:{lockfile}")])?;
    let new = match head {
        Some(h) => git(repo, &["show", &format!("{h}:{lockfile}")])?,
        None => std::fs::read_to_string(repo.join(lockfile))?,
    };
    let mut seeds = Vec::new();
    for (name, version, source) in changed_lock_entries(&old, &new)? {
        let p = ws
            .find_locked(&name, &version, source.as_deref())
            .with_context(|| {
                format!("{name} {version} is in Cargo.lock but not in cargo metadata")
            })?;
        seeds.push((p, Reason::Lock));
    }
    Ok(seeds)
}

type LockKey = (String, String, Option<String>);

/// Entries added to, or changed in, `new`. Removed entries need no seed: what
/// depended on them now depends on something else, so its entry changed.
///
/// Dependencies are compared as (name, version): `Cargo.lock` spells one
/// `"name"` while a single version is locked and `"name version"` once there
/// are two, which would otherwise flag every dependent of a crate that merely
/// gained a second version.
pub fn changed_lock_entries(old: &str, new: &str) -> Result<Vec<LockKey>> {
    type Entries = BTreeMap<LockKey, (Option<String>, BTreeSet<(String, String)>)>;
    fn entries(text: &str) -> Result<Entries> {
        let doc: toml::Value = toml::from_str(text).context("parsing Cargo.lock")?;
        let packages: Vec<&toml::Value> = doc
            .get("package")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .collect();
        let field = |p: &toml::Value, k: &str| p.get(k).and_then(|v| v.as_str()).map(str::to_owned);
        let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for p in &packages {
            if let (Some(n), Some(v)) = (field(p, "name"), field(p, "version")) {
                versions.entry(n).or_default().push(v);
            }
        }
        let mut out = BTreeMap::new();
        for p in packages {
            let key = (
                field(p, "name").context("lock entry without a name")?,
                field(p, "version").context("lock entry without a version")?,
                field(p, "source"),
            );
            let deps = p
                .get("dependencies")
                .and_then(|d| d.as_array())
                .into_iter()
                .flatten()
                .filter_map(|d| d.as_str())
                .map(|d| {
                    let mut parts = d.split(' ');
                    let name = parts.next().unwrap_or_default().to_owned();
                    let version = parts.next().map(str::to_owned).unwrap_or_else(|| {
                        versions
                            .get(&name)
                            .and_then(|v| v.first())
                            .cloned()
                            .unwrap_or_default()
                    });
                    (name, version)
                })
                .collect();
            out.insert(key, (field(p, "checksum"), deps));
        }
        Ok(out)
    }
    let old = entries(old)?;
    Ok(entries(new)?
        .into_iter()
        .filter(|(k, v)| old.get(k) != Some(v))
        .map(|(k, _)| k)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_diff_reports_added_and_changed_entries() {
        let old = r#"
version = 4
[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b"]
[[package]]
name = "b"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"
[[package]]
name = "c"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "11"
"#;
        let new = r#"
version = 4
[[package]]
name = "a"
version = "0.1.0"
dependencies = ["b"]
[[package]]
name = "b"
version = "1.0.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "01"
[[package]]
name = "c"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "11"
"#;
        let changed: Vec<_> = changed_lock_entries(old, new)
            .unwrap()
            .into_iter()
            .map(|(n, v, _)| format!("{n} {v}"))
            .collect();
        // `a` still spells its dependency "b", but it now resolves to 1.0.1;
        // `c` is untouched.
        assert_eq!(changed, ["a 0.1.0", "b 1.0.1"]);
    }

    #[test]
    fn lock_diff_ignores_respelled_dependencies() {
        let entry = |name: &str, version: &str, deps: &str| {
            format!("[[package]]\nname = \"{name}\"\nversion = \"{version}\"\ndependencies = [{deps}]\n")
        };
        let old = [entry("a", "0.1.0", "\"c\""), entry("c", "1.0.0", "")].concat();
        let new = [
            entry("a", "0.1.0", "\"c 1.0.0\""),
            entry("b", "0.1.0", "\"c 2.0.0\""),
            entry("c", "1.0.0", ""),
            entry("c", "2.0.0", ""),
        ]
        .concat();
        let changed: Vec<_> = changed_lock_entries(&old, &new)
            .unwrap()
            .into_iter()
            .map(|(n, v, _)| format!("{n} {v}"))
            .collect();
        assert_eq!(changed, ["b 0.1.0", "c 2.0.0"]);
    }
}

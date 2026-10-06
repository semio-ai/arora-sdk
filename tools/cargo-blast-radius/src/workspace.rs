//! The workspace as `cargo metadata` describes it: packages, the dependency
//! graph between them (reversed, for propagation), and the configuration that
//! maps files to packages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde_json::Value;

use crate::paths::{normalize, relative_to};

/// The key under `[workspace.metadata]` and `[package.metadata]`.
pub const CONFIG_KEY: &str = "blast-radius";

/// Files at the workspace root that change how every package builds.
const GLOBAL_ROOT_FILES: &[&str] = &[
    "Cargo.toml",
    ".cargo/config.toml",
    ".cargo/config",
    "rust-toolchain.toml",
    "rust-toolchain",
];

pub struct Package {
    pub name: String,
    pub version: String,
    /// Where the package comes from, as `Cargo.lock` spells it; `None` for
    /// path packages.
    pub source: Option<String>,
    /// `None` for packages from a registry or git; their sources are not ours.
    pub dir: Option<String>,
    pub member: bool,
    pub default_member: bool,
    /// Extra inputs declared under `[package.metadata.blast-radius] inputs`,
    /// as patterns relative to the workspace root.
    pub inputs: Option<GlobSet>,
}

/// How one package depends on another.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    /// Normal or build dependency (artifact dependencies included): the
    /// dependent's own artifacts change.
    Build,
    /// Dev-dependency: only the dependent's tests, benches and examples change.
    Dev,
}

/// What a changed file means.
#[derive(Debug, PartialEq)]
pub enum FileClass {
    /// Affects no package.
    Ignored,
    /// Affects every package; the string says why.
    Global(&'static str),
    /// Affects these packages (and, through the graph, their dependents).
    Owned(Vec<usize>),
}

pub struct Workspace {
    pub root: PathBuf,
    pub target_dirs: Vec<PathBuf>,
    pub packages: Vec<Package>,
    /// For each package, the packages that depend on it and how.
    pub dependents: Vec<Vec<(usize, Edge)>>,
    /// Crate root source file → (package, whether the target is a lib/bin/build
    /// script whose output other packages consume).
    pub target_roots: HashMap<PathBuf, (usize, bool)>,
    /// Local packages, longest directory first, for ownership lookups.
    by_dir: Vec<usize>,
    ignore: GlobSet,
    global: GlobSet,
}

impl Workspace {
    pub fn load(manifest_path: Option<&Path>, metadata_args: &[String]) -> Result<Self> {
        let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        cmd.args(["metadata", "--format-version", "1"]);
        if let Some(path) = manifest_path {
            cmd.arg("--manifest-path").arg(path);
        }
        cmd.args(metadata_args);
        let output = cmd.output().context("running `cargo metadata`")?;
        if !output.status.success() {
            bail!(
                "`cargo metadata` failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let metadata: Value =
            serde_json::from_slice(&output.stdout).context("parsing `cargo metadata` output")?;
        Self::from_metadata(&metadata)
    }

    pub fn from_metadata(m: &Value) -> Result<Self> {
        let root = PathBuf::from(str_at(m, "workspace_root")?);
        let mut target_dirs = vec![PathBuf::from(str_at(m, "target_directory")?)];
        if let Some(build_dir) = m["build_directory"].as_str() {
            if !target_dirs.iter().any(|d| d == Path::new(build_dir)) {
                target_dirs.push(build_dir.into());
            }
        }

        let ids = |key: &str| -> Vec<&str> {
            m[key]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default()
        };
        let members = ids("workspace_members");
        // Older cargo omits the key: every member is then a default member.
        let defaults = if m.get("workspace_default_members").is_some() {
            ids("workspace_default_members")
        } else {
            members.clone()
        };

        let config = &m["metadata"][CONFIG_KEY];
        let ignore = globs(&root_patterns(config, "ignore")?)?;
        let global = globs(&root_patterns(config, "global")?)?;

        let mut packages = Vec::new();
        let mut by_id = HashMap::new();
        let mut target_roots = HashMap::new();
        for p in m["packages"]
            .as_array()
            .context("no packages in metadata")?
        {
            let id = str_at(p, "id")?;
            let idx = packages.len();
            let local = p["source"].is_null();
            let dir = if local {
                let manifest = PathBuf::from(str_at(p, "manifest_path")?);
                let dir = manifest.parent().context("manifest without a directory")?;
                Some(relative_to(dir, &root))
            } else {
                None
            };
            let inputs = match (&dir, p["metadata"][CONFIG_KEY]["inputs"].as_array()) {
                (Some(dir), Some(patterns)) => {
                    let patterns = patterns
                        .iter()
                        .map(|v| {
                            let pat = v.as_str().context("`inputs` entries must be strings")?;
                            Ok(normalize(&format!("{dir}/{pat}")))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Some(globs(&patterns)?)
                }
                _ => None,
            };
            if local {
                for t in p["targets"].as_array().into_iter().flatten() {
                    let consumed = t["kind"].as_array().into_iter().flatten().any(|k| {
                        let k = k.as_str().unwrap_or("");
                        k.contains("lib") || k == "proc-macro" || k == "bin" || k == "custom-build"
                    });
                    target_roots.insert(PathBuf::from(str_at(t, "src_path")?), (idx, consumed));
                }
            }
            packages.push(Package {
                name: str_at(p, "name")?.to_owned(),
                version: str_at(p, "version")?.to_owned(),
                source: p["source"].as_str().map(str::to_owned),
                dir,
                member: members.contains(&id),
                default_member: defaults.contains(&id),
                inputs,
            });
            by_id.insert(id.to_owned(), idx);
        }

        let mut dependents = vec![Vec::new(); packages.len()];
        let nodes = m["resolve"]["nodes"]
            .as_array()
            .context("no resolve graph in metadata (was --no-deps passed?)")?;
        for node in nodes {
            let from = by_id[str_at(node, "id")?];
            for dep in node["deps"].as_array().into_iter().flatten() {
                let to = *by_id
                    .get(str_at(dep, "pkg")?)
                    .context("dependency missing from packages")?;
                let mut edge = None;
                for kind in dep["dep_kinds"].as_array().into_iter().flatten() {
                    match kind["kind"].as_str() {
                        Some("dev") => edge = edge.or(Some(Edge::Dev)),
                        _ => edge = Some(Edge::Build),
                    }
                }
                // Cargo older than 1.41 has no dep_kinds: assume the worst.
                dependents[to].push((from, edge.unwrap_or(Edge::Build)));
            }
        }

        let mut by_dir: Vec<usize> = (0..packages.len())
            .filter(|&i| packages[i].dir.is_some())
            .collect();
        by_dir.sort_by_key(|&i| std::cmp::Reverse(packages[i].dir.as_ref().unwrap().len()));

        Ok(Self {
            root,
            target_dirs,
            packages,
            dependents,
            target_roots,
            by_dir,
            ignore,
            global,
        })
    }

    /// The package whose directory contains `rel` (a path relative to the
    /// workspace root), the innermost one when packages nest.
    pub fn owner(&self, rel: &str) -> Option<usize> {
        self.by_dir.iter().copied().find(|&i| {
            let dir = self.packages[i].dir.as_deref().unwrap();
            dir.is_empty() || rel == dir || rel.starts_with(&format!("{dir}/"))
        })
    }

    /// What a change to `rel` (relative to the workspace root) touches.
    ///
    /// Declared `inputs` win over `ignore`; `ignore` wins over directory
    /// ownership; anything left over — a file in no package — is global.
    pub fn classify(&self, rel: &str) -> FileClass {
        if GLOBAL_ROOT_FILES.contains(&rel) || self.global.is_match(rel) {
            return FileClass::Global("workspace-wide file");
        }
        let mut owners: Vec<usize> = self
            .packages
            .iter()
            .enumerate()
            .filter(|(_, p)| p.inputs.as_ref().is_some_and(|g| g.is_match(rel)))
            .map(|(i, _)| i)
            .collect();
        if owners.is_empty() && self.ignore.is_match(rel) {
            return FileClass::Ignored;
        }
        if let Some(o) = self.owner(rel) {
            if !owners.contains(&o) {
                owners.push(o);
            }
        }
        if owners.is_empty() {
            FileClass::Global("file outside every package")
        } else {
            FileClass::Owned(owners)
        }
    }

    /// The package a `Cargo.lock` entry stands for.
    pub fn find_locked(&self, name: &str, version: &str, source: Option<&str>) -> Option<usize> {
        self.packages
            .iter()
            .position(|p| p.name == name && p.version == version && p.source.as_deref() == source)
    }
}

fn str_at<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .with_context(|| format!("`{key}` missing from cargo metadata"))
}

fn root_patterns(config: &Value, key: &str) -> Result<Vec<String>> {
    config[key]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            let s = v
                .as_str()
                .with_context(|| format!("`{CONFIG_KEY}.{key}` entries must be strings"))?;
            Ok(normalize(s))
        })
        .collect()
}

/// A glob set where `*` stays within a path component and `**` crosses them.
/// A pattern also matches everything below it, so a directory name is enough.
pub fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut set = GlobSetBuilder::new();
    for pat in patterns {
        for p in [pat.clone(), format!("{}/**", pat.trim_end_matches('/'))] {
            set.add(
                GlobBuilder::new(&p)
                    .literal_separator(true)
                    .build()
                    .with_context(|| format!("invalid pattern `{pat}`"))?,
            );
        }
    }
    Ok(set.build()?)
}

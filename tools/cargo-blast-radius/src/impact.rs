//! Propagating a change through the reversed dependency graph.

use std::collections::VecDeque;

use crate::workspace::{Edge, Workspace};

/// Why a package is affected — the first cause found, which is enough to
/// explain it.
#[derive(Clone, Debug, PartialEq)]
pub enum Reason {
    /// A file the package owns or declares as an input changed.
    File(String),
    /// Its entry in `Cargo.lock` changed (version, checksum or dependencies).
    Lock,
    /// A (normal, build or artifact) dependency is affected.
    Dep(usize),
    /// A dev-dependency is affected: only tests, benches and examples.
    DevDep(usize),
    /// A workspace-wide change.
    Global(String),
}

pub struct Impact {
    /// Packages whose own artifacts (lib, bins, build script) may change.
    pub build: Vec<Option<Reason>>,
    /// Packages whose tests may change: `build` plus direct dev-dependents.
    pub test: Vec<Option<Reason>>,
}

impl Impact {
    pub fn everything(ws: &Workspace, why: &str) -> Self {
        let all = vec![Some(Reason::Global(why.to_owned())); ws.packages.len()];
        Self {
            build: all.clone(),
            test: all,
        }
    }

    pub fn propagate(ws: &Workspace, seeds: Vec<(usize, Reason)>) -> Self {
        let n = ws.packages.len();
        let mut build: Vec<Option<Reason>> = vec![None; n];
        let mut queue = VecDeque::new();
        for (p, why) in seeds {
            if build[p].is_none() {
                build[p] = Some(why);
                queue.push_back(p);
            }
        }
        while let Some(p) = queue.pop_front() {
            for &(dependent, edge) in &ws.dependents[p] {
                if edge == Edge::Build && build[dependent].is_none() {
                    build[dependent] = Some(Reason::Dep(p));
                    queue.push_back(dependent);
                }
            }
        }
        // Dev-dependencies do not propagate further: a package's tests are
        // nobody's dependency.
        let mut test = build.clone();
        for (p, reached) in build.iter().enumerate() {
            if reached.is_some() {
                for &(dependent, edge) in &ws.dependents[p] {
                    if edge == Edge::Dev && test[dependent].is_none() {
                        test[dependent] = Some(Reason::DevDep(p));
                    }
                }
            }
        }
        Self { build, test }
    }

    /// Explains why `p` is in the test set, as a chain back to the change.
    pub fn explain(&self, ws: &Workspace, p: usize) -> String {
        let mut out = String::new();
        let mut cur = p;
        let mut reason = self.test[p].clone();
        // Bounded: each hop follows a strictly earlier BFS discovery.
        for _ in 0..ws.packages.len() + 1 {
            match reason {
                None => break,
                Some(Reason::File(f)) => {
                    out.push_str(&format!("changed {f}"));
                    break;
                }
                Some(Reason::Lock) => {
                    out.push_str(&format!(
                        "Cargo.lock entry {} {} changed",
                        ws.packages[cur].name, ws.packages[cur].version
                    ));
                    break;
                }
                Some(Reason::Global(why)) => {
                    out.push_str(&why);
                    break;
                }
                Some(Reason::Dep(d)) | Some(Reason::DevDep(d)) => {
                    let dev = matches!(reason, Some(Reason::DevDep(_)));
                    out.push_str(&format!(
                        "{}{} ← ",
                        if dev { "dev: " } else { "" },
                        ws.packages[d].name
                    ));
                    cur = d;
                    reason = self.build[d].clone();
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::workspace::FileClass;

    /// a ← b (normal) ← c (dev); a ← t (artifact build-dep); `ext` is a
    /// registry crate a depends on; d declares `libs/` as an input.
    fn workspace() -> Workspace {
        let pkg = |name: &str, dir: Option<&str>, meta: serde_json::Value| {
            json!({
                "id": format!("{name}-id"), "name": name, "version": "0.1.0",
                "source": if dir.is_some() { serde_json::Value::Null } else { json!("registry+x") },
                "manifest_path": format!("/r/{}/Cargo.toml", dir.unwrap_or("reg")),
                "targets": [{ "kind": ["lib"], "src_path": format!("/r/{}/src/lib.rs", dir.unwrap_or("reg")) }],
                "metadata": meta,
            })
        };
        let dep = |name: &str, kind: serde_json::Value| json!({ "pkg": format!("{name}-id"), "dep_kinds": [{ "kind": kind }] });
        let node = |name: &str, deps: Vec<serde_json::Value>| json!({ "id": format!("{name}-id"), "deps": deps });
        let m = json!({
            "workspace_root": "/r",
            "target_directory": "/r/target",
            "workspace_members": ["a-id", "b-id", "c-id", "d-id", "t-id"],
            "workspace_default_members": ["a-id", "b-id", "c-id", "d-id"],
            "metadata": { "blast-radius": { "ignore": ["docs", "**/*.md"] } },
            "packages": [
                pkg("a", Some("crates/a"), json!(null)),
                pkg("b", Some("crates/b"), json!(null)),
                pkg("c", Some("crates/c"), json!(null)),
                pkg("d", Some("modules/d"), json!({ "blast-radius": { "inputs": ["../../libs"] } })),
                pkg("t", Some("tests"), json!(null)),
                pkg("ext", None, json!(null)),
            ],
            "resolve": { "nodes": [
                node("a", vec![dep("ext", json!(null))]),
                node("b", vec![dep("a", json!(null))]),
                node("c", vec![dep("b", json!("dev"))]),
                node("d", vec![]),
                node("t", vec![dep("a", json!("build"))]),
                node("ext", vec![]),
            ]},
        });
        Workspace::from_metadata(&m).unwrap()
    }

    fn names(ws: &Workspace, set: &[Option<Reason>]) -> Vec<String> {
        (0..set.len())
            .filter(|&i| set[i].is_some())
            .map(|i| ws.packages[i].name.clone())
            .collect()
    }

    #[test]
    fn classifies_files() {
        let ws = workspace();
        let idx = |n: &str| ws.packages.iter().position(|p| p.name == n).unwrap();
        assert_eq!(
            ws.classify("crates/a/src/lib.rs"),
            FileClass::Owned(vec![idx("a")])
        );
        assert_eq!(ws.classify("crates/a/readme.md"), FileClass::Ignored);
        assert_eq!(ws.classify("docs/x/y.txt"), FileClass::Ignored);
        assert_eq!(
            ws.classify("libs/cpp/x.hpp"),
            FileClass::Owned(vec![idx("d")])
        );
        assert!(matches!(ws.classify("Cargo.toml"), FileClass::Global(_)));
        assert!(matches!(ws.classify("ci/run.sh"), FileClass::Global(_)));
        // A sibling directory sharing a prefix is not inside the package.
        assert!(matches!(
            ws.classify("crates/ab/x.rs"),
            FileClass::Global(_)
        ));
    }

    #[test]
    fn propagates_build_edges_but_stops_at_dev_edges() {
        let ws = workspace();
        let ext = ws.packages.iter().position(|p| p.name == "ext").unwrap();
        let impact = Impact::propagate(&ws, vec![(ext, Reason::Lock)]);
        assert_eq!(names(&ws, &impact.build), ["a", "b", "t", "ext"]);
        assert_eq!(names(&ws, &impact.test), ["a", "b", "c", "t", "ext"]);
        let c = ws.packages.iter().position(|p| p.name == "c").unwrap();
        assert_eq!(
            impact.explain(&ws, c),
            "dev: b ← a ← ext ← Cargo.lock entry ext 0.1.0 changed"
        );

        let b = ws.packages.iter().position(|p| p.name == "b").unwrap();
        let impact = Impact::propagate(&ws, vec![(b, Reason::File("crates/b/src/lib.rs".into()))]);
        assert_eq!(names(&ws, &impact.build), ["b"]);
        assert_eq!(names(&ws, &impact.test), ["b", "c"]);
    }
}

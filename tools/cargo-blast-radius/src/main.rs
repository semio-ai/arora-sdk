//! `cargo blast-radius`: which packages of a Cargo workspace a change can
//! reach, so CI builds and tests only those — and `cargo blast-radius verify`,
//! which checks that answer against the inputs cargo itself tracked in a build.

mod changes;
mod impact;
mod paths;
mod verify;
mod workspace;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::changes::Source;
use crate::impact::Impact;
use crate::workspace::{Workspace, CONFIG_KEY};

const USAGE: &str = "\
cargo blast-radius — select the workspace packages a change can affect

USAGE:
    cargo blast-radius [OPTIONS] (--base <REV> | --changed-files <FILE> | --all)
    cargo blast-radius verify [OPTIONS]

CHANGES:
    --base <REV>             Diff from the merge base of <REV> and --head
    --head <REV>             Default: the working tree, untracked files included
    --changed-files <FILE>   Paths relative to the repository root, one per line
                             ('-' for stdin), instead of asking git
    --all                    Select everything (e.g. on the main branch)

OPTIONS:
    --manifest-path <PATH>   Workspace manifest (default: found from the cwd)
    --metadata-arg <ARG>     Passed to `cargo metadata`; repeatable. Replaces the
                             default `--all-features`, which keeps every optional
                             dependency in the graph
    --scope <default|all>    Members `test-args` selects from: the default
                             members (what a bare `cargo test` runs; default)
                             or every member
    --format <text|json|github>
                             github: `key=value` lines for $GITHUB_OUTPUT on
                             stdout, the readable report on stderr

verify checks that every file cargo recorded as an input of a workspace target
(rustc dep-info, build-script rerun-if-changed) would select that target when
changed. Run it after a build; it exits 1 on a gap and says how to close it.

CONFIGURATION (Cargo.toml):
    [workspace.metadata.blast-radius]
    ignore = [\"docs/**\", \"**/*.md\"]   # changes here select nothing
    global = [\"ci/**\"]                  # changes here select everything

    [package.metadata.blast-radius]
    inputs = [\"../../libs/cpp\"]         # files outside the package it reads

A changed file selects the packages that declare it as an input, else nothing
if ignored, else the package whose directory holds it, else everything.
";

struct Args {
    verify: bool,
    base: Option<String>,
    head: Option<String>,
    changed_files: Option<String>,
    all: bool,
    manifest_path: Option<PathBuf>,
    metadata_args: Vec<String>,
    scope_all: bool,
    format: String,
}

fn parse_args() -> Result<Args> {
    let mut raw: Vec<String> = std::env::args().skip(1).collect();
    // Invoked as `cargo blast-radius`, cargo passes the subcommand name first.
    if raw.first().map(String::as_str) == Some("blast-radius") {
        raw.remove(0);
    }
    let mut args = Args {
        verify: false,
        base: None,
        head: None,
        changed_files: None,
        all: false,
        manifest_path: None,
        metadata_args: Vec::new(),
        scope_all: false,
        format: "text".into(),
    };
    let mut metadata_args = None::<Vec<String>>;
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        let mut value = || -> Result<String> {
            inline
                .clone()
                .or_else(|| it.next())
                .with_context(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "verify" => args.verify = true,
            "--base" => args.base = Some(value()?),
            "--head" => args.head = Some(value()?),
            "--changed-files" => args.changed_files = Some(value()?),
            "--all" => args.all = true,
            "--manifest-path" => args.manifest_path = Some(value()?.into()),
            "--metadata-arg" => metadata_args.get_or_insert_with(Vec::new).push(value()?),
            "--scope" => {
                args.scope_all = match value()?.as_str() {
                    "default" => false,
                    "all" => true,
                    s => bail!("unknown scope `{s}` (default|all)"),
                }
            }
            "--format" => args.format = value()?,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            _ => bail!("unexpected argument `{arg}`\n\n{USAGE}"),
        }
    }
    args.metadata_args = metadata_args.unwrap_or_else(|| vec!["--all-features".into()]);
    if !matches!(args.format.as_str(), "text" | "json" | "github") {
        bail!("unknown format `{}` (text|json|github)", args.format);
    }
    Ok(args)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode> {
    let args = parse_args()?;
    let ws = Workspace::load(args.manifest_path.as_deref(), &args.metadata_args)?;
    if args.verify {
        return run_verify(&ws);
    }

    let outcome = if args.all {
        changes::Outcome {
            changed_files: Vec::new(),
            ignored_files: Vec::new(),
            impact: Impact::everything(&ws, "--all"),
        }
    } else if let Some(list) = &args.changed_files {
        let text = if list == "-" {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        } else {
            std::fs::read_to_string(list).with_context(|| format!("reading {list}"))?
        };
        let files = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        changes::analyze(&ws, Source::List(files))?
    } else if let Some(base) = &args.base {
        changes::analyze(
            &ws,
            Source::Git {
                base: base.clone(),
                head: args.head.clone(),
            },
        )?
    } else {
        bail!("say what changed: --base <REV>, --changed-files <FILE> or --all\n\n{USAGE}");
    };

    report(&ws, &outcome, &args);
    Ok(ExitCode::SUCCESS)
}

fn report(ws: &Workspace, outcome: &changes::Outcome, args: &Args) {
    let impact = &outcome.impact;
    let members: Vec<usize> = (0..ws.packages.len())
        .filter(|&i| ws.packages[i].member)
        .collect();
    let names = |set: &[Option<impact::Reason>]| -> Vec<&str> {
        let mut v: Vec<&str> = members
            .iter()
            .filter(|&&i| set[i].is_some())
            .map(|&i| ws.packages[i].name.as_str())
            .collect();
        v.sort_unstable();
        v
    };
    let build = names(&impact.build);
    let test = names(&impact.test);
    let all = members.iter().all(|&i| impact.test[i].is_some());
    let in_scope: Vec<&str> = {
        let mut v: Vec<&str> = members
            .iter()
            .filter(|&&i| {
                impact.test[i].is_some() && (args.scope_all || ws.packages[i].default_member)
            })
            .map(|&i| ws.packages[i].name.as_str())
            .collect();
        v.sort_unstable();
        v
    };
    let test_args = in_scope
        .iter()
        .map(|n| format!("-p {n}"))
        .collect::<Vec<_>>()
        .join(" ");

    let mut text = String::new();
    text.push_str(&format!(
        "blast radius: {} changed file(s) ({} ignored) → {} of {} members to test, {} to build\n",
        outcome.changed_files.len(),
        outcome.ignored_files.len(),
        test.len(),
        members.len(),
        build.len()
    ));
    let mut rows: Vec<(String, String)> = members
        .iter()
        .filter(|&&i| impact.test[i].is_some())
        .map(|&i| {
            let what = if impact.build[i].is_some() {
                "build+test"
            } else {
                "test"
            };
            (
                format!("  {:<32} {:<10}", ws.packages[i].name, what),
                impact.explain(ws, i),
            )
        })
        .collect();
    rows.sort();
    if all && !rows.is_empty() {
        text.push_str(&format!("  everything: {}\n", rows[0].1));
    } else {
        for (row, why) in rows {
            text.push_str(&format!("{row} {why}\n"));
        }
    }

    let summary = json!({
        "all": all,
        "build": build,
        "test": test,
        "test-args": test_args,
        "changed-files": outcome.changed_files,
        "ignored-files": outcome.ignored_files,
    });
    match args.format.as_str() {
        "json" => {
            eprint!("{text}");
            println!("{}", serde_json::to_string_pretty(&summary).unwrap());
        }
        "github" => {
            eprint!("{text}");
            println!("all={all}");
            println!("any={}", !in_scope.is_empty());
            println!("build={}", json!(build));
            println!("test={}", json!(test));
            println!("test-args={test_args}");
        }
        _ => print!("{text}"),
    }
}

fn run_verify(ws: &Workspace) -> Result<ExitCode> {
    let repo = changes::repo_root(ws);
    let report = verify::verify(ws, &repo)?;
    println!(
        "verify: {} dep-info file(s) and {} build-script output(s) under {}; {} input(s) checked",
        report.depinfo_files,
        report.build_script_outputs,
        ws.target_dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        report.inputs_checked
    );
    if report.depinfo_files == 0 {
        bail!("no dep-info for any workspace target: build first (e.g. `cargo test --no-run`)");
    }
    if report.violations.is_empty() {
        println!("verify: every tracked input selects the target that reads it");
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "verify: {} input(s) would not select the target that reads them:",
        report.violations.len()
    );
    let mut by_package: std::collections::BTreeMap<usize, Vec<&verify::Violation>> =
        Default::default();
    for v in &report.violations {
        by_package.entry(v.package).or_default().push(v);
    }
    for (p, vs) in by_package {
        let pkg = &ws.packages[p];
        println!("\n  {} reads:", pkg.name);
        for v in &vs {
            println!("    {} — {}", v.file, v.why);
        }
        let dir = ws.root.join(pkg.dir.as_deref().unwrap_or(""));
        let rel: Vec<String> = vs
            .iter()
            .map(|v| format!("\"{}\"", paths::relative_to(&ws.root.join(&v.file), &dir)))
            .collect();
        println!(
            "  fix: depend on the owner, or add to {}:\n    [package.metadata.{CONFIG_KEY}]\n    inputs = [{}]",
            paths::relative_to(&dir.join("Cargo.toml"), &ws.root),
            rel.join(", ")
        );
    }
    Ok(ExitCode::from(1))
}

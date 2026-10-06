# cargo-blast-radius

Which packages of a Cargo workspace a change can affect, so CI builds and
tests only those. It also checks that answer against the inputs cargo itself
recorded during the build.

It works on any Cargo workspace. It reads nothing but `cargo metadata`, git and
the target directory, and has no dependency on the surrounding repository. It
lives here until it gets a repository of its own.

## Two halves

**Selection** (`cargo blast-radius --base origin/main`). This runs before the
build, from the dependency graph alone:

1. **Changed files** come from `git diff` against the merge base. `--head`
   compares against another revision instead of the working tree (untracked
   files included).
2. **Each file maps to packages:**
   - Packages whose `[package.metadata.blast-radius] inputs` list the file come
     first.
   - Otherwise, a file matching `[workspace.metadata.blast-radius] ignore`
     selects nothing.
   - Otherwise, the file belongs to the innermost package whose directory holds
     it.
   - Otherwise, the file is in no package, and **everything** is selected.
   - The workspace `Cargo.toml`, `.cargo/config.toml` and `rust-toolchain*` are
     always global.
3. **`Cargo.lock` is diffed entry by entry.** Every external crate whose
   version, checksum or resolved dependencies moved becomes a seed, so a
   lockfile bump selects only what reaches the bumped crates.
4. **Seeds spread through the reversed `cargo metadata` resolve graph**,
   resolved with `--all-features` so optional dependencies count. Normal, build
   and artifact (`-Z bindeps`) edges spread transitively and give the **build**
   set. Packages that dev-depend on a member of the build set join the
   **test** set, and the spread stops there: nothing depends on a package's
   tests.

**Verification** (`cargo blast-radius verify`). This runs after the build. For
every compiled workspace target, cargo keeps two records:

- rustc's dep-info (`deps/*.d`): every source file, every `include_str!` /
  `include_bytes!` file, and every proc-macro tracked path that went in.
- each build script's `rerun-if-changed` paths.

These are exactly the inputs cargo uses to decide what to rebuild. `verify`
checks that a change to each of them would select the target that read it.
When one wouldn't, it names the file and the `inputs` entry that closes the
gap, and exits 1. Running it in CI keeps the configuration honest: a new
hidden dependency fails the build that introduces it.

The guarantee: if `verify` passes on a full build, then for every change, each
target the selection skips is one that cargo's own fingerprinting would not
rebuild either, because none of its tracked inputs changed.

## Limits

These are the cases where "provably" stops:

- **Untracked reads.** A build script that reads a file without printing
  `rerun-if-changed`, or a test that opens a fixture at run time, leaves no
  record for cargo or for `verify`. Files in no package are global by default
  for this reason. Keep `ignore` to files nothing reads, and print
  `rerun-if-changed` for what a build script reads (cargo needs it for
  incremental builds anyway).
- **Feature unification.** `cargo test -p a -p b` unifies features over `a` and
  `b` only, not over the whole workspace. A failure that needs a feature some
  unselected package turns on can slip through. Running everything on the main
  branch (`--all`) catches it after the merge.
- **Graph precision.** The graph is the union over all features and platforms,
  so a change can select a package that never compiles the changed crate in
  any configuration CI actually runs. This errs on the safe side.

## Configuration

```toml
# Workspace Cargo.toml
[workspace.metadata.blast-radius]
ignore = ["docs/**", "**/*.md", "LICENSE"]  # selects nothing (verify checks no target reads them)
global = ["ci/**"]                          # selects everything

# A member's Cargo.toml: files outside its directory that it reads
[package.metadata.blast-radius]
inputs = ["../../libs/cpp", "../../crates/engine/wit"]
```

Patterns are globs: `*` stays within a path component, `**` crosses
components, and a bare directory matches everything below it. Workspace
patterns are relative to the workspace root; `inputs` are relative to the
package.

## CI

```sh
cargo install --git <this repository> cargo-blast-radius   # or build it from a path
cargo blast-radius --base "origin/$BASE_REF" --format github >> "$GITHUB_OUTPUT"
```

`--format github` writes these outputs (the readable report goes to stderr):

| output      | value |
|-------------|-------|
| `all`       | `true` when a global file changed (or `--all`) |
| `any`       | `true` when some member in scope needs testing |
| `build`     | JSON array of members whose artifacts may change |
| `test`      | JSON array of members whose tests may change (`build` plus dev-dependents) |
| `test-args` | `-p a -p b …` over `test` ∩ scope (default members; `--scope all` for every member) |

Gate steps on them:

```yaml
- run: cargo test --release ${{ steps.affected.outputs.test-args }}
  if: steps.affected.outputs.any == 'true'
- run: cargo blast-radius verify
  if: steps.affected.outputs.any == 'true'
- run: wasm-pack test --headless --firefox crates/engine
  if: contains(fromJSON(steps.affected.outputs.test), 'engine')
```

Never run `cargo test ${{ …test-args }}` without the `any` guard: with an empty
list it tests the whole workspace.

Run with `--all` on the main branch, so that what a selection could miss
(feature unification, untracked reads) still gets caught after the merge. The
checkout needs enough history for the merge base (`fetch-depth: 0`).

`--changed-files <file|->` takes the list from another tool instead of git.
`Cargo.lock` then cannot be diffed, and any change to it selects everything.
`--format json` prints the same data as one object.

## Prior art

All three tools below were run on this repository (October 2026). The cases
were edits to a crate's source, to `arora-engine/wit/arora-module.wit`, to
`libs/cpp`, to `modules/test-cpp/records`, to `.cargo/config.toml` and to a
doc page.

- [cargo-delta](https://github.com/tekian/cargo-delta) maps files to crates by
  parsing the source (`mod`, `#[path]`, `include_str!`) and diffs `Cargo.lock`
  per package. It got source edits right, but treats a file it cannot attribute
  as affecting nothing. The WIT, `libs/cpp`, records and `.cargo/config.toml`
  edits all selected nothing. `delta.toml` trip wires can fix each case, but
  nothing says which ones are missing.
- [cargo-rail](https://github.com/loadingalias/cargo-rail) checks against
  cargo's records too. It runs a build under a rustc wrapper, records
  dep-info and `rerun-if-changed`, and widens scope wherever that record is
  incomplete. It is a larger engine (compiler cache, releases, split/sync),
  pre-1.0, and needs rustc ≥ 1.98.1, newer than this repository's pinned
  nightly. On this workspace its record stays incomplete (cross-target bindeps
  invocations, directory reads by build scripts, members `cargo test` does not
  build), so any file outside a crate, docs included, selects everything.
- [determinator](https://github.com/guppy-rs/guppy/tree/main/tools/determinator)
  (guppy) is a feature-aware library with path rules. It has no CLI, and its
  documentation says `rerun-if-changed` inputs must be repeated as rules by
  hand.

Coverage-based selectors ([cargo-difftests](https://github.com/dnbln/cargo-difftests),
[cargo-affected](https://github.com/max-sixty/cargo-affected)) pick individual
tests instead of packages.

# Implementation plan: extracting Arora modules (BT pilot)

Companion to [`proposal-split-arora-repos.md`](proposal-split-arora-repos.md).
Read the proposal first — this file assumes the architectural target
(§2, §5) and the recipe (§2.3) are already understood.

This is the executable plan. Each task is sized to be one PR. Each PR
has a definition of done (DoD); do not move on until it passes.

Assume the agent has shell access in
`/Users/victor.paleologue/Code/Semio/` with all four repos checked out
side by side, and `gh` at `/opt/homebrew/bin/gh`.

If a step's DoD does not hold, **STOP and report back**. Do not paper
over a red CI with `--no-verify` or by skipping tests.

---

## Conventions

- Branch naming: `feat/extract-<short-name>` in each affected repo.
- Commit style: follow the engine's existing convention
  (`<type>(<scope>): <subject>`).
- Never force-push to `main`/`master`. Never delete branches the user
  has not authorized.
- Always run CI before merging by opening a draft PR; do not merge if
  CI is red.
- All cross-repo git deps pin to a tag or commit, never to a branch
  (proposal §5.5).

## Checkpoints where the agent must stop and ask

- After **PR 1** (before publishing to crates.io): confirm the version
  bump with Victor.
- After **PR 4** (the test refactor): report what was easy vs hard.
  This determines whether PR 5+ proceed or whether the recipe needs
  revision (proposal §7 risk 5).
- Before **PR 5 step 7** and **PR 9 step 3**: needs `admin:org` scope
  refresh — Victor's hands on keyboard.
- Before **PR 8** (repo rename): confirm with Victor; renames affect
  external bookmarks.

---

## Tooling and environment

- `gh` is at `/opt/homebrew/bin/gh` (v2.92.0).
- Token scopes today: `gist`, `read:org`, `repo`. `repo` is enough to
  create repos in `semio-ai`. Org-level rulesets and org-level secrets
  need `admin:org` — refresh on demand via:
  ```sh
  gh auth refresh -s admin:org
  ```
- Victor is `semio-ai` admin (verified via
  `gh api orgs/semio-ai/memberships/victorpaleologue`).
- `semio-ai/engine` has been renamed to `semio-ai/arora-engine`; its
  default branch is `main`. Branch protection / rulesets are the
  baseline set in PR 9 — re-audit before changing via
  `gh api repos/semio-ai/arora-engine/branches/main/protection` and
  `gh api repos/semio-ai/arora-engine/rulesets`.

---

## PR 1 — Add `CallBridge`/`Callable`/`CallableId`/`CallError` to `arora-types`

**Repo:** `arora-types`. **Branch:** `feat/call-bridge`.

Context: these four symbols are pure interface but live in `arora`
(the engine crate) today, in `arora-engine/crates/arora/src/call.rs`.
They reference only `Call`, `CallResult`, `Value`, `Uuid` — all
already in `arora-types`. The block to move verbatim:

```rust
pub trait Callable {
    fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, CallError>;
}

pub trait CallBridge {
    fn arora_call(&mut self, module: &Uuid, call: Call) -> Result<CallResult, CallError>;
    fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId;
    fn arora_unregister_callable(&mut self, callable_id: &CallableId);
    fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError>;
}

// plus CallError enum and the CallableId struct + impls
```

Do **not** move: `serialize_to_arg`, `CallableRegistry`,
`CALLABLE_ID_TYPE_ID`, `CALLABLE_ID_ID_FIELD_ID`, the
`From<DispatchError> for CallError` impl — these are engine-internal.

Steps:

1. Create `src/call/bridge.rs` with the four traits + `CallError` +
   `CallableId`.
2. Re-export from `src/call/mod.rs`:
   `pub mod bridge; pub use bridge::*;`.
3. Bump `Cargo.toml` to `version = "1.2.0"`. One-line note in
   `readme.md`.
4. Add tests under `tests/` that exercise an in-test `MockCallBridge`.
   The point is to prove the traits are usable without an engine.
5. Open PR, get CI green, merge, tag `v1.2.0`, **stop here**, ask
   Victor before `cargo publish`.

**DoD:** `arora-types v1.2.0` is on crates.io. `cargo add arora-types`
in a scratch project gives access to `arora_types::call::CallBridge`.

---

## PR 2 — Engine re-exports the traits from `arora-types`

**Repo:** `engine`. **Branch:** `feat/call-bridge-from-types`.
Prerequisite: PR 1 published.

1. In `crates/arora/Cargo.toml`: bump `arora-types` to `"1.2"`.
2. In `crates/arora/src/call.rs`: delete the local definitions of
   `CallBridge`, `Callable`, `CallableId`, `CallError`. Replace with:
   ```rust
   pub use arora_types::call::{CallBridge, Callable, CallableId, CallError};
   ```
   Keep `CallableRegistry`, `serialize_to_arg`,
   `CALLABLE_ID_TYPE_ID`, `CALLABLE_ID_ID_FIELD_ID`, and the
   `From<DispatchError> for CallError` impl in place.
3. Run:
   ```sh
   cargo build --workspace --release
   cargo test --workspace --release
   cargo build -p arora --target wasm32-unknown-unknown --no-default-features --release
   ```
   Fix import paths if anything broke.

**DoD:** Engine CI green on this branch. No behavioral change. The
diff is small (≤100 lines) and is mostly deletions.

---

## PR 3 — Standalone BT repo: drop engine runtime deps

**Repo:** `arora-behavior-tree`. **Branch:** `feat/decouple-engine`.
Prerequisite: PR 1 merged and published.

1. In `arora-behavior-tree/arora-behavior-tree/Cargo.toml`:
   - Delete `arora = { git = ... }` and `arora-buffers = { git = ... }`
     from `[dependencies]`.
   - Bump `arora-types` to `"1.2"`.
   - Update `semio-client` and `semio-record` `branch` from `"update"`
     to `"main"` (matches the engine).
2. In `[dev-dependencies]`: keep `arora-module-core` and
   `arora-registry` git deps on the engine repo *for now* — they
   move in PR 5. Verify they still resolve.
3. Fix all `use arora::...` / `use arora_buffers::...` imports in
   `src/` to use `arora_types::call::*` instead. Run `cargo build`
   until clean.
4. Run `cargo test`. The tests that spin up a real engine via
   `arora-module-core` + `arora-registry` should still pass — they
   are dev-deps, that is fine for this PR. Test decoupling (mocking
   `CallBridge`) is **PR 4**, not this one.
5. Mirror any commits from the engine's
   `crates/arora-behavior-tree*` that are missing standalone-side.
   Use `git log -- crates/arora-behavior-tree` in the engine repo as
   the reference set. As of writing the missing ones include:
   `843f6d4`, `9ddd832`, `bf20ec7`, `1904007`, `bcefec7`, `1d41628`,
   `bf9d320`, `44e7418`. Cherry-pick where possible; rewrite by hand
   where paths differ.

**DoD:** `arora-behavior-tree` repo CI green. `Cargo.toml` has no
reference to the engine in `[dependencies]`. Engine reference remains
only in `[dev-dependencies]` (removed in PR 5).

---

## PR 4 — BT tests stand on a mock `CallBridge` (the truth-test)

**Repo:** `arora-behavior-tree`. **Branch:** `feat/mock-call-bridge`.

This is the validation step for the extraction recipe (proposal
§2.3, §7 risk 5). If this PR cannot be completed cleanly, **stop
the whole plan and revise the recipe**.

1. Audit
   `arora-behavior-tree/arora-behavior-tree/src/tests.rs` and any
   other test files. For each test, decide:
   - **Unit-style (most):** rewrite to use an in-test
     `MockCallBridge` that records calls and returns canned
     `CallResult`s. Provide it in a `tests/common/mod.rs` helper
     (reuse the mock from PR 1's `arora-types` tests as a starting
     point).
   - **Genuinely needs the executor (some):** mark `#[ignore]` and
     add a one-line reason. These move to `arora-sdk` in PR 7.
2. Goal: `cargo test` works with **no** dev-dep on
   `arora-module-core` or `arora-registry` for the non-ignored tests.
3. If a specific test cannot be unhooked from the engine, document
   *why* in a comment. Do not paper over.
4. Once non-ignored tests pass without engine deps, delete
   `arora-module-core`, `arora-registry`, `semio-client`,
   `semio-record` from `[dev-dependencies]` if nothing else needs
   them.

**DoD:** `cargo test` in the BT repo passes with `[dev-dependencies]`
free of any `git = "...engine.git"` reference. **If this cannot be
achieved, STOP and escalate.**

---

## PR 5 — Create `semio-ai/arora-module-authoring` repo, populate it

**Repos:** new `arora-module-authoring`, then `engine`, then
`arora-behavior-tree`.

1. Create the repo:
   ```sh
   /opt/homebrew/bin/gh repo create semio-ai/arora-module-authoring --private \
     --description "Arora module tooling: codegen, registry, buffers"
   ```
2. Clone locally:
   ```sh
   git clone git@github.com:semio-ai/arora-module-authoring.git \
     /Users/victor.paleologue/Code/Semio/arora-module-authoring
   ```
3. Move the following crates from `arora-engine/crates/` to
   `arora-module-authoring/crates/` *with their git history* (`git filter-repo`
   or `git subtree split`):
   - `arora-module-core`, `arora-module-cli`, `arora-module-cpp`,
     `arora-module-rust`, `arora-registry`, `arora-vfs`,
     `arora-util`, `arora-buffers`, `wasi-sdk`

   (See proposal §7 risk 2 about whether `arora-buffers` should
   instead merge into `arora-types`. Decide before this step.)
4. Create `arora-module-authoring/Cargo.toml` workspace pointing at the moved
   crates. Copy `rust-toolchain.toml` and `.cargo/config.toml` from
   the engine.
5. Convert any inter-crate `path = "..."` deps that crossed the
   boundary into git deps on `arora-types` (already on crates.io).
   They should be none.
6. Copy `.github/workflows/continuous.yml` from `arora-engine`, trim to
   what `arora-module-authoring` needs (no NAO, no browser test, but keep the
   wasm32-wasip1 codegen smoke). It already carries clippy + fmt jobs
   (see "CI baseline" below).
7. **Stop and ask Victor** to add `SEMIO_GIT_CREDENTIAL` as an
   org-level secret if not already org-level. Needs `admin:org`:
   ```sh
   gh auth refresh -s admin:org
   gh secret set SEMIO_GIT_CREDENTIAL --org semio-ai --visibility all
   ```
   If already org-level, no action needed — the new repo inherits it.
8. Push to `main`, verify CI green, tag `v0.1.0`.
9. In `arora-engine`: open branch `feat/use-arora-module-authoring-repo`. Delete the
   moved crates from `arora-engine/crates/`. Update workspace `members`
   and `Cargo.toml` to git-dep them (`tag = "v0.1.0"`). Run
   `cargo build --workspace`. Update CI workflow's `branches:` allow
   list if needed.
10. In `arora-behavior-tree`: same change in its
    `[dev-dependencies]` / `[build-dependencies]`. Drop the engine
    references.

**DoD:** `arora-module-authoring` repo exists, CI green, tagged `v0.1.0`.
Engine and BT repos consume it via git tag, their CI is green.

---

## PR 6 — Move `behavior-tree-nodes` module into the BT repo

**Repo:** `arora-behavior-tree` (and `arora-engine` to remove).

1. `git subtree split` `arora-engine/modules/behavior-tree-nodes` into a
   branch with its history.
2. Land it under `arora-behavior-tree/modules/behavior-tree-nodes`.
3. Its `Cargo.toml` should depend on `arora-behavior-tree-types`
   (path), `arora-types` (crates.io), and `arora-module-rust` (git
   dep on `arora-module-authoring v0.1.0`). No engine reference.
4. Build it:
   ```sh
   cargo build -p behavior-tree-nodes --target wasm32-wasip1 --release
   ```
5. In `arora-engine`: delete `modules/behavior-tree-nodes`. Update workspace
   `members`/`default-members`. Update CI workflow's
   `cargo build -p behavior-tree-nodes --target wasm32-wasip1`
   step — either remove or re-point to a git dep.

**DoD:** `behavior-tree-nodes` builds from the BT repo without
touching the engine. The engine repo no longer references it.

---

## PR 7 — Create `semio-ai/arora-sdk`, move `arora-web` into it

**Repos:** new `arora-sdk`, then `arora-engine`.

1. ```sh
   gh repo create semio-ai/arora-sdk --private \
     --description "Arora SDK: engine + behavior-tree integration, arora-web"
   ```
2. Move `arora-engine/crates/arora-web` into `arora-sdk/crates/arora-web`
   with history.
3. In `arora-sdk/crates/arora-web/Cargo.toml`: switch `arora` from
   `path = ...` to git dep on the engine repo at a pinned tag.
4. Add `arora-sdk/crates/arora-web-behavior-tree` that depends on
   the BT repo and exposes a `BehaviorTreeRunner` wasm-bindgen
   surface. Extract the BT-aware bits currently in
   `arora-web/src/lib.rs` and `arora-web/www/`.
5. Add `arora-sdk/crates/arora-sdk` library crate that re-exports
   `arora` (engine) and `arora_behavior_tree`, and provides the
   `Instance::builder()` described in proposal §5.2.
6. Add `arora-sdk/crates/arora-sdk-cli` binary crate implementing
   proposal §5.3 (`arora run trees/hello.xml --module …`).
7. Move the engine-backed BT integration tests (those marked
   `#[ignore]` in PR 4) here and unignore them.
8. CI: full workflow — clippy, fmt, build, test,
   `wasm32-unknown-unknown`, headless Firefox via `wasm-pack test`.
9. In `arora-engine`: delete `crates/arora-web`. Update workspace and CI.

**DoD:** `arora-sdk` repo CI green. The `wasm-pack` output of
`arora-sdk`'s web crate runs the existing demo from `www/`.

---

## PR 8 — Rename the launcher binary to `arora-engine`

**Repo:** `arora-engine`.

The repo rename `engine` → `arora-engine` **already landed**: the
remote is `git@github.com:semio-ai/arora-engine.git` and clones work
(GitHub still redirects the old `engine` URL). The only part of the
original rename step left is the launcher binary — the crate still
produces a binary named `arora-cli`.

1. Rename the `crates/arora-cli` binary target to `arora-engine` in its
   `Cargo.toml` (`[[bin]] name = "arora-engine"`). Keep the crate
   name `arora-cli` to minimize blast radius.
2. Once `arora-module-authoring`, `arora-behavior-tree`, and `arora-sdk` exist,
   point their git dep URLs at `arora-engine.git` directly rather than
   leaning on the redirect.

**DoD:** `cargo install --path crates/arora-cli` produces a binary
named `arora-engine`.

---

## PR 9 — Org-wide CI baseline + ruleset

**Repos:** `semio-ai/.github` (create if needed), and the org
settings.

The CI bar we want on every repo:

| Job | Required? |
|---|---|
| `cargo build --release` workspace | yes |
| `cargo test --release` | yes |
| `cargo clippy --all-targets -- -D warnings` | yes |
| `cargo fmt --all -- --check` | yes |
| `wasm32-wasip1` guest build | yes for `arora-module-authoring`, `arora-behavior-tree`, `arora-engine` |
| `wasm32-unknown-unknown` build | yes for `arora-sdk`, `arora-engine` |
| Headless Firefox browser test | yes for `arora-sdk` |
| Markdown link check | yes |
| Version-bump check on PR | yes for `arora-types` (only crates.io-published) |

Current state (audit before changing):

- `arora-engine/.github/workflows/continuous.yml` — now has clippy + fmt
- `arora-behavior-tree/.github/workflows/continuous.yml` — no clippy,
  no fmt, very thin
- `arora-types/.github/workflows/ci.yml` — has clippy + fmt + version
  bump. Reference point.

Steps:

1. Add reusable workflow `.github/workflows/_rust.yml` in
   `semio-ai/.github`. Inputs:
   `wasm: bool`, `browser: bool`, `nao: bool`, `version_check: bool`.
2. Replace each repo's `continuous.yml` with a one-pager:
   ```yaml
   jobs:
     ci:
       uses: semio-ai/.github/.github/workflows/_rust.yml@main
       with:
         wasm: true
         browser: false
   ```
3. With `admin:org`, create an org ruleset requiring PR + passing CI
   on `main`/`master` for all five repos:
   ```sh
   gh auth refresh -s admin:org
   gh api -X POST orgs/semio-ai/rulesets --input ruleset.json
   ```
   Draft `ruleset.json` in the PR description for human review
   first. Minimum it should enforce:
   - Pull request required
   - Required status check: `ci` (the reusable workflow's job name)
   - Block force pushes
   - No deletions

**DoD:** All five repos run identical CI via the reusable workflow.
Pushing directly to `main`/`master` on any of them is blocked.

---

## After PR 9 — document the recipe

Write `arora-behavior-tree/EXTRACTING-A-MODULE.md` (or in
`arora-module-authoring`) documenting what PRs 3, 4, 6 actually required. This
is the artefact future extractions copy from. Cover at minimum:
- Cargo.toml dependency rules (proposal §2.3).
- How to write a `MockCallBridge` for tests.
- How to wire `arora-module-rust` in `build.rs` without the engine.
- CI template (the reusable workflow from PR 9).

The polly module is the obvious next candidate to validate the
recipe holds. Do not do it as part of this plan — do it after, with
a fresh proposal that should be ≤2 pages because most of the
groundwork is reusable.

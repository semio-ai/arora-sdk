# Implementation plan: bringing studio-bridge into the Arora architecture

Companion to [`proposal-bring-studio-bridge-in.md`](proposal-bring-studio-bridge-in.md).
Read the proposal first — this file assumes the target shape (§5),
the HAL placement decision (§3), and the data interface (§4) are
already understood.

This is the executable plan. Each task is sized to be one PR. Each
PR has a definition of done (DoD); do not move on until it passes.

If a step's DoD does not hold, **STOP and report back**. Do not
paper over a red CI with `--no-verify` or by skipping tests.

---

## Conventions

- Branch naming: `feat/<short-name>` in each affected repo.
- Commit style: follow each repo's existing convention
  (`<type>(<scope>): <subject>` in the Arora repos).
- Never force-push to `main`/`master`. Never delete branches the
  user has not authorized.
- Always run CI before merging by opening a draft PR; do not merge
  if CI is red.
- All cross-repo git deps pin to a tag or commit, never to a branch.

## Sequencing relative to proposal 1

| Plan-2 PR | Depends on |
|---|---|
| PR 1 (data interface in arora-types) | proposal 1 PR 1 merged + published (so arora-types is already in the "add new module" flow) |
| PR 2 (arora-ecbs first slice) | PR 1 published |
| PR 3 (Key/StateChange move) | PR 2 |
| PR 4 (HAL trait in arora-sdk) | proposal 1 PR 7 (arora-sdk exists) + PR 3 |
| PR 5 (BridgeConnector) | PR 4 |
| PR 6 (Instance accepts HAL + bridge) | PR 4, PR 5 |
| PR 7 (arora binary repo) | PR 6 |
| PR 8 (move HAL impls) | PR 7 |
| PR 9 (decommission studio-bridge engine + headless) | PR 8 |

Do not start PR 4 until proposal 1's PR 7 is merged.

## Checkpoints where the agent must stop and ask

- After **PR 1** (before `cargo publish arora-types`): confirm the
  version bump with Victor.
- After **PR 2** (arora-ecbs first slice): the trait shape is
  load-bearing for everything else. Review with Victor whether the
  `DataStore` trait survived contact with a real impl. If not,
  revise the trait before continuing (proposal §4, §8 risk 4).
- Before **PR 7** (create `arora` binary repo): name collision
  (proposal §8 risk 5). Confirm the binary/library naming with
  Victor.
- Before **PR 9** (deleting the studio-bridge engine + headless +
  controller + robots subtrees): this is irreversible-ish. Confirm.

---

## Tooling and environment

Same baseline as plan 1:

- `gh` at `/opt/homebrew/bin/gh` (v2.92.0).
- `repo` scope is enough to create repos under `semio-ai`.
- `admin:org` needed for org-level secrets/rulesets: refresh via
  `gh auth refresh -s admin:org`.
- Repos checked out side-by-side under
  `/Users/victor.paleologue/Code/Semio/`.

Additional working directories this plan touches:
- `studio-bridge/` (existing, will shrink).
- `arora-ecbs/` (exists as empty repo; this plan fills it).
- `arora/` (does not exist yet; PR 7 creates it).

---

## PR 1 — `arora-types::data` module (trait + HashMap impl)

**Repo:** `arora-types`. **Branch:** `feat/data-interface`.

Context: §4 of the proposal. We add a new `data::` module that
defines `Key`, `StateChange`, `DataError`, and the `DataStore`
trait. We include a `HashMap`-backed reference impl in the same
crate so consumers can use it without taking on arora-ecbs as a
dependency.

Steps:

1. Create `src/data/mod.rs` with submodules:
   - `key.rs` — `Key` (a path-shaped identifier; mirror the
     current `studio-bridge-msgs::Key` shape).
   - `state.rs` — `StateChange`, constructor helpers
     (`StateChange::set(key, value)`).
   - `error.rs` — `DataError`.
   - `store.rs` — the `DataStore` trait per proposal §4.
   - `memory.rs` — `MemoryStore`, a `HashMap<Key, Value>`-backed
     `DataStore` impl with a `tokio::sync::broadcast` for
     `subscribe()`. Pull the backpressure pattern from
     `studio-bridge-engine` (it already does this for mpsc
     channels — proposal §4 smell 3).
2. Re-export from `src/lib.rs`:
   ```rust
   pub mod data;
   ```
3. Add tests under `tests/data.rs`:
   - Write then read returns the value.
   - Subscribe + write delivers the change.
   - Snapshot returns the full state.
   - Two subscribers both receive every change; a slow subscriber
     does not block a fast one.
4. Bump `Cargo.toml` to `version = "1.3.0"` (after proposal 1's
   `1.2.0`).
5. Open PR, get CI green, merge, tag `v1.3.0`, **stop and ask
   Victor** before `cargo publish`.

**DoD:** `arora-types v1.3.0` is on crates.io.
`arora_types::data::{Key, StateChange, DataStore, MemoryStore}` is
usable in a scratch project.

---

## PR 2 — `arora-ecbs` first slice: real `DataStore` impl

**Repo:** `arora-ecbs`. **Branch:** `feat/datastore-impl`.
Prerequisite: PR 1 published.

The repo is empty today. We do *not* try to design the full
entity-component model in this PR. Goal: one concrete
implementation that satisfies the `DataStore` trait, with whatever
structure makes sense for ecbs — even if that just means a
`HashMap` with a different concurrency story than the
`arora-types::MemoryStore` reference.

Steps:

1. `cargo init --lib` at the repo root. Add `arora-types = "1.3"`.
2. Create `src/store.rs` with `pub struct Store { ... }` implementing
   `arora_types::data::DataStore`. Backing storage choice is open
   — start with `Arc<RwLock<HashMap<Key, Value>>>` if no better
   idea has emerged; the trait abstracts it.
3. Add a `Store::handle()` method returning a cheaply-cloneable
   handle (`Arc<Self>` or similar) so callers can hand the same
   store to HAL, bridge, BT, and the engine.
4. Tests: re-run the same suite as PR 1's `tests/data.rs` against
   `arora_ecbs::Store`. If the trait needs a tweak to make this
   ergonomic, **stop and revise PR 1's trait** rather than
   working around it.
5. CI: copy the `_rust.yml` reusable workflow from plan 1's PR 9
   (if landed) or a minimal `cargo build && cargo test`.
6. Tag `v0.1.0`.

**Checkpoint:** Stop. Report to Victor on whether the trait
survived contact. If the entity-component shape demands a
different surface (e.g. typed components, queries), revise PR 1
before continuing.

**DoD:** `arora-ecbs` v0.1.0 builds and tests green. The same
test bodies that pass against `MemoryStore` pass against
`arora_ecbs::Store`.

---

## PR 3 — Move `Key` and `StateChange` from `studio-bridge-msgs` to `arora-types`

**Repo:** `studio-bridge` (and `arora-types` if any fix-up needed).
**Branch:** `feat/key-to-arora-types`.
Prerequisite: PR 1 merged.

Context: proposal §3 and §7 first bullet. `Key` and `StateChange`
currently live in `studio-bridge/msgs/src/state.rs` and are used
by `device-client`, `controller`, and (after PR 1) by
`arora-types::data`. Until they share a single definition, the
HAL trait extraction in PR 4 cannot land.

Steps:

1. In `studio-bridge/msgs/src/state.rs`: delete the local `Key`
   and `StateChange` types. Re-export from `arora-types`:
   ```rust
   pub use arora_types::data::{Key, StateChange};
   ```
   Keep anything else in `state.rs` that does not move (wire-only
   wrappers, serde shims).
2. In `studio-bridge/msgs/Cargo.toml`: bump `arora-types` to
   `"1.3"`.
3. Build the studio-bridge workspace; fix any path-of-import
   breakage (`studio_bridge_msgs::Key` callers stay working via
   the re-export, but qualified paths inside `msgs` may need
   adjustment).
4. Run the studio-bridge test suite. Particularly: any wire-format
   round-trip tests that pinned to the old `Key` serde shape.

**DoD:** `studio-bridge` workspace builds and tests green with
`Key`/`StateChange` re-exported from `arora-types`. No behaviour
change at the wire level (verify by running an existing
device-client round-trip test, or by serializing a known message
and diffing against a committed fixture).

---

## PR 4 — `arora-sdk::Hal` trait

**Repo:** `arora-sdk`. **Branch:** `feat/hal-trait`.
Prerequisite: proposal 1's PR 7 merged (so arora-sdk exists), and
plan-2 PR 3 merged.

Steps:

1. In `arora-sdk/crates/arora-sdk/Cargo.toml`: add
   `arora-types = "1.3"`, `async-trait`, `futures-core` (for
   `Stream`).
2. Create `src/hal.rs` with the trait sketched in proposal §3:
   `Hal::describe`, `read`, `write`, `updates`.
3. Add `HalDescription`, `HalResult`, `HalError` next to it.
   Mirror the surface of today's
   `studio_bridge_controller::StudioBridgeController` so the port
   in PR 8 is mechanical:
   - `get_model` → `describe`.
   - `get` / `get_all` → `read`.
   - `update` / `update_single` → `write`.
   - `get_model_glb` → optional method or a separate
     `HalAssets` extension trait (decide; if unsure, separate
     trait keeps `Hal` lean).
4. Provide a `FakeHal` in a `#[cfg(any(test, feature = "fake"))]`
   module — the port of today's
   `studio_bridge_controller::fake::FakeController`. Strip
   bridge-specific behaviour; it just echoes writes back into its
   own store.
5. Tests: `FakeHal` round-trip via a `MemoryStore`. Subscribing to
   `updates()` while writing yields the writes in order.
6. Tag `v0.x.0` (one minor bump in arora-sdk).

**DoD:** `arora-sdk::Hal` exists and is implemented by `FakeHal`.
Tests pass. No reference to `studio-bridge` in `arora-sdk`'s
`Cargo.toml`.

---

## PR 5 — `studio-bridge::BridgeConnector`

**Repo:** `studio-bridge`. **Branch:** `feat/bridge-connector`.
Prerequisite: PR 2 (arora-ecbs) tagged, PR 4 (HAL trait) tagged.

Context: proposal §5.2 ("studio-bridge AFTER"). The bridge stops
owning the loop. It becomes a connector built around a
`DataStore` handle.

Steps:

1. New crate `studio-bridge/connector/` with library target
   `studio-bridge-connector` (or merge into an existing
   not-going-away crate; do not put it in `engine`, which is being
   removed in PR 9).
2. API per proposal §6.1:
   ```rust
   pub struct BridgeConnector { ... }
   impl BridgeConnector {
       pub fn builder() -> BridgeConnectorBuilder { ... }
       pub async fn run(self) -> Result<(), BridgeError> { ... }
   }
   pub struct BridgeConnectorBuilder { ... }
   impl BridgeConnectorBuilder {
       pub fn with_data_store(self, ds: Arc<dyn DataStore>) -> Self;
       pub fn with_studio_client(self, sc: ...) -> Self;
       pub fn with_device_client(self, dc: ...) -> Self;
       pub async fn build(self) -> Result<BridgeConnector, BridgeError>;
   }
   ```
3. Internals: lift the bidirectional mirroring from
   `studio-bridge/engine/src/engine.rs` (the "wait on studio
   messages, apply to controller, send back" loop) but rewrite so
   *both sides* are `DataStore` ↔ studio-client. The controller no
   longer exists in this code path; HAL writes show up in the
   store and the connector forwards them to Studio.
4. Error/lifecycle parity (proposal §7 bullet 2): the connector
   must fail in the same way today's `studio_bridge_engine::run`
   fails on device unregistration. Define a `BridgeError` variant
   for this and document that `Instance::run()` will propagate it.
   Add a regression test (mock studio-client that emits an
   unregister event; assert `connector.run()` returns the expected
   error variant).
5. Do **not** delete `studio-bridge/engine/` or `controller/` or
   `headless/` yet — they stay in place until PR 9.

**DoD:** `BridgeConnector` builds against a `DataStore` handle and
a pair of studio/device clients. The unregister-regression test
passes.

---

## PR 6 — `arora-sdk::Instance::builder()` accepts `Hal` and `BridgeConnector`

**Repo:** `arora-sdk`. **Branch:** `feat/instance-hal-bridge`.
Prerequisite: PR 5.

Context: proposal §6.1 — the robot main wires `Instance::builder()
.with_data_store(store).with_hal(hal).with_bridge(bridge).build()`.

Steps:

1. Extend `arora_sdk::InstanceBuilder` with:
   ```rust
   pub fn with_data_store(self, ds: impl DataStore + 'static) -> Self;
   pub fn with_hal(self, hal: Box<dyn Hal>) -> Self;
   pub fn with_bridge(self, bridge: studio_bridge_connector::BridgeConnector) -> Self;
   ```
   The `with_bridge` method is feature-gated behind
   `feature = "bridge"` so an SDK consumer that does not want
   studio-bridge does not pay for it.
2. `Instance::run()` becomes a `tokio::select!` over:
   - the engine's own dispatch loop,
   - `bridge.run()` if set,
   - HAL `updates()` → `data_store.write()`,
   - data-store change subscription → HAL `write` for keys the HAL
     owns (decide ownership: simplest is "HAL writes its own
     `describe()`-listed keys back; everything else is bridge
     territory").
3. Document the supervision rule: if any of the three tasks
   returns `Err`, `Instance::run()` returns `Err` and aborts the
   others. This preserves today's "studio-bridge dies → process
   dies" semantics required by PR 5's DoD.
4. Tests:
   - Instance with `FakeHal` + `MemoryStore` + no bridge: HAL
     writes appear in the store.
   - Instance with `FakeHal` + `MemoryStore` + a mock
     `BridgeConnector` that fails after 100ms: instance returns
     the bridge's error.

**DoD:** `Instance::builder()` exposes all four `with_*` methods.
The supervision tests pass.

---

## PR 7 — Create the `arora` binary repo

**Repo:** new `semio-ai/arora`. **Branch (in arora):** initial
commit `main`.
Prerequisite: PR 6 tagged. **Stop and confirm naming with Victor
before this PR** (proposal §8 risk 5).

Steps:

1. ```sh
   /opt/homebrew/bin/gh repo create semio-ai/arora --private \
     --description "Arora robot launcher: HAL impls + SDK + bridge, feature-gated per robot"
   git clone git@github.com:semio-ai/arora.git \
     /Users/victor.paleologue/Code/Semio/arora
   ```
2. Workspace layout:
   ```
   arora/
   ├── Cargo.toml          (workspace)
   ├── crates/
   │   └── arora-launcher  (library — workspace lib name to dodge
   │                        the binary/crate name collision)
   ├── hal/                (per-robot HAL impls, one crate each)
   │   ├── arora-hal-fake
   │   └── (PR 8 fills the rest)
   ├── src/main.rs         (binary target `arora`, in the root
   │                        package — `[[bin]] name = "arora"`)
   └── .github/workflows/  (reuses _rust.yml)
   ```
3. Root `Cargo.toml` has `[[bin]] name = "arora"` so
   `cargo install --path .` produces `arora`. The library crate
   inside `crates/arora-launcher` exists for code reuse and to
   avoid the package-name collision with the `arora` engine crate.
4. `arora-launcher/src/lib.rs`: the `build_instance(args)`
   function called from `main.rs`. It reads CLI args (use `clap`),
   builds the data store (default `arora_ecbs::Store::new()`),
   selects the HAL via feature, builds the bridge connector,
   constructs `arora_sdk::Instance::builder()`, and returns the
   `Instance`.
5. `src/main.rs`: parse args; build instance; `instance.run().await`.
6. Feature flags (initially): `fake` (always on for dev). PR 8
   adds the rest.
7. Push to `main`. CI green. Tag `v0.1.0`.

**DoD:** `cargo run --features fake -- --robot fake` boots an
arora instance using `FakeHal` + `MemoryStore` and exits cleanly
on SIGINT.

---

## PR 8 — Move per-robot HAL impls from `studio-bridge` to `arora`

**Repos:** `arora` (new home), `studio-bridge` (source — do not
delete yet). **Branch (arora):** `feat/hal-impls`. **Branch
(studio-bridge):** none yet — deletion happens in PR 9.

Today: `studio-bridge/robots/{ros2-robots,restful-api-robots,nao}/`
implement `studio_bridge_controller::StudioBridgeController`.

Steps:

1. For each robot family, decide whether to land it as one crate
   or split it (proposal §7 bullet 3). Default plan:
   - `arora/hal/arora-hal-ros2` containing `quori`, `ur3`, `ur5`,
     `nao-ros2`, `pepper-ros2` as Cargo features (mirrors today's
     feature gating).
   - `arora/hal/arora-hal-restful` containing `hackerbot`.
   - `arora/hal/arora-hal-fake` already exists from PR 7.
2. Subtree-split each robot crate from studio-bridge **with
   history** into the new home (use `git filter-repo` or
   `git subtree split`).
3. In each moved crate, port the trait impl from
   `StudioBridgeController` to `arora_sdk::Hal`. The method
   mapping is the one from PR 4 step 3.
4. Update `arora`'s `Cargo.toml` features:
   ```toml
   [features]
   default = []
   fake     = ["arora-hal-fake"]
   quori    = ["arora-hal-ros2/quori"]
   ur3      = ["arora-hal-ros2/ur3"]
   # …mirror today's headless feature list…
   ```
5. The launcher's `build_hal()` selects between them using
   `#[cfg(feature = "...")]`.
6. CI: build with each feature; tests with `fake`.

**DoD:** Building `arora --features ur5` (or any other
robot-flagged feature) produces a binary that, when run against a
reachable robot endpoint, behaves like today's
`studio-bridge-headless --features ur5`. Compare on a checkout
that still has both.

---

## PR 9 — Decommission `studio-bridge/{engine,controller,headless,robots}`

**Repo:** `studio-bridge`. **Branch:** `feat/decommission-runtime`.
Prerequisite: PR 8 in `arora` is verified working against at least
one real robot (or against the same fixture the old
`studio-bridge-headless` test harness used). **Stop and confirm
with Victor before merging this PR.**

Steps:

1. Delete from the workspace:
   - `studio-bridge/engine/`
   - `studio-bridge/controller/`
   - `studio-bridge/headless/`
   - `studio-bridge/robots/`
2. Remove them from the root `Cargo.toml` `members`.
3. Drop `animation-player` dep (it moved with the engine/launcher
   per proposal §8 risk 3 — confirm where it ended up: most
   likely `arora-sdk` or `arora-launcher`).
4. Remaining workspace members:
   - `msgs`, `studio-client`, `device-client`,
     `firestore-stream`, `devices-firestore`, `connector` (from
     PR 5), `get-robot-model`, `token-storage`.
5. Update the studio-bridge README to describe the connector role
   (no more "headless binary" section).
6. Tag a major version bump (`v0.x` → `v1.0`?) since the
   workspace surface area drops sharply.

**DoD:** `cargo build --workspace` in studio-bridge succeeds with
the runtime crates gone. `cargo install --path crates/arora` in
the arora repo still works. Production deploys swap from the old
`studio-bridge-headless` binary to the new `arora` binary with no
behaviour change observable to Studio.

---

## After PR 9 — documentation and cleanup

- Write `arora/README.md`: how to add a new robot family (new
  crate under `hal/`, new feature flag, new launcher arm).
- Write `studio-bridge/CONNECTOR.md`: how to embed the bridge
  connector standalone (proposal §6.2 is the starting point).
- Land a one-page entry in `arora-ecbs` explaining the
  `DataStore` contract and which methods are hot-path vs cold.
- Promote the `Hal` trait from `arora-sdk` to `arora-engine` if a
  use case has emerged that justifies it (proposal §8 risk 1).
  Otherwise leave it.

---

## What is *not* in this plan

- The full entity-component shape of `arora-ecbs`. PR 2 ships a
  thin `DataStore` impl; the structural work is its own proposal.
- ROS 2 / Zenoh transport changes. The connector reuses today's
  `studio-client` and `device-client` as-is.
- Removing `Firestore` from the bridge. That is a separate concern
  (proposal 3, if it happens).
- Browser / wasm support for the HAL trait. The trait is
  `Send + Sync + async_trait` — fine for native, undefined for
  wasm. Defer until a real wasm-HAL consumer appears.

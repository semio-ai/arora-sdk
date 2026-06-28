# Implementation plan: bringing studio-bridge into Arora (consolidated-workspace revision)

> **Revised 2026-06-27.** The original version of this plan assumed the
> multi-repo split (separate `arora-types`, `arora-ecbs`, `arora-sdk`, and a
> new `arora` binary repo). That split was **reversed**: everything now lives
> in **one `arora-sdk` workspace** (the `arora-engine` repo). This revision
> rewrites the plan for that reality and for Victor's three-interface framing.
> The companion rationale is still [`proposal-bring-studio-bridge-in.md`](proposal-bring-studio-bridge-in.md)
> (read §3–§4 for the HAL/data reasoning); ignore its repo-topology sections.

If a step's DoD does not hold, **STOP and report back**. Do not paper over red
CI. Never force-push `main`; open PRs.

---

## 1. The target in one picture

`arora` (the crate already in this workspace, `crates/arora`) becomes the
opinionated runtime that **bundles three pluggable interfaces**. You get all
three by depending on `arora`; you choose an *implementation* of each.

```
                        ┌────────────────────────── crates/arora ──────────────────────────┐
   pick a HAL impl ───► │  HAL (control)   ◄── trait, from studio-bridge `controller`        │
   (fake | nao_ros2 |   │  Data (state)    ◄── trait, from studio-bridge `State`/StateChange │
    ur5 | hackerbot…)   │  Bridge (conn.)  ◄── trait, modelled on device-client `DeviceClient`│
   pick a bridge impl ─►│                                                                    │
   (studio-bridge|none) │  + the engine loop (was studio-bridge `engine`) — library side      │
                        │  + the launcher    (was studio-bridge `headless`) — binary side     │
                        └────────────────────────────────────────────────────────────────────┘
                                 │ Data trait              │ Bridge trait
                                 ▼                          ▼
                        arora-types::data          studio-bridge (stays a separate repo):
                        (Key, State, StateChange,    device-client (impls the Bridge trait),
                         DataStore + in-mem impl)    studio-client + Zenoh routing ("studio + router"),
                                                     msgs (wire), firestore-stream, token-storage
```

The three interfaces:

| Interface | Source today (studio-bridge) | New name / home | Implementations | Selected by |
|---|---|---|---|---|
| **Control** | `controller::StudioBridgeController` | `Hal` trait — `crates/arora-hal` | fake, ros2 (quori/ur3/ur5/nao-ros2/pepper-ros2/unitree-g1), restful (hackerbot), nao | `arora` cargo feature |
| **Data** | `msgs::State` / `Key` / `StateChange` + the subscribe/get/update APIs | `arora-types::data` (`DataStore` trait + `MemoryStore`) | in-memory default; `arora-ecbs` later | builder arg (default in-mem) |
| **Bridge** | `device-client::DeviceClient` | `Bridge` trait — `crates/arora` (or `arora-sdk`) | `studio-bridge`'s connector (stays in that repo) | builder arg / `bridge` feature |

"Getting `arora` gets you these interfaces automatically; you specify which one
you want (fake, nao_ros2, …)" = the `arora` crate wires Control + Data + Bridge
into one run loop; you pick a HAL impl by feature and a bridge impl by builder.

---

## 2. What moves into the `arora-sdk` workspace, and what stays in `studio-bridge`

### Moves IN (studio-bridge → this workspace)

| From studio-bridge | To | Notes |
|---|---|---|
| `controller` (trait + `FakeController`) | `crates/arora-hal` (trait + `arora-hal-fake`) | Rename `StudioBridgeController` → `Hal`; depend only on `arora-types` (no bridge types). |
| `robots/ros2-robots`, `robots/restful-api-robots`, `robots/nao` | `crates/arora-hal-{ros2,restful,nao}` (or grouped `crates/arora-hal/*`) | Port `impl StudioBridgeController` → `impl Hal`. Keep the per-robot cargo features. |
| `engine` (the `run()` driver loop) | `crates/arora` **library** (`Arora::run`) | This is "engine → arora, the library side." The `tokio::select!` over device-info / commands / controller-data / animation becomes the arora run loop over Bridge + HAL + Data + BT. |
| `headless` (the binary) | `crates/arora` **binary** (`src/main.rs`) | "headless → arora." Migrate **all** of it: clap args + env fallbacks, `bridge_config`, `app_data_files`, encrypted token storage, interactive device-info prompts, feature-gated HAL selection, Firebase build-env forwarding, device-info sync, then run. |
| `msgs::{State, Key, StateChange, AroraOp, AroraCall…}` (shared vocabulary) | `arora-types::data` | Wire-only types (`DeviceInfo`, Zenoh key encoding) stay in `msgs`. `msgs` re-exports the moved types from `arora-types` for wire compat. |
| animation-player integration (engine ticks `animation_engine`) | `crates/arora` (lib) | The animation tick is part of the engine loop; carry it. `animation-player` becomes an `arora` dep (or a feature). |

### Stays in `studio-bridge` (per Victor)

- **`device-client`** — the `DeviceClient` trait **and** its Firebase + Zenoh
  implementations. ("device client interfaces and implementations stay.") It
  will additionally implement `arora`'s `Bridge` trait (or be wrapped by a thin
  `BridgeConnector` that does).
- **`studio-client` + the Zenoh routing** = "studio + router." The browser/native
  Studio side, device discovery (liveliness), claim/RPC routing over Zenoh.
- **`msgs`** (wire format), **`firestore-stream`**, **`devices-firestore`**,
  **`token-storage`**, **`get-robot-model`** — bridge/auth plumbing.
- A **`BridgeConnector`** that adapts `device-client` to the `Bridge` trait, so
  `arora` depends on `studio-bridge` only through that trait (+ an optional
  `bridge` feature).

> Note: there is **no `router` crate** in studio-bridge today — routing is the
> Zenoh network + `studio-client` + the engine loop. "The router stays" is read
> as "the Studio-facing Zenoh routing (studio-client side) stays in the bridge."
> Confirm if a dedicated router crate is intended (open decision D1).

---

## 3. Target crate layout

```
arora-sdk workspace (this repo) — added crates in *bold*:
  crates/
    arora-types        + data:: { Key, Value(already), State, StateChange, DataStore, MemoryStore, DataError }
    arora-engine, arora-behavior-tree(+types,+yaml), arora-registry,
    arora-module-authoring/{core,cli,cpp,rust}, arora-buffers, arora-util,
    arora-vfs, wasi-sdk, arora-cli, arora-web
    arora              ← absorbs engine loop (lib) + headless (bin); exposes Bridge trait;
                         features select the HAL impl
    *arora-hal*        ← the Hal trait (+ FakeHal); arora-types-only deps
    *arora-hal-ros2*   ← quori/ur3/ur5/nao-ros2/pepper-ros2/unitree-g1 (features)
    *arora-hal-restful*← hackerbot
    *arora-hal-nao*    ← NAO C++ SDK binding (if kept)
  modules/ … (unchanged)

studio-bridge (separate repo, shrinks):
  msgs (− moved vocab, re-exports it), studio-client, device-client (+ impl Bridge),
  firestore-stream, devices-firestore, token-storage, get-robot-model, connector/
```

`arora` depends on `studio-bridge` only via the optional `bridge` feature, the
way `arora-sdk` already depends on the behavior tree behind a seam. The engine,
BT, module tooling, and types stay bridge-free.

---

## 4. Phased PRs (all in-workspace unless noted)

Each phase is one PR with a DoD. Phases 1–4 are additive (nothing breaks);
5–7 do the cutover.

**Phase 1 — `arora-types::data`.**
Add `Key`, `State`, `StateChange`, `DataError`, the `DataStore` trait, and a
`MemoryStore` (HashMap + `tokio::broadcast` for `subscribe`). Mirror the current
`msgs::state` shapes so the later move is mechanical. Tests: write/read,
subscribe-delivers, snapshot, slow-subscriber-doesn't-block-fast.
*DoD:* `arora_types::data::*` usable; release.yml will publish the version bump.
**Checkpoint:** confirm the version bump (and the `Key` serde shape, since it
becomes the wire type) with Victor before publish.

**Phase 2 — `crates/arora-hal` (the Control interface).**
Port `StudioBridgeController` → `Hal` (deps: `arora-types` only). Map
`get_model→describe`, `get/get_all→read`, `update→write`, `subscribe→updates`,
`get_model_glb→` a `HalAssets` extension trait (keep `Hal` lean). Include
`FakeHal` (port of `FakeController`). Tests: FakeHal round-trip + ordered
`updates()`. *DoD:* `arora-hal` builds, `FakeHal` passes, no studio-bridge dep.

**Phase 3 — the `Bridge` interface + studio-bridge `BridgeConnector`.**
Define a `Bridge` trait in `arora` modelled on `DeviceClient`
(`device_info_updated`, `update_device_info`, `data_requested`, `send_data`,
`command_receiver`), in `arora-types` terms. In **studio-bridge**, add a
`connector` crate implementing it over the existing `device-client`. Preserve
the unregister→error lifecycle (regression test). *DoD:* `BridgeConnector`
satisfies `Bridge`; unregister test passes. (studio-bridge PR; pin by tag.)

**Phase 4 — `arora` library absorbs the engine loop.**
Bring `engine::run`'s `tokio::select!` into `Arora` as the driver loop over:
Bridge commands → HAL/Data; HAL `updates()` → Data; Data changes → Bridge
`send_data`; device-info sync; animation tick; BT tick. Supervision: if any task
errors, `Arora::run` returns `Err` and aborts the rest (matches today's
"bridge dies → process exits"). Builder: `Arora::builder().with_data_store(..)
.with_hal(..).with_bridge(..).build()`. Keep `run_groot_xml` for direct trees.
*DoD:* `Arora` with FakeHal + MemoryStore + mock bridge passes supervision tests.

**Phase 5 — `arora` binary absorbs headless.**
Migrate the whole `headless` binary into `crates/arora/src/main.rs` (+ helper
modules): CLI/env, `bridge_config`, app-data dir, token encrypt/load, prompts,
Firebase env forwarding, device-info sync, feature-gated HAL selection, then
`Arora::run`. *DoD:* `cargo run -p arora --features fake` boots a fake instance
and exits cleanly on SIGINT, matching today's `studio-bridge-headless`.

**Phase 6 — robot HAL impls.**
Subtree-split `robots/*` from studio-bridge **with history** into
`crates/arora-hal-{ros2,restful,nao}`; port impls to `Hal`; wire `arora`
features (`quori`, `ur5`, `hackerbot`, …) mirroring headless. *DoD:* `arora
--features ur5` behaves like `studio-bridge-headless --features ur5` against a
robot endpoint.

**Phase 7 — shrink studio-bridge.** (studio-bridge PR; **confirm before merge.**)
Remove `engine`, `controller`, `headless`, `robots`. Move the shared vocab to
the `arora-types` re-export. Keep device-client, studio-client, msgs, firestore
helpers, token-storage, connector. Decide where `animation-player` lands (arora).
*DoD:* studio-bridge builds with runtime crates gone; `arora` reproduces the old
headless behaviour to Studio.

---

## 5. Documentation migration (≈35 files, ~7.9k lines — a real workstream)

Studio-bridge carries substantial docs. They split by *what the doc is about*,
not where the code goes. Do this incrementally alongside the phase that moves
the corresponding code; track with a checklist so none is dropped.

**Move INTO this repo (arora runtime / HAL / data / engine concerns):**
- `headless/README.md` (183) → `crates/arora/` runtime/launcher docs.
- `engine/README.md` (8) → folded into `crates/arora` lib docs.
- `robots/ros2-robots/README.md` (341), `robots/nao/readme.md` (62) → the
  `arora-hal-*` crate readmes; `restful-api-robots` rustdoc (46) → `arora-hal-restful`.
- `fake_bot/README.md` (119), `fake_bot/EXECUTION_APPROACHES.md` (144) → arora HAL/testing docs.
- Data/state-model design: `zenoh-study/key_typing_and_registry.md` (313),
  `zenoh-study/local_echo_ecs_sync.md` (239) → `docs/` (informs `arora-types::data`).
- Relevant architecture from `docs/index.md` (414) → merged into this repo's
  `docs/architecture.md` (the runtime/loop/HAL parts).

**Stay in studio-bridge (Studio / Zenoh / connectivity concerns):**
- `README.md` (313, rewritten to the connector role), `docs/diagnosing-zenoh-sessions.md` (415),
  most of `zenoh-study/*` (claim mechanism, ACL, REST-vs-WS, deployment, slides, feasibility…),
  `studio-client/**` docs (808), `device-client/README.md` (25), `msgs/tests/README.md` (51).

**Split:** `docs/index.md` and `zenoh-study/zenoh_bridge_proposal_draft.md`
straddle both — extract the arora-runtime parts here, leave the Zenoh-bridge
parts there, and cross-link.

A precise per-file destination table is the first task of the doc workstream;
the inventory (path + topic + size) is captured in the study that produced this
revision.

---

## 6. Open decisions (need Victor) before scaffolding

- **D1 — "router".** No `router` crate exists; "studio + router stay" is read as
  "studio-client + Zenoh routing stay." Confirm, or name the intended router crate.
- **D2 — `arora-ecbs`.** Bring the (empty) `arora-ecbs` repo into this workspace
  as the canonical `DataStore`, or keep `arora-types::MemoryStore` as the default
  and defer ecbs? (Plan above defers it.)
- **D3 — HAL crate layout.** One `crates/arora-hal` + sibling impl crates, or a
  grouped `crates/arora-hal/{fake,ros2,restful,nao}` like arora-module-authoring?
- **D4 — `Bridge` trait location.** In `crates/arora`, or a lean `crates/arora-bridge`
  so non-arora consumers can implement it without pulling arora?
- **D5 — `animation-player`.** Bring it into the workspace, keep it a git dep of
  `arora`, or feature-gate it?
- **D6 — wire-format coupling.** Moving `Key`/`StateChange` to `arora-types`
  couples the in-process and wire types; keep them additive-only (never restructure).

## 7. What is NOT in this plan
- The full entity-component shape of `arora-ecbs` (its own design).
- ROS2/Zenoh transport changes — the bridge reuses studio-client/device-client as-is.
- Browser/wasm HAL (the trait is native-async; defer until a wasm-HAL consumer exists).

# Proposal: configurations — modules, type records and behaviors applied together

Status: draft, for review.
Date: 2026-10-15.

A device runs modules and behaviors. Today each reaches it by a separate path:
an operator copies a module directory into the device directory, a client
pushes a behavior through the interpreter module, and nothing carries the type
records a module's functions name. This document does two things:

- **Section 1–2** review what exists for distributing and loading modules and
  types, where it overlaps, and the smallest change that lets a device index a
  guest module's record-typed exports with what exists.
- **Section 3–6** propose a **configuration**: a versioned asset that bundles
  modules, the type records they need and the behaviors that use them, and a
  bridge operation that tells a device to run one.

In short:

- **A guest module's type records travel beside its header, and the device
  indexes the module from its module record.** The header stays as it is. The
  builder takes a set of frozen records; a guest module whose record is in the
  set joins the method index with the record's signatures, checked against the
  header. This unblocks behaviors calling `step_values`
  ([STUDIO-347](https://linear.app/semio-ai/issue/STUDIO-347)) before any
  store or protocol work.
- **A module is a published Studio asset** with two artifacts: its executable,
  and its interface (the header and the frozen records). `arora-index`
  ([#360](https://github.com/semio-ai/arora-sdk/pull/360)) resolves and fetches
  both, verified by hash.
- **A configuration is a published asset too.** Its manifest pins modules by
  scoped name, version and content hash, and carries behaviors inline, each
  with its graph type and version. HAL and bridge settings are not part of it.
- **Two new bridge operations**: `ApplyConfiguration` replaces what a device
  runs with a configuration; `DescribeConfiguration` returns the one it runs.
  The device resolves every asset with its own identity, prepares the new
  engine beside the running one, and swaps between two steps. A configuration
  that does not apply leaves the old one running.
- **The claim is the gate.** The router grants the new operations to the
  device's claimant, as it grants calls; the device adds no access rule.
- **The device keeps the configuration it runs** in its device directory and
  applies it again at start, from the asset cache when offline.
- **The cost is a two-repo release**: an `arora-bridge` major, a
  `studio-bridge-msgs` major with its client chain, a new router grant, and a
  protocol revision a device announces, so a client never sends the new
  operations to a device that cannot answer them.

---

## 1. What exists

### 1.1 Inventory

| Component | Where | What it does | Used by | Status |
|---|---|---|---|---|
| `arora-index` | arora-sdk, `crates/arora-index` | Resolves a published Studio v2 asset `{scopedName, range}` to a release with Studio's selection rules; fetches its primary artifact verified by SHA-256; content-addressed cache with sealed private blobs and `resolved.json`; `resolve_with_fallback`; `build::models()` for build pins. Asset types `Model`, `Animation`, `Face`. | Nothing yet | [#360](https://github.com/semio-ai/arora-sdk/pull/360), open, `publish = false` |
| `arora-registry` 2.0 | arora-sdk | Local record registry: adds, tags (freezes) and looks up structures, enumerations, modules and folders by UUID or folder path; loads record directories (`records/{folder,enumeration,structure,module}/<uuid>@<version>.yaml`) and in-memory records (`load_records`). | `arora-module-cli` and its C++/Rust generators, `arora-cli`, `arora-behavior-tree`'s build | Merged, published |
| `arora-registry-remote` 1.0 | arora-sdk | `RemoteRegistry` (read-only, GraphQL to semio-db through `semio-client`) and `RemoteCachedRegistry` (remote plus a local cache; writes stay local). Reads the semio-cli login file. | `arora-cli`, `arora-module-cli` | Merged, private |
| `feat/type-registry` | arora-sdk branch | Emitted `records/` and a `records.json` id→name index per module from `arora-module-rust`; a wasm `Registry` in `arora-web` resolving ids to names. | Nothing | Unmerged; the crate it changes no longer exists |
| Module directories | `arora`, `module_dir` | A device loads every `<device dir>/modules/*/` (a `header.json` beside one artifact) at start, and each `--module` directory. | The `arora` runner | Merged |
| Guest module indexing | `arora`, `module_discovery` | A header-loaded guest export joins the method index only when its parameters and return are primitives or optional scalar primitives. `with_declared_module` indexes every export from a Rust declaration. | Every device | Merged (arora 12.13) |
| Module declarations | `arora-module`, `arora-module-macros` | A Rust declaration produces the header, the frozen **module record** (`AroraModule::record`), the exports and client stubs. Derived types produce a `ty::low::Type`, not a structure record. | polly, test-rust-wasm, vizij modules | Merged |
| `module.yaml` pipeline | `arora-module-cli`, `arora-module-authoring` | Analyzes a `module.yaml` against a registry: frozen module record and frozen type records (`ModuleAsset::{Module,Type}`), then generates C++/Rust bindings. | C++ modules, importing Rust modules | Merged |
| `arora-web` | arora-sdk | Browser runtime: `AroraRuntimeBuilder.withModule(headerJson, bytes)`, `withCompiledModule`. No records. | arora-sdk demos | Merged |
| Bridge operations | `arora-bridge` 6.1 | `Get`, `Update`, `Call`, `ListKeys`, `ListMethods` (deprecated), `DescribeMethods`. Behaviors travel as calls to the interpreter module (`LOAD`, `EDIT`, `SPAWN`, `HALT`). Nothing carries modules or types. | Every bridge | Merged |
| Studio Bridge | studio-bridge | `AroraOp` mirrors `BridgeOp` (msgs 6.x); sender-addressed command keys `rpc/{device}/{principal}/{op}/{target}` the router authorizes per operation; protocol token `mgmt/{device}/liveliness/protocol/2`. | Studio, `arora --features studio-bridge` | Merged |
| Studio v2 assets | semio_studio | `scopedNames`, `publishedAssets/{id}/releases`; a release has `artifacts: {key → {storage, contentHash, role}}` with roles `Source`, `Preview`; `AssetType` is `Model`, `Animation`, `Face`; `resolveDependency` selects releases; projects keep `dependencies` and `resolved`. No module, type or configuration asset. | Studio | Merged |
| `studio-arora` | semio_studio, Studio-on-Arora branches | Studio's own wasm crate on arora 12.13. Loads `@vizij/animation-module` (header and wasm from npm) once as a `CompiledModule`, builds each emulated device with `with_compiled_module`; pushes the stepping behavior by `spawn_graph`, relinks by `edit`. | Studio | Branch |
| `vizij-animation-module` | vizij-rs | The animation module, declared in Rust; `examples/header.rs` prints the header the npm package `@vizij/animation-module` 4.0.0 ships beside the wasm. Its declaration produces the module record; nothing exports it. | Studio, vizij devices | Merged, published |
| semio-cli | semio-cli | CLI over semio-db: record create/tag/get-frozen per kind, blob upload/download, username/password login stored in `~/.semio/cli.yaml`. No Studio v2. | Developers, `arora-registry-remote`'s config | Maintained, unused by devices |
| semio-db, semio-client, semio-record, semio-store-rpc | separate repos | GraphQL record store on ScyllaDB (records by UUID and version, semver tags, folder tree), a WebSocket blob store, and their client. | semio-cli, `arora-registry-remote` | Maintained and tested; no production deployment found |

### 1.2 The gap behind STUDIO-347

`AroraBuilder::build` freezes a guest export's signature from its header. A
header's `TypeRef` names a type by id without a version, and a frozen signature
pins every record reference to a version, so an export that names a record —
`step_values` returns `StepValues`, `player_states` returns `PlayerState[]` —
dispatches but is not indexed. A behavior tree resolves its leaves against the
method index, so a graph calling either is refused.

`with_declared_module::<M>()` indexes those exports, because the declaration
carries frozen signatures. It needs the declaring crate linked into the host.
Studio's devices take the module as header and wasm from npm, and the `arora`
runner takes module directories, so neither can use it.

The frozen signatures exist: the declaration produces them as the module record
(`AroraModule::record`), the form a store serves. What is missing is a way to
hand that record to a device.

### 1.3 Overlaps, and which component owns each concern

| Concern | Overlapping components | Owner | Why |
|---|---|---|---|
| Finding and fetching a published artifact | `arora-index`; `arora-registry-remote` with semio-db; semio-cli's blob download | `arora-index` | Studio v2 is where assets are published, versioned and granted. Its identity is the scoped name, the release and the content hash; semio-db's is a UUID and a folder path, which Studio's sharing design replaces at the distribution boundary. |
| Holding frozen records in a process | `arora-registry`'s `LocalRegistry`; the `feat/type-registry` `Registry`; `ty::low::TypeRegistry` | `arora-registry` for records | One record store, used for authoring (tag, freeze) and, read-only, by a device. `ty::low::TypeRegistry` is the header vocabulary's, and stays on its own axis ([design note](design_decisions.md#two-type-vocabularies-two-axes)). The `records.json` name index is not needed: a record carries its name. |
| A file layout for records | `arora-registry`'s `records/` YAML directory; `records.json`; the store's frozen wire shape | `records/` with the store's frozen serde shape | The frozen serde shape is already the wire contract ([design note](design_decisions.md#the-frozen-serde-shape-is-a-wire-contract)). JSON and YAML carry it alike. |
| Where a device keeps modules | `<device dir>/modules/`; `arora-index`'s `<device dir>/assets/` cache | Both, for different sources | `modules/` is what an operator installs by hand, unverified. `assets/` is content-addressed and verified. A configuration's modules stay in the cache, by hash; they are never copied into `modules/`. |
| Publishing from a terminal | semio-cli (semio-db); the CLI session of ARORA-165 | The CLI that hosts ARORA-165's session | Publishing a module asset needs the Studio identity that session provides. semio-cli talks to semio-db only. |
| Signing in | semio-cli's username/password file; `arora-index`'s `TokenSource`; the bridge client's credentials | `TokenSource`, fed by the device credential (Stage 2.1) or the CLI session | One token source per identity serves the resolver and the bridge ([Studio v2 design](studio-v2-models-and-devices.md), decision 13). |
| Release selection | `arora-index` (Rust) and Studio's `resolveDependency` (TypeScript) | Both, by design | A device and a project must select the same release (decision 16). Shared fixtures would keep them equal. |

`arora-registry-remote`, semio-db and semio-cli stay where they are used today
(module authoring against a record store). No device path depends on them, and
this proposal adds none.

---

## 2. Loading a module with its records, on what exists

This part needs no store, no protocol change and no configuration. It unblocks
STUDIO-347 on both kinds of devices.

### 2.1 The runtime

- **`AroraBuilder::with_type_records(records)`**, repeatable. `records` is a
  read-only set of frozen records keyed by id and version: module records,
  structures, enumerations. It lives in `arora-registry`, beside
  `LocalRegistry`, and needs no folder tree: a device looks records up by exact
  id and version, it never tags or resolves a path.
- **At `build`, a guest module whose module record is in the set is indexed
  from it.** The record is the one at the module's id and the header's
  version. Each export of the header must have the same id, name and parameter
  ids in the record, and each frozen type must name the id the header's
  `TypeRef` names. A record that does not match its header fails the build,
  naming the export: a wrong record is a packaging error, not a reason to
  index less. A guest module without a record keeps today's primitive-only
  indexing and warning.
- **Every record reference a module record depends on must be in the set**,
  so a client that reads the device's records can decode what it returns. A
  missing dependency fails the build.
- **The runner reads records from the module directory.** A module directory
  may carry `records/` beside `header.json`, in `arora-registry`'s layout;
  `module_dir::read` returns them with the header and the artifact, and the
  runner hands them to the builder.

  ```text
  <module directory>/
  ├── header.json
  ├── animation.wasm
  └── records/
      ├── module/<module id>@4.0.0.json
      └── structure/<StepValues id>@1.0.0.json …
  ```

- **`arora-web` and `studio-arora`** gain the same input:
  `withTypeRecords(json)` on the browser builder.

The header does not change, and every export still goes through the method
index. The record is a second form of the module's interface that a store
serves; the device uses it the way `with_declared_module` uses a declaration.

### 2.2 The producers

- **Rust declarations.** The derive produces a structure or enumeration
  record for each `AroraType` (`AroraType::record()`), at the version
  `arora_type_version()` already returns, so a declared module can export the
  closure of its records. Today the derive produces only the header
  vocabulary's `Type`.
- **An export step** writes `records/` beside the header: for a declared
  module from `AroraModule::record` and the derived types; for a `module.yaml`
  module from `arora-module-cli`'s analysis, which already produces both.
- **vizij-animation-module** exports its records with its header, and
  `@vizij/animation-module` ships them in `artifact/records/`; its loader
  returns them beside `headerJson` and `wasmBytes`.

### 2.3 STUDIO-347 on this path

- **Studio's page.** `loadStudioWorld` reads the records with the header and
  the wasm, compiles the module once, and builds each emulated device with
  `with_compiled_module` and `with_type_records`. `step_values` and
  `player_states` join the index; the stepping graph's `spawn_graph` is
  accepted.
- **The `arora` runner.** The operator installs the module directory from
  the npm package's `artifact/`, records included, under
  `<device dir>/modules/animation/`. Studio pairs with the device, finds
  `step_values` in `DescribeMethods`, and spawns the same graph over the
  Studio Bridge.

---

## 3. Configurations

### 3.1 What a configuration holds

A configuration says what a device runs on top of its hardware: which modules,
with which type records, running which behaviors.

| In | Why |
|---|---|
| **Modules**, each pinned by scoped name, version and the content hashes of its artifacts | A configuration means exactly these bytes. |
| **Type records**: the records every module's interface carries, and records a configuration adds for its behaviors' own types | The method index and every client need them to type the calls. |
| **Behaviors**, inline, each a named graph with its graph type and version | A behavior is project content, small, and edited in Studio. Its type and version say which interpreter runs it ([behavior languages](proposal-behavior-languages.md) 3.6). |
| **Requirements**: executors, interpreters with their versions, and optionally a model family | A device refuses a configuration it cannot run before fetching anything. |

| Out | Why |
|---|---|
| **The HAL and its config** | A device's HAL and models are fixed for its life ([HAL components](proposal-hal-components.md)); changing them means restarting Arora. A configuration may *require* a family; it never changes one. |
| **Bridges and their settings** | The configuration arrives over a bridge. A configuration that changes its own channel can lose the device. |
| **Key values** | Keys belong to the HAL and the modules that describe them. A behavior that needs a starting value writes it. |
| **Host modules** | They are compiled into the device's binary; a configuration cannot add or remove one. |
| **Live edits** | `edit_graph` and task runs after the apply are commands, not configuration. `DescribeConfiguration` reports what was applied, and the behavior module's `branches()` what runs now. |

### 3.2 The manifest

A configuration is one JSON document. Its serde shape is a wire contract like
the frozen records'.

```json
{
  "format": "1.0.0",
  "requires": {
    "executors": ["wasm"],
    "interpreters": { "behavior-tree": "^1.1.0" },
    "family": null
  },
  "modules": [
    {
      "scopedName": "@vizij/animation-module",
      "version": "4.0.0",
      "artifacts": {
        "interface": "<sha-256 of the interface artifact>",
        "wasm": "<sha-256 of the wasm executable>"
      }
    }
  ],
  "records": [],
  "behaviors": [
    {
      "name": "animation-stepping",
      "graphType": "behavior-tree",
      "graphVersion": "1.1.0",
      "graph": { "…": "…" }
    }
  ]
}
```

- **Pins, never ranges.** A configuration as authored in a project names
  ranges, as a project's `dependencies` do; publishing it, or sending it from
  Studio, locks every range to the release Studio's `resolveDependency`
  selects, with its hashes. A device never selects a release for a
  configuration, so a device and the Studio that sent the configuration run
  the same bytes.
- **`records`** holds records the configuration adds, in the frozen shape.
  Most configurations carry none: each module's interface brings its own.
- **The record set must be closed.** Every record reference reachable from a
  module record or a behavior resolves inside the configuration. One record id
  and version with two different contents refuses the configuration.

### 3.3 Identity, versions and storage

Configurations and modules are Studio v2 assets, with the identity, releases,
grants and resolution every other asset has.

- **Asset types `module` and `configuration`** join `model`, `animation` and
  `face`, in Studio and in `arora-index`'s `AssetType`.
- **A module release has an artifact per role**, each with its own content
  hash, as Studio's sharing design places them:
  - `interface`: the header and the frozen records, one JSON document. It is
    small and inspectable, so a project can show a module's functions without
    the executable.
  - one executable per executor: `wasm` first; native libraries per target
    later.
- **A configuration release** has one artifact, the manifest. Its content hash
  is the configuration's identity on a device.
- **A configuration that is not published** — Studio sending a project's
  current state — is the same manifest, inline. Its identity is the hash of
  its canonical JSON.
- **The cache is `arora-index`'s.** Every artifact lands in
  `<device dir>/assets/blobs/` by hash, sealed when private. A configuration
  adds no cache of its own.

### 3.4 The bridge operations

Two operations join `BridgeOp`:

```rust
pub enum BridgeOp {
    // … Get, Update, Call, ListKeys, ListMethods, DescribeMethods
    /// Replace what the device runs with a configuration.
    ApplyConfiguration(ConfigurationRequest),
    /// Describe the configuration the device runs.
    DescribeConfiguration,
}

pub struct ConfigurationRequest {
    /// The configuration to run.
    pub configuration: ConfigurationSource,
    /// Apply only if the device runs this configuration now (its content
    /// hash), so a client never replaces one it has not seen. `None` applies
    /// over any.
    pub replaces: Option<String>,
}

pub enum ConfigurationSource {
    /// A published configuration release, pinned.
    Published { scoped_name: String, version: String, content_hash: String },
    /// A manifest sent whole.
    Inline(Configuration),
}
```

Both reply, like every operation, with a `CallResult` whose `ret` encodes over
the value plane:

```rust
pub struct ConfigurationReport {
    /// What happened to this request.
    pub outcome: ConfigurationOutcome,
    /// What the device runs after it: the identity and the pins.
    pub running: Option<RunningConfiguration>,
}

pub enum ConfigurationOutcome {
    Applied,
    /// Nothing changed. `stage` names the step that refused it.
    Refused { stage: ConfigurationStage, reason: String },
    /// `DescribeConfiguration`'s answer.
    Described,
}

pub enum ConfigurationStage {
    Requirements, Resolve, Fetch, Verify, Records, Load, Index, Behaviors, Busy, Replaced,
}
```

`Replaced` refuses a request whose `replaces` is not the configuration the
device runs. `Err(String)` stays what it is for every operation: the request could not be
decoded or reached nothing. A refusal is an answer, so a client can show its
stage and reason.

**Why an operation, and not a module function.** A call is dispatched by the
engine, and applying a configuration replaces the engine. The operation is
answered by the runtime around the engine, as `ListKeys` and `DescribeMethods`
are. It also gives the router an operation of its own to grant, apart from
calls.

**How it composes with the other operations.**

- After `Applied`, `DescribeMethods` lists the new modules' functions with
  their record signatures. A client that cached the method list reads it again.
- `ListKeys` and `Get` read the configuration's progress from the store
  (3.6); the Studio Bridge's state sync carries it like any key.
- `Call` to the interpreter module still loads, edits, spawns and halts
  behaviors. A configuration's behaviors are where a device starts; calls
  change it from there.
- `Update` is untouched.

### 3.5 What applying does

The device applies one configuration at a time. A second request while one is
being applied is refused at `Busy`.

1. **Requirements.** The device checks the executors, the interpreters and
   their versions, and the family against its own. Nothing is fetched yet.
2. **Resolve and fetch.** For each pin the device asks its asset source for
   the artifacts: `arora-index` on a native device, with the device's own
   token source; the page's own store client in Studio. Each artifact is
   verified against the pinned hash and stored in the cache.
3. **Prepare beside the running engine.** The device compiles each module,
   builds the record set, builds a new method index from the module records
   (2.1), and lowers each behavior against that index. Any failure here
   refuses the configuration, with the old one untouched.
4. **Swap between two steps.** The device halts every task run (each reports
   halted through its status key), drops the old engine with its guest
   modules, installs the new one with the host modules (the HAL module, the
   interpreter module), and loads the behaviors. If a behavior does not load,
   the old engine, kept until this point, is restored, and the configuration
   is refused at `Behaviors`.
5. **Persist.** The device writes the manifest to
   `<device dir>/configuration/applied.json` (a temporary file, then a
   rename), then replies `Applied`.

What it keeps and what it drops:

| Kept | Dropped |
|---|---|
| The HAL, its components and models | Guest modules and their state (an animation module's loaded animations) |
| Bridges, their sessions and the device's claim | Behaviors and task runs |
| The store: keys, values, key meta | The method index (rebuilt) |
| Modules from `modules/` and `--module` | |

**Replace, then diff.** The first version replaces the whole guest module set:
each configuration gets a fresh engine, the way a device is rebuilt after it is
dropped. A diff that keeps the instance of a module present in both
configurations, with its state, needs the executors to unload a module (the
wasm executor's `unload_module` is unimplemented today). That is a later stage,
and depends on the module-unloading work.

**Operator modules.** Modules from `modules/` and `--module` are the device's
base set, present in every configuration. A configuration that names a module
id already in the base set is refused at `Load`, naming both: one module id,
one module ([design note](design_decisions.md#a-device-carries-its-modules-as-module-directories)).

**At start.** A device with an `applied.json` applies it before its first
step, resolving each pin again: a live answer re-checks the device's
entitlement, a network error uses the cache, and a definitive "no" clears the
pin (`arora-index` decision 15). A configuration that no longer applies at
start leaves the device running its base set, and reports the refusal (3.6).

### 3.6 Progress, failure and the current configuration

The runtime writes three keys under its reserved `arora/` prefix, closed to
remote writes:

| Key | Holds |
|---|---|
| `arora/configuration/running` | The running configuration's identity: content hash, and scoped name and version when published. Unit when none. |
| `arora/configuration/pending` | While a configuration is being applied: its content hash and the current stage. Unit otherwise. |
| `arora/configuration/refused` | The last refusal: content hash, stage and reason. |

The reply to `ApplyConfiguration` is final: `Applied` or `Refused`. Fetching
can outlast a query's timeout; a client that loses the reply reads these keys,
which the Studio Bridge already syncs. `DescribeConfiguration` returns the
whole manifest with its pins, which a key would hold poorly.

### 3.7 Who may apply a configuration

- **The claim is the gate.** The router grants `ApplyConfiguration` to the
  device's claimant, as it grants calls and writes; it grants
  `DescribeConfiguration` to whoever may read the device. The device adds no
  access rule of its own. On the open router, which authenticates nobody,
  anyone who reaches the device may apply one, as anyone may call it today.
- **The device resolves with its own identity, never the sender's.** A
  claimant cannot make a device fetch an asset its owner may not use, and
  sends no token. Until the device credential exists (Stage 2.1 and 2.3 of
  the [Studio v2 design](studio-v2-models-and-devices.md)), a device resolves
  public assets only.
- **No executable travels inline.** An inline manifest carries behaviors and
  records; every module is a published, hash-pinned asset. Development keeps
  `--module` for unpublished modules.
- **Private bytes stay sealed**, in the cache `arora-index` keeps. A device
  never serves a configuration's module bytes; `DescribeConfiguration` returns
  pins.

### 3.8 Release and rollout

- **`arora-bridge` major (7.0.0).** `BridgeOp` gains two variants and is
  matched exhaustively by the runtime and every bridge. The same major can
  mark it `#[non_exhaustive]` so a later operation is a minor (open question).
- **`arora-bridge-ws` and `arora-bridge-ros2`.** The local control panel may
  offer apply for development; the ROS 2 bridge has no such service and does
  not produce the operations.
- **studio-bridge.** `AroraOp` gains the two variants and `CommandOp` two
  chunks (`apply-configuration`, `describe-configuration`): a
  `studio-bridge-msgs` major, then majors of `arora-studio-bridge-client` and
  `studio-bridge-studio-client`, then `arora` re-pins the device client. A
  release in two repos.
- **The router** grants the new chunks to the claimant. The grant is compiled
  with the others.
- **Negotiation.** A device that answers the new operations announces
  protocol revision 3 (`mgmt/{device}/liveliness/protocol/3`). The studio
  client sends them only to a device announcing 3 or more. Studio shows an
  older device as one that cannot be configured remotely, and keeps using it
  as today: calls to the modules its operator installed. A new device answers
  an older client exactly as before.

---

## 4. Studio

### 4.1 Emulated devices

An emulated device is the same runtime as a real one, behind a loopback
bridge. Studio applies a configuration to it with the same
`ApplyConfiguration` on that bridge. The runtime's asset source is a seam:
in the page it is Studio's own store client, which already holds the bytes and
the records, so an emulated device fetches nothing over HTTP and needs no
`arora-index` (which is native).

### 4.2 Real devices

Studio claims the device, checks its protocol revision, and sends the project's
configuration inline: module pins locked from the project's `resolved`, and
the project's behaviors. The device resolves the pins with its own identity.

### 4.3 STUDIO-347, end to end

1. vizij-rs publishes `@vizij/animation-module` 4.0.0 as a module asset: the
   `wasm` artifact and the `interface` artifact (header, module record, and
   the records of `StepValues`, `OutputKeys`, `PlayerState`, `InstanceState`,
   `AnimationClip` and the types they reference).
2. A Studio project depends on `@vizij/animation-module ^4.0.0`, resolved to
   4.0.0. Studio builds the configuration: that pin, and the
   `animation-stepping` behavior (`behavior-tree` 1.1).
3. **Emulated device:** Studio applies it on the loopback bridge; the page's
   asset source hands the cached bytes and interface; `step_values` is indexed
   from the module record; the behavior loads.
4. **Real device on the `arora` runner:** Studio applies it over the Studio
   Bridge. The device resolves the public pin through `arora-index`, fetches
   and verifies both artifacts, prepares the engine, swaps, and replies
   `Applied`. Studio reads `DescribeMethods` again and finds `step_values`.
5. When the output key table changes, Studio edits the running behavior with
   a call (`EDIT` today, `edit_graph` on the `animation-stepping` branch once
   branches exist); the configuration is unchanged.
6. The device restarts: it applies `applied.json` again, from the cache when
   offline.

---

## 5. How this composes with `arora-index`

`arora-index` stays the one component that finds and fetches published
artifacts. This proposal uses it as it is, and asks for these additions:

- **`AssetType::Module` and `AssetType::Configuration`.**
- **Fetching a named artifact.** A module release has several artifacts; a
  configuration pins each by role and hash. `fetch(&pin, artifact_key)` and a
  pin that carries each artifact's hash cover it.
- **Resolving an exact pin** is a range of one version; the resolver's
  verification, cache, sealing and fallback apply unchanged.
- **The data model without the transport.** `ResolvedRef`, `AssetType`,
  versions and ranges are what the manifest and Studio's page need, in wasm.
  Keeping them buildable without `reqwest` and `tokio`'s file system (a
  default `resolver` feature) lets the browser runtime share them.
- **The runtime resolves, never a module or a behavior**, as decision 13
  states for HALs.

Decision 9 of the Studio v2 design merges the resolver into `arora-registry`
once it covers all asset types. Module and configuration assets are those
types; the device's record set (2.1) is the part of `arora-registry` the merge
keeps for devices.

---

## 6. Stages

Each stage lands on its own.

1. **Records beside headers** (section 2). `arora-registry`: the read-only
   record set. `arora`: `with_type_records`, indexing from module records,
   `records/` in module directories. `arora-types-derive`: structure and
   enumeration records. `arora-web`: `withTypeRecords`. vizij-rs: export and
   ship the records. Studio: pass them. Unblocks STUDIO-347.
2. **Module assets.** Studio: the `module` asset type and the `interface` and
   `wasm` roles. `arora-index`: the asset type and named artifacts. A
   publishing command in the CLI that hosts ARORA-165's session.
3. **Configurations in the runtime.** The manifest type, the asset source
   seam, apply and describe as runtime functions (`Arora::apply_configuration`),
   `applied.json` and the start-up apply, and `--configuration` on the runner
   (a published pin or a manifest file). No protocol change.
4. **The bridge operations** (3.4, 3.8): `arora-bridge` 7, the studio-bridge
   chain, the router grant, protocol revision 3, and Studio's apply.
5. **Diff apply**, keeping unchanged module instances; needs module
   unloading. Behaviors as named branches once the behavior module of
   [behavior languages](proposal-behavior-languages.md) lands; until then a
   configuration carries at most one behavior, loaded with `LOAD`.

---

## 7. Open questions

- **Type-only assets.** Should records shared by several modules, or used
  only by behaviors, be assets of their own (`types`), pinned by a
  configuration, rather than copies in each interface?
- **`#[non_exhaustive]` on `BridgeOp`** in the same major, so the next
  operation is a minor, at the cost of a compile-time check in each bridge.
- **A configuration module already in the base set**: refuse (proposed), or
  let the configuration replace it?
- **Serving records.** Should a device answer a request for the records it
  holds, for a client that did not send the configuration, or is the store the
  only source?
- **Inline executables for development**, behind a device flag like `--open`?
- **Model family in `requires`**: required, or left to Studio's binding check?
- **`arora-index` questions**: see section 5.

---

## 8. Design decisions

### The header does not carry frozen types

A header is the module's identity and its unversioned interface; the record is
its versioned, frozen form, and the store serves it. Freezing types into the
header would make two frozen copies of the interface that can disagree, and
change a format every module generator writes. The device takes the record,
and checks it against the header instead.

### Records go through the method index

A record-typed export joins the method index like any other, so
`DescribeMethods`, the behavior tree's lowering and dispatch see the same
functions. A side table of record signatures beside the index would let a
behavior call what a remote cannot discover.

### Pins, not ranges, in what a device applies

A device that selects releases could run different bytes from the Studio that
sent the configuration, and from another device with the same configuration.
Ranges belong to authoring; publishing and sending lock them.

### The device fetches; the sender sends references

Sending bytes over the bridge would make a module's size a query's payload,
bypass the hash a release pins, and let a claimant hand a device bytes its
owner is not entitled to. The device's own identity and cache make the result
the same whoever sent the request, and let it restart offline.

### Replace before diff

Replacing the whole guest module set needs only what the runtime already does
when a device is dropped and built again. A diff that keeps module state needs
module unloading in every executor, and the replace path stays the fallback
for anything a diff cannot express.

### An operation, not a module function

See 3.4: a call is dispatched by the engine an apply replaces, and the router
grants operations separately.

### Alternatives considered

- **A configuration as a project export Studio pushes call by call.** It is
  what Studio does today for one module it assumes is installed. It cannot add
  a module, is not atomic, and leaves nothing a device can restore at start.
- **semio-db as the record store devices read.** It keys records by UUID and
  folder; Studio's assets are where modules are published and granted, and a
  second identity for the same artifact would drift.
- **The HAL in the configuration.** A HAL's models and components are fixed
  for its life; changing them is a restart, which a configuration cannot
  express without dropping its own bridge.

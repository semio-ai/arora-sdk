# Proposal: configurations — modules, type records and behaviors applied together

Status: draft, for review.
Date: 2026-10-16.

A device runs modules and behaviors. Today each reaches it by a separate path:
an operator copies a module directory into the device directory, a client
pushes a behavior through the interpreter module, and nothing carries the type
records a module's functions name. This document does two things:

- **Sections 1–2** review what exists for distributing and loading modules and
  types, where it overlaps, and the smallest change that lets a device index a
  guest module's record-typed exports with what exists.
- **Sections 3–6** propose a **configuration**: a record that references the
  modules, type records and behaviors a device runs, and one bridge operation
  that tells a device to run it.

In short:

- **Everything a device runs is a record.** Modules, structures and
  enumerations already are; behaviors and configurations become record kinds
  too. Records are versioned and frozen, held in `arora-registry`, and kept by
  the store's backend (semio-db) as versioned assets. Releases, ranges and
  content hashes are record metadata.
- **A device resolves through its own `arora-registry`**, filled from a
  **registry source**: local record directories, semio-db
  (`arora-registry-remote`), or Studio. arora-sdk knows only the source trait;
  the Studio source lives in studio-bridge's client, which already owns the
  connection to Studio, the credentials and the device's cache directory.
- **A guest module is indexed from its module record.** The header stays as it
  is; the record, checked against it, gives every export its frozen signature
  in the method index. This unblocks behaviors calling `step_values`
  ([STUDIO-347](https://linear.app/semio-ai/issue/STUDIO-347)) before any
  store or protocol work.
- **A configuration is a frozen record** that pins module records, type
  records and behavior records, with requirements on the device. HAL and
  bridge settings are not part of it.
- **One remote surface:** `ApplyConfiguration` changes what a device runs, and
  `DescribeConfiguration` returns it. There is no remote operation to load,
  reload or unload a single module.
- **Applying is a diff, between steps.** Unchanged modules keep their instance
  and state; changed ones are reloaded, removed ones unloaded, new ones
  loaded. Behaviors keep running across a reload. A configuration that does
  not apply leaves the old one running.
- **The claim is the gate.** The router grants the new operations to the
  device's claimant, as it grants calls; the device adds no access rule.
- **The cost is a two-repo release**: an `arora-bridge` major, an
  `arora-types` major for the new record kinds, a `studio-bridge-msgs` major
  with its client chain, a router grant, and a protocol revision a device
  announces, so a client never sends the new operations to a device that
  cannot answer them.

---

## 1. What exists

### 1.1 Inventory

| Component | Where | What it does | Used by | Status |
|---|---|---|---|---|
| Type records | `arora-types`, `record` | Versioned declarations — structures, enumerations, modules, folders — unfrozen (version requirements) or frozen (pinned versions); `Freeze` and `Resolver` pin them. The frozen serde shape is the store's wire contract. A module record carries an `executable` id, attached at publication. | Registries, module authoring, the behavior tree | Merged |
| `arora-registry` 2.0 | arora-sdk | Local record registry: adds, tags (freezes) and looks up records by id or folder path and version requirement; loads record directories (`records/{folder,enumeration,structure,module}/<uuid>@<version>.yaml`) and in-memory records (`load_records`). | `arora-module-cli` and its generators, `arora-cli`, `arora-behavior-tree`'s build | Merged, published |
| `arora-registry-remote` 1.0 | arora-sdk | `RemoteRegistry` reads records from semio-db through `semio-client`; `RemoteCachedRegistry` puts a `LocalRegistry` in front of it as a cache. Reads the semio-cli login file. | `arora-cli`, `arora-module-cli` | Merged, private |
| semio-db, semio-client, semio-record, semio-store-rpc | separate repos | GraphQL record store on ScyllaDB: records by id and version, semver tags with a `yanked` flag, the folder tree, path-to-id names; a WebSocket blob store; their client. | semio-cli, `arora-registry-remote` | Maintained, tested end to end; no production deployment found |
| semio-cli | semio-cli | CLI over semio-db: create, tag and read records per kind, blob upload and download, login. | Developers | Maintained |
| `arora-index` | arora-sdk, `crates/arora-index` | Resolves a published Studio v2 asset `{scopedName, range}` to a release with Studio's selection rules; fetches its primary artifact verified by SHA-256; content-addressed cache with sealed private blobs and `resolved.json`; `resolve_with_fallback`; `build::models()`. Reads Firestore and Storage directly. | Nothing yet | [#360](https://github.com/semio-ai/arora-sdk/pull/360), open, `publish = false` |
| `feat/type-registry` | arora-sdk branch | Emitted `records/` and a `records.json` id→name index per module; a wasm `Registry` in `arora-web` resolving ids to names. | Nothing | Unmerged; the crate it changes no longer exists |
| Module directories | `arora`, `module_dir` | A device loads every `<device dir>/modules/*/` (a `header.json` beside one artifact) at start, and each `--module` directory. | The `arora` runner | Merged |
| Guest module indexing | `arora`, `module_discovery` | A header-loaded guest export joins the method index only when its parameters and return are primitives or optional scalar primitives. `with_declared_module` indexes every export from a Rust declaration. | Every device | Merged (arora 12.13) |
| Module reload | arora-sdk branch `spike/unload-modules` | `Arora::load_module`, `reload_module`, `unload_module` on a running device, between steps; a shared, mutable method index; the engine's `reload_module`, `unload_module`, `contains_module`. | Nothing yet | Spike, becoming a PR |
| Module declarations | `arora-module`, `arora-module-macros` | A Rust declaration produces the header, the frozen **module record** (`AroraModule::record`), the exports and client stubs. Derived types produce a `ty::low::Type`, not a structure record. | polly, test-rust-wasm, vizij modules | Merged |
| `module.yaml` pipeline | `arora-module-cli`, `arora-module-authoring` | Analyzes a `module.yaml` against a registry into a frozen module record and frozen type records, then generates C++/Rust bindings. | C++ modules, importing Rust modules | Merged |
| `arora-web` | arora-sdk | Browser runtime: `AroraRuntimeBuilder.withModule(headerJson, bytes)`, `withCompiledModule`. No records. | arora-sdk demos | Merged |
| Bridge operations | `arora-bridge` 6.1 | `Get`, `Update`, `Call`, `ListKeys`, `ListMethods` (deprecated), `DescribeMethods`. Behaviors travel as calls to the interpreter module (`LOAD`, `EDIT`, `SPAWN`, `HALT`). Nothing carries modules or records. | Every bridge | Merged |
| Studio Bridge | studio-bridge | `AroraOp` mirrors `BridgeOp` (msgs 6.x); sender-addressed command keys `rpc/{device}/{principal}/{op}/{target}` the router authorizes per operation; protocol token `mgmt/{device}/liveliness/protocol/2`; the device's Studio credentials. | Studio, `arora --features studio-bridge` | Merged |
| Studio v2 assets | semio_studio | `scopedNames`, `publishedAssets/{id}/releases`; a release has `artifacts: {key → {storage, contentHash, role}}`; asset types `Model`, `Animation`, `Face`; `resolveDependency`. No module, type or configuration asset. | Studio | Merged |
| `studio-arora` | semio_studio, Studio-on-Arora branches | Studio's wasm crate on arora 12.13. Loads `@vizij/animation-module` (header and wasm from npm) once as a `CompiledModule`; builds each emulated device with `with_compiled_module`; pushes the stepping behavior by `spawn_graph`, relinks it by `edit`. | Studio | Branch |
| `vizij-animation-module` | vizij-rs | The animation module, declared in Rust; the npm package `@vizij/animation-module` 4.0.0 ships its header beside the wasm. Its declaration produces the module record; nothing exports it. | Studio, vizij devices | Merged, published |

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

The frozen signatures exist: the declaration produces them as the module
record (`AroraModule::record`), the form a registry holds. What is missing is
a registry on the device, and records to fill it.

### 1.3 Overlaps, and which component owns each concern

| Concern | Overlapping components | Owner | Why |
|---|---|---|---|
| What a published thing is | Records (`arora-types`); Studio v2 assets and `arora-index`'s `AssetType` / `ResolvedRef` | Records | A record is already versioned, frozen and referenced by id and version; a module record is what a declaration and the authoring pipeline produce. Release metadata — content hashes, deprecation (`yanked`), the scoped name — attaches to a record version instead of forming a second vocabulary. |
| Holding records in a process | `arora-registry`'s `LocalRegistry`; the `feat/type-registry` `Registry`; `arora-index`'s `resolved.json` | `arora-registry` | One registry for authoring (tag, freeze) and for devices (exact lookup). A record carries its name, so the `records.json` index is not needed. `ty::low::TypeRegistry` stays on the header vocabulary's axis ([design note](design_decisions.md#two-type-vocabularies-two-axes)). |
| Where records come from | `arora-registry-remote` (semio-db); `arora-index` (Studio v2); record directories | A **registry source** trait in `arora-registry`, one implementation per origin | `RemoteCachedRegistry` already pairs a local registry with a remote; generalized, a device's registry is a `LocalRegistry` in front of any source. semio-db is the store's backend; Studio's source lives in studio-bridge's client. `arora-index`'s selection, verification and sealing become that source's implementation. |
| Connecting to Studio, credentials | `arora-index`'s `FIREBASE_*` configuration and `TokenSource`; studio-bridge's client | studio-bridge's client | Arora works without Studio. The bridge client already holds the Studio connection and the device's credentials. |
| The device's cache directory | `arora-index`'s `<device dir>/assets/`; the Studio Bridge client's directory; `<device dir>/modules/` | One cache, in the device directory, shared with the bridge client | Two cache roots for one device drift. Operator-installed `modules/` stays a separate, unverified input. |
| Publishing | semio-cli (semio-db); Studio's web publishing | semio-cli for records, and the store's backend | semio-cli already creates and tags records and uploads blobs. |
| Release selection | `arora-registry`'s `resolve_tag`; `arora-index`; Studio's `resolveDependency` | `arora-registry`'s resolution, with the source supplying versions and metadata | One rule for a range: the newest tagged, non-yanked version that satisfies it. A source that mirrors Studio maps deprecation to `yanked`. |

---

## 2. Loading a module with its records, on what exists

This part needs no store, no protocol change and no configuration. It unblocks
STUDIO-347 on both kinds of devices.

### 2.1 The runtime

- **`AroraBuilder::with_registry(registry)`** gives the device a
  `LocalRegistry`. The builder looks records up by exact id and version, which
  the registry answers from memory; it never tags or resolves a path.
- **At `build`, a guest module whose module record is in the registry is
  indexed from it.** The record is the one at the module's id and the
  header's version. Each export of the header must have the same id, name and
  parameter ids in the record, and each frozen type must name the id the
  header's `TypeRef` names. A record that does not match its header fails the
  build, naming the export: a wrong record is a packaging error, not a reason
  to index less. A guest module without a record keeps today's primitive-only
  indexing and warning.
- **Every record a module record depends on must be in the registry**, so a
  client reading the device's records can decode what it returns. A missing
  dependency fails the build.
- **The runner fills the registry from module directories.** A module
  directory may carry `records/` beside `header.json`, in `arora-registry`'s
  layout; the runner loads it with `load_records_from_yaml_dir`.

  ```text
  <module directory>/
  ├── header.json
  ├── animation.wasm
  └── records/
      ├── folder/<folder id>.yaml
      ├── module/<module id>@4.0.0.yaml
      └── structure/<StepValues id>@1.0.0.yaml …
  ```

- **`arora-web` and `studio-arora`** gain the same input: `withRecords(…)` on
  the browser builder, loading in-memory records with `load_records`.
  `arora-registry` builds for `wasm32-unknown-unknown`.

The header does not change, and every export still goes through the method
index. The device uses the record the way `with_declared_module` uses a
declaration.

### 2.2 The producers

- **Rust declarations.** The derive produces a structure or enumeration
  record for each `AroraType` (`AroraType::record()`), at the version
  `arora_type_version()` already returns, so a declared module exports the
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
  `with_compiled_module` and the records. `step_values` and `player_states`
  join the index; the stepping graph's `spawn_graph` is accepted.
- **The `arora` runner.** The operator installs the module directory from the
  npm package's `artifact/`, records included, under
  `<device dir>/modules/animation/`. Studio pairs with the device, finds
  `step_values` in `DescribeMethods`, and spawns the same graph over the
  Studio Bridge.

---

## 3. Configurations

### 3.1 What a configuration holds

A configuration says what a device runs on top of its hardware.

| In | Why |
|---|---|
| **Module records**, pinned by id and version, each with its executables | A configuration means exactly these modules. |
| **Type records** the module records and behaviors depend on | The method index and every client need them to type the calls. |
| **Behavior records**, each a named graph with its graph type and version | A behavior is versioned like a module. Its type and version say which interpreter runs it ([behavior languages](proposal-behavior-languages.md) 3.6). |
| **Requirements**: executors, interpreters with their versions, optionally a model family | A device refuses a configuration it cannot run before fetching anything. |

| Out | Why |
|---|---|
| **The HAL and its config** | A device's HAL and models are fixed for its life ([HAL components](proposal-hal-components.md)); changing them means restarting Arora. A configuration may *require* a family; it never changes one. |
| **Bridges and their settings** | The configuration arrives over a bridge. A configuration that changes its own channel can lose the device. |
| **Key values** | Keys belong to the HAL and the modules that describe them. A behavior that needs a starting value writes it. |
| **Host modules** | They are compiled into the device's binary. |
| **Live edits** | `edit_graph` and task runs after the apply are commands. `DescribeConfiguration` reports what was applied, and the behavior module's `branches()` what runs now. |

### 3.2 The records

Two record kinds join `RecordType`, each with an unfrozen and a frozen form,
like a module:

```rust
// record::behavior::frozen
pub struct Behavior {
    pub parent: Uuid,
    pub name: String,
    pub graph_type: String,          // "behavior-tree"
    pub graph_version: Version,      // the format's version
    pub graph: Graph,
    pub dependencies: Vec<FrozenReference>,  // the module records it calls
}

// record::configuration::frozen
pub struct Configuration {
    pub parent: Uuid,
    pub name: String,
    pub requires: Requirements,      // executors, interpreters, family
    pub modules: Vec<FrozenReference>,
    pub behaviors: Vec<FrozenReference>,
    pub types: Vec<FrozenReference>, // records the others do not already bring
}
```

- **Freezing locks a configuration.** As authored, a configuration names
  version requirements (the unfrozen form); freezing it against a registry
  pins each to the newest tagged version that satisfies it, with the existing
  `Freeze` and `Resolver`. A device applies only frozen configurations, so a
  device and the client that sent the configuration run the same records.
- **Freezing checks the behaviors against the modules.** Each function a
  behavior calls must be an export of a module record the configuration pins,
  with a matching signature. The device does not repeat this check when it
  applies or reloads: a configuration carries both sides, and is checked where
  it is made.
- **The record set is closed.** Every reference reachable from the
  configuration resolves to a record its registry can supply.

**Release metadata on a record version.** A tagged version carries, besides
its frozen content:

- the content hash of each artifact, by key: for a module, the header and one
  executable per executor (`wasm`, then native targets); the module record's
  `executable` id becomes this map;
- `yanked`, which semio-db already stores per tag;
- the scoped name a source publishes it under, when it has one.

A module's published form is therefore its module record, its header and
executables as artifacts of that record version, and the type records it
depends on as records of their own.

### 3.3 Registry sources

```rust
#[async_trait]
pub trait RegistrySource {
    /// The versions of a record, with their metadata.
    async fn versions(&self, selector: &Selector) -> Result<Vec<TaggedVersion>, SourceError>;
    /// A frozen record at an exact version.
    async fn record(&self, id: Uuid, version: &Version) -> Result<Record, SourceError>;
    /// An artifact of a record version, verified against its hash.
    async fn artifact(&self, id: Uuid, version: &Version, key: &str) -> Result<Bytes, SourceError>;
}
```

- **A device's registry is a `LocalRegistry` in front of a source**, the
  shape `RemoteCachedRegistry` has today. It persists under the device
  directory (records in the `records/` layout, artifacts by content hash), in
  the directory the bridge client already uses, not a second cache root.
- **Implementations:** record directories (local, no network); semio-db
  (`arora-registry-remote`); Studio, in studio-bridge's client, with its
  connection, credentials, private-artifact sealing and offline fallback —
  what `arora-index` implements today; Studio's page, backed by its own store
  client.
- **A source authenticates as the device**, never as a remote that asks the
  device for something.

### 3.4 Naming a configuration

A configuration is named in record terms. What `arora-index` calls an asset
specification maps onto them:

| `arora-index` | Record terms |
|---|---|
| scoped name | `Selector::Path`, resolved to a record id by the registry |
| version range | `VersionReq`, in an unfrozen reference: authoring only |
| asset type | `RecordType`, implied by the operation and checked by `type_of` |
| content hash | release metadata of the record version |

The bridge operation takes the frozen form — id, exact version and the
configuration record's content hash — because a device applies only what is
frozen. A name and a range are resolved by whoever sends it (Studio, a CLI),
against the same registry.

### 3.5 The bridge operations

Two operations join `BridgeOp`. They are the whole remote surface for what a
device runs: a remote changes the device's modules and behaviors by applying a
configuration, never by loading, reloading or unloading one module.

```rust
pub enum BridgeOp {
    // … Get, Update, Call, ListKeys, ListMethods, DescribeMethods
    /// Change what the device runs to a configuration.
    ApplyConfiguration(ConfigurationRequest),
    /// Describe the configuration the device runs.
    DescribeConfiguration,
}

pub struct ConfigurationRequest {
    pub configuration: ConfigurationSource,
    /// Apply only if the device runs this configuration now, so a client
    /// never replaces one it has not seen. `None` applies over any.
    pub replaces: Option<FrozenReference>,
}

pub enum ConfigurationSource {
    /// A configuration record the device's registry resolves.
    Published { reference: FrozenReference, content_hash: String },
    /// A frozen configuration sent whole, with the records it pins that are
    /// not published: a project's behaviors as edited in Studio. Never a
    /// module's executable.
    Inline { configuration: ConfigurationFrozen, records: Vec<Record> },
}
```

Both reply, like every operation, with a `CallResult` whose `ret` encodes over
the value plane:

```rust
pub struct ConfigurationReport {
    pub outcome: ConfigurationOutcome,
    /// What the device runs after it: the configuration and its pins.
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
    Requirements, Resolve, Fetch, Verify, Prepare, Swap, Busy, Replaced, Restart,
}
```

`Replaced` refuses a request whose `replaces` is not what the device runs;
`Restart` one that would reload or unload a native module (3.6). `Err(String)`
stays what it is for every operation: the request could not be decoded or
reached nothing. A refusal is an answer, so a client can show its stage and
reason.

**Why an operation, and not a module function.** Applying reloads and unloads
modules, which runs between steps: a module function runs inside a step, and a
guest calling it would re-enter the engine. The operation is answered by the
runtime around the engine, as `ListKeys` and `DescribeMethods` are, and the
router can grant it apart from calls.

**How it composes with the other operations.**

- After `Applied`, `DescribeMethods` lists the new modules' functions with
  their record signatures. A client that cached the method list reads it again.
- `ListKeys` and `Get` read the configuration's progress (3.7); the Studio
  Bridge's state sync carries it like any key.
- `Call` to the interpreter module still edits, spawns and halts behaviors
  from where the configuration started them.
- `Update` is untouched.

### 3.6 What applying does

The device applies one configuration at a time; a second request while one is
being applied is refused at `Busy`.

1. **Requirements.** The device checks executors, interpreters and family
   against its own. Nothing is fetched yet.
2. **Resolve and fetch.** The device's registry resolves every record the
   configuration pins, through its source, and fetches each artifact it does
   not hold, verified against the hash its record version carries.
3. **Diff.** Against the running configuration, each module is *kept* (same
   id, version and executable hash), *reloaded* (same id, anything else
   changed), *unloaded* or *loaded*; each behavior is kept, replaced, halted
   or started. A configuration that would reload or unload a native module is
   refused at `Restart`: native modules are not reloaded; the operator
   restarts Arora with the configuration (3.8).
4. **Prepare.** The device compiles each new or changed module, keeping
   compiled modules it already has (instantiating a precompiled wasm module
   costs about 0.9 M instructions, compiling it about 1.5 G), builds the new
   method index from the module records (2.1), and lowers each new behavior
   against it. A failure here changes nothing.
5. **Swap, between two steps.** The device instantiates every new and changed
   module before dropping any old instance, then reloads, unloads and loads,
   and starts, replaces or halts behaviors. A behavior that is kept keeps
   running as if nothing changed: its next call reaches the new instance of a
   reloaded module. A command from a bridge that is in flight to a module
   being reloaded or unloaded is interrupted, and its caller gets an error. If
   an instance fails to start, the new instances are dropped and nothing has
   changed.
6. **Persist.** The device writes the configuration's reference (or, inline,
   its records) to `<device dir>/configuration/`, then replies `Applied`.

| Kept | Changed |
|---|---|
| The HAL, its components and models | Reloaded modules start from a fresh instance, without the old one's state |
| Bridges, their sessions and the device's claim | Unloaded modules leave dispatch and the method index |
| The store: keys, values, key meta | Replaced and removed behaviors; task runs of a removed behavior are halted |
| Unchanged modules, with their instance and state | |
| Unchanged behaviors, running | |
| Modules from `modules/` and `--module` | |

**Operator modules.** Modules from `modules/` and `--module` are the device's
base set, present under every configuration. A configuration that names a
module id in the base set is refused at `Prepare`, naming both: one module id,
one module ([design note](design_decisions.md#a-device-carries-its-modules-as-module-directories)).

**At start.** A device with a persisted configuration applies it before its
first step, through its registry: a live answer from the source re-checks the
device's access, a network error uses the cache, and a definitive "no" from
the source removes the record from the cache. A configuration that no longer
applies at start leaves the device running its base set, and reports the
refusal (3.7).

### 3.7 Progress, failure and the current configuration

The runtime writes three keys under its reserved `arora/` prefix, closed to
remote writes:

| Key | Holds |
|---|---|
| `arora/configuration/running` | The running configuration's reference and content hash. Unit when none. |
| `arora/configuration/pending` | While a configuration is being applied: its reference and the current stage. Unit otherwise. |
| `arora/configuration/refused` | The last refusal: reference, stage and reason. |

The reply to `ApplyConfiguration` is final: `Applied` or `Refused`. Fetching
can outlast a query's timeout; a client that loses the reply reads these keys,
which the Studio Bridge already syncs. `DescribeConfiguration` returns the
configuration record with every record it pins.

### 3.8 Applying without a bridge

The runtime offers the same apply as functions: `Arora::apply_configuration`
and `Arora::configuration`. The runner takes `--configuration <selector>@<version>`
or a frozen configuration file, and applies it at start; that is how a device
with native modules changes them, by restarting.

### 3.9 Who may apply a configuration

- **The claim is the gate.** The router grants `ApplyConfiguration` to the
  device's claimant, as it grants calls and writes; it grants
  `DescribeConfiguration` to whoever may read the device. The device adds no
  access rule of its own. On the open router, which authenticates nobody,
  anyone who reaches the device may apply one, as anyone may call it today.
- **The device resolves as itself.** Its source authenticates with the
  device's identity, never the sender's: a claimant cannot make a device fetch
  a record its owner may not use, and sends no token.
- **No executable travels inline.** An inline configuration carries records;
  every executable is an artifact of a published module record, verified by
  hash. Development keeps `--module` for unpublished modules.
- **Private artifacts stay sealed** in the device's cache, by the source that
  fetched them. `DescribeConfiguration` returns records, never executables.

### 3.10 Release and rollout

- **`arora-types` major.** `RecordType` gains `Behavior` and `Configuration`,
  and is matched exhaustively. The record kinds' frozen serde shapes join the
  store's wire contract, with their golden tests; semio-db accepts the new
  kinds.
- **`arora-registry` minor.** The registry source trait, exact lookup, and
  the new kinds.
- **`arora-bridge` major (7.0.0).** `BridgeOp` gains two variants and is
  matched exhaustively by the runtime and every bridge. The same major can
  mark it `#[non_exhaustive]` so a later operation is a minor (open question).
- **`arora-bridge-ws` and `arora-bridge-ros2`.** The local control panel may
  offer apply for development; the ROS 2 bridge does not produce the
  operations.
- **studio-bridge.** `AroraOp` gains the two variants and `CommandOp` two
  chunks (`apply-configuration`, `describe-configuration`): a
  `studio-bridge-msgs` major, then majors of `arora-studio-bridge-client` and
  `studio-bridge-studio-client`, then `arora` re-pins the device client. The
  Studio registry source ships in the same device client. A release in two
  repos.
- **The router** grants the new chunks to the claimant, compiled with the
  other grants.
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
`ApplyConfiguration` on that bridge. Its registry source is Studio's own store
client in the page, which already holds the records and the bytes, so an
emulated device fetches nothing.

### 4.2 Real devices

Studio claims the device, checks its protocol revision, and sends the
project's configuration: frozen against the project's resolved dependencies,
inline when its behaviors are not published. The device resolves the module
records and artifacts through its own source, as itself.

### 4.3 STUDIO-347, end to end

1. vizij-rs publishes `@vizij/animation-module` 4.0.0: its module record with
   the header and `wasm` artifacts, and the records of `StepValues`,
   `OutputKeys`, `PlayerState`, `InstanceState`, `AnimationClip` and the
   types they reference.
2. A Studio project depends on the module `^4.0.0`. Studio freezes the
   project's configuration: the module record at 4.0.0, and the
   `animation-stepping` behavior record (`behavior-tree` 1.1), checked against
   the module record's `step_values` and `player_states`.
3. **Emulated device:** Studio applies it on the loopback bridge; the page's
   source supplies the records and the compiled module; `step_values` is
   indexed from the module record; the behavior starts.
4. **Real device on the `arora` runner:** Studio applies it over the Studio
   Bridge. The device's Studio source resolves the module record and fetches
   its artifacts, the device prepares, swaps between two steps, and replies
   `Applied`. Studio reads `DescribeMethods` again and finds `step_values`.
5. When the output key table changes, Studio edits the running behavior with
   a call (`EDIT` today, `edit_graph` on the branch once branches exist); the
   configuration is unchanged.
6. vizij-rs publishes 4.1.0 with a fix. Studio applies the configuration
   frozen against it: the device reloads the animation module between two
   steps, and the stepping behavior keeps running, its next `step_values`
   reaching the new instance.
7. The device restarts: it applies its persisted configuration again, from
   the cache when offline.

---

## 5. How this composes with `arora-index`

`arora-index` implements what a Studio registry source needs: Studio's
selection rules, hash verification, sealing private artifacts, a pin used only
on a network error, and a definitive "no" that clears it. In the shape of
this proposal:

- **Its resolution and fetching become the Studio source**, behind the
  registry source trait, in studio-bridge's client beside the connection and
  credentials it already holds; arora-sdk keeps no Firestore or Storage code.
- **Its vocabulary maps onto records** (3.4): a `ResolvedRef` is a record
  version with its metadata, `resolved.json` is the device registry's cache,
  and an asset type is a record kind.
- **What the source needs to serve configurations:**
  1. several artifacts per record version, each with its own hash, fetched by
     key;
  2. an exact pin (version and hash) checked before download;
  3. the record and metadata types buildable for `wasm32` without the
     transport, so Studio's page uses the same types;
  4. an extensible set of kinds, so a new kind is not a breaking change.
- **Build pins** (`build::models()`) are a source used at build time; they
  stay a build-script helper over the same trait.

---

## 6. Stages

Each stage lands on its own.

1. **Records beside headers** (section 2). `arora`: `with_registry`,
   indexing from module records, `records/` in module directories.
   `arora-types-derive`: structure and enumeration records. `arora-web`:
   records. vizij-rs: export and ship the records. Studio: pass them.
   Unblocks STUDIO-347.
2. **Module reload** on a running device, from the module-reload work: load,
   reload and unload between steps, instantiation before any old instance is
   dropped, interrupted commands, native modules refused.
3. **Record kinds and sources.** `arora-types`: behavior and configuration
   records; release metadata on record versions. `arora-registry`: the source
   trait, exact lookup, freezing a configuration with its checks. Sources:
   record directories, semio-db.
4. **Configurations in the runtime.** `Arora::apply_configuration` with the
   diff and swap (3.6), persistence and the start-up apply,
   `--configuration` on the runner. No protocol change.
5. **The bridge operations** (3.5, 3.10): `arora-bridge` 7, the studio-bridge
   chain with the Studio source, the router grant, protocol revision 3, and
   Studio's apply.
6. **Behaviors as named branches**, once the behavior module of
   [behavior languages](proposal-behavior-languages.md) lands; until then a
   configuration carries at most one behavior, loaded with `LOAD`.

---

## 7. Open questions

- **Record identity across sources.** Records are referenced by UUID and
  version; Studio identifies an asset by scoped name and release. Is the
  scoped name a `Selector::Path` over the same UUID identity in every source
  (proposed), and how does semio-db's folder tree carry a scope?
- **Models, animations and faces as records.** The same registry would hold
  them as record kinds; this proposal needs only modules, types, behaviors and
  configurations.
- **Interrupted commands.** Does an in-flight command to a reloaded module
  fail with a distinct error a client can retry on?
- **Atomic swap of several modules.** Instantiating every new module before
  dropping any old one needs the engine to split reload into instantiate and
  commit.
- **Compiled-module cache on native devices.** Keep compiled wasm across
  restarts (by executable hash), or only in memory?
- **`#[non_exhaustive]` on `BridgeOp` and `RecordType`** in the same majors,
  so the next operation or kind is a minor.
- **A configuration module already in the base set**: refuse (proposed), or
  let the configuration replace it?
- **Serving records.** Should a device answer a request for the records it
  holds, for a client without the store?
- **Model family in requirements**: required, or left to Studio's binding
  check?

---

## 8. Design decisions

### Published things are records

Arora already has a versioned, frozen, referenceable form for what it shares:
records, held in `arora-registry`. A module record is what a declaration and
the authoring pipeline produce. Expressing releases, ranges and hashes as
record metadata keeps one vocabulary and one registry for modules, types,
behaviors and configurations; a second, asset-shaped vocabulary would have to
be translated at every boundary.

### arora-sdk knows a registry source, not Studio

Arora works without Studio. The source trait lets a device fill its registry
from local directories, semio-db or Studio alike; Studio's connection,
credentials and cache stay in studio-bridge's client, which owns them already.

### The header does not carry frozen types

A header is the module's identity and its unversioned interface; the record is
its versioned, frozen form. Freezing types into the header would make two
frozen copies of the interface that can disagree, and change a format every
module generator writes. The device takes the record and checks it against the
header.

### Records go through the method index

A record-typed export joins the method index like any other, so
`DescribeMethods`, the behavior tree's lowering and dispatch see the same
functions. A side table of record signatures would let a behavior call what a
remote cannot discover.

### Frozen configurations only on a device

A device that selects versions could run different records from the client
that sent the configuration, and from another device with the same one.
Version requirements belong to authoring; freezing locks them, and checks the
behaviors against the modules where the configuration is made.

### One remote surface for what a device runs

A remote changes modules and behaviors only by applying a configuration.
Separate load, reload and unload operations would leave a device in states no
configuration describes, which `DescribeConfiguration` could not report and a
restart could not restore.

### Apply is a diff, between steps

Reloading a module between steps keeps everything else running: unchanged
modules keep their state, and a behavior keeps running across a reload of a
module it calls. A reload never runs inside a module function, because a
guest calling it would re-enter the engine. Native modules are not reloaded;
a device with native modules changes them by restarting.

### The device fetches; the sender sends references

Sending executables over the bridge would make a module's size a query's
payload, bypass the hash a record version carries, and let a claimant hand a
device bytes its owner is not entitled to. The device's own source and cache
make the result the same whoever sent the request, and let it restart offline.

### Alternatives considered

- **A Studio asset vocabulary in arora-sdk** (asset types, resolved
  references, a Studio resolver as a crate of its own). It duplicates records
  and their registry, and puts Studio's protocol in a runtime that works
  without Studio.
- **Load, reload and unload as remote operations.** See "One remote surface".
- **Replacing the whole engine on every apply.** It drops the state of
  modules the configuration did not change.
- **A configuration pushed call by call.** It is what Studio does today for
  one module it assumes is installed. It cannot add a module, is not atomic,
  and leaves nothing a device can restore at start.
- **The HAL in the configuration.** A HAL's models and components are fixed
  for its life; changing them is a restart.

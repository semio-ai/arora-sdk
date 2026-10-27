# Proposal: configurations — modules, type records and behaviors applied together

Status: draft, for review.
Date: 2026-10-26.

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
  too. Records are versioned and frozen, held in `arora-registry`, and
  published in packages: a release of a package carries its record as an
  artifact, hashed like the others.
- **Names address, ids identify, hashes verify.** A package is looked up by
  its scoped name, `@scope/name`, which routes the request to one registry.
  It keeps an id of its own, bound by its registry at its first publish,
  through renames, transfers and moves to another registry. A release is a
  package's version, published once, with a SHA-256 per artifact. Records
  keep their UUIDs inside; nothing looks a package up by a record's UUID
  ([registry identity](registry-identity.md)).
- **Each type is a package of its own**, with its own semver. A module
  depends on the type packages it names, and a resolver picks the version of
  each type compatible with every module using it.
- **The store is the only source of records.** A device resolves through its
  own `arora-registry`, filled through a **registry source**, and does not
  serve records. arora-sdk knows only the source trait, so a backend could be
  distributed; for now the only backend is Studio's, whose source is
  Semio's `semio-studio-assets` crate, signed in through
  `semio-studio-session`. The device's cache stays in the device directory
  the bridge client uses.
- **Every record lives in a scope**, the top folder named by the `@handle`
  of the account that publishes it, as a scoped name says. A module and its
  own types live in the module's scope; a type modules of different scopes
  share lives in the scope it states, published once there. Its owner
  declares a scope's id in code; where records are published or exchanged,
  the store's ownership of the scope decides.
- **One command.** `arora` runs a device and, through `arora-pkg`, manages
  its project's packages. It reaches Studio by naming semio as the provider
  (`arora auth login semio`); a provider fills roles behind `arora`'s verbs
  and adds none (3.11).
- **Three parties, three kinds of access.** An author signs in to publish.
  Whoever deploys a configuration of public packages needs no account. A
  device resolves as itself, and may be enrolled with a Studio account that
  manages it.
- **A guest module is indexed from its module record.** The header stays as it
  is; the record, checked against it, gives every export its frozen signature
  in the method index. This unblocks behaviors calling `step_values`
  ([STUDIO-347](https://linear.app/semio-ai/issue/STUDIO-347)) before any
  store or protocol work.
- **A configuration is a frozen record** that references module records,
  type records and behavior records, pins the release of every package they
  come from, and states requirements on the device. HAL and bridge settings
  are not part of it.
- **One remote surface:** `ApplyConfiguration` changes what a device runs, and
  `DescribeConfiguration` returns it. There is no remote operation to load,
  reload or unload a single module.
- **Applying is a diff, between steps.** Unchanged modules keep their instance
  and state; changed ones are reloaded, removed ones unloaded, new ones
  loaded. Behaviors keep running across a reload. A configuration that does
  not apply leaves the old one running.
- **The claim is the gate.** The router grants the new operations to the
  device's claimant, as it grants calls; the device adds no access rule.
- **The cost is a release across four repos**: an `arora-bridge` major, an
  `arora-types` major for the new record kinds with an `arora-registry`
  major for the source trait, a `studio-bridge-msgs` major with its client
  chain, a router grant, a protocol revision a device announces, so a client
  never sends the new operations to a device that cannot answer them, and
  Studio's publish endpoint binding package ids.

---

## 1. What exists

### 1.1 Inventory

| Component | Where | What it does | Used by | Status |
|---|---|---|---|---|
| Type records | `arora-types`, `record` | Versioned declarations — structures, enumerations, modules, folders — unfrozen (version requirements) or frozen (pinned versions); `Freeze` and `Resolver` pin them. The frozen serde shape is the store's wire contract. A module record carries an `executable` id, attached at publication. | Registries, module authoring, the behavior tree | Merged |
| `arora-registry` 2.1 | arora-sdk | Local record registry: adds, tags (freezes) and looks up records by id or folder path and version requirement, or by exact id and version (`module_at`, `type_at`); loads record directories (`records/{folder,enumeration,structure,module}/<uuid>@<version>.yaml`) and in-memory records (`load_records`). | `arora`'s devices, `arora-module-cli` and its generators, `arora-cli`, `arora-behavior-tree`'s build | Merged, published |
| `arora-registry-remote` 1.0 | arora-sdk | `RemoteRegistry` reads records from semio-db through `semio-client`; `RemoteCachedRegistry` puts a `LocalRegistry` in front of it as a cache. Reads the semio-cli login file. | `arora-cli`, `arora-module-cli` | Merged, private; over semio-db |
| semio-record | semio-record | A record vocabulary over `arora-types`' references and freezing: record kinds — users, organizations, folders, modules, structures, enumerations, animations, scenes — with private, public, unfrozen and frozen facets, the actions that build them, and access control lists. | semio-db and its clients; a newer store client | Maintained, reused |
| semio-db, semio-client, semio-store-rpc | separate repos | GraphQL record store on ScyllaDB: records by id and version, semver tags with a `yanked` flag, a folder tree rooted at its user and organization records, path-to-id names; a WebSocket blob store; their client. | semio-cli, `arora-registry-remote` | Deprecated |
| semio-cli | semio-cli | `semio`: resolves, fetches, shows and publishes Studio's assets through the Studio source; signs in by a code a person approves and enrolls devices ([semio-cli#11](https://github.com/semio-ai/semio-cli/pull/11)). Its record commands over semio-db remain. | Developers | Maintained; ships `arora-semio`, Studio's provider, and keeps only `semio db …` (3.11) |
| `arora-cli` 0.1 | arora-sdk, `crates/arora-cli` | Loads modules from headers and executables, calls a function a YAML file describes, repeats and times it; fills its registry from record directories or from semio-db through `arora-registry-remote`. The integration tests drive it. | Developers, the integration tests | Merged, unpublished; becomes `arora call` (3.11) |
| `arora-index` | arora-sdk, `crates/arora-index` | Resolves an asset `{scopedName, range}` to a release with Studio's selection rules, through an object-safe `Source` trait; verifies each artifact by SHA-256 before use; content-addressed cache with sealed private blobs and `resolved.json`; `resolve_with_fallback`; `DirSource`, a local directory; `build::models_with(source)`. Its pins hold no package id, and `fetch` finds a release only under the pin's exact name. | Nothing yet | Drafted in [#360](https://github.com/semio-ai/arora-sdk/pull/360), closed unmerged; the starting point of stage 3 |
| Studio source | studio-bridge, `index-source` (to ship as `semio-studio-assets`) | `StudioSource` implements `arora-index`'s `Source` over Firestore and Storage REST: follows a name's claim or alias to its package and lists its releases, fetches artifacts, plans and makes Studio's publish writes. | semio-cli | Drafted in [studio-bridge#109](https://github.com/semio-ai/studio-bridge/pull/109), closed unmerged; the starting point of `semio-studio-assets` |
| `semio-studio-session` | studio-bridge | Sign-in to Studio by a code a person approves, device enrollment by code or token, a session that renews its ID tokens, and the credential kept encrypted in a directory. One crate for the device client, the TUI and `arora-semio`. | The device client, the TUI, `arora-semio` | [studio-bridge#114](https://github.com/semio-ai/studio-bridge/pull/114), open |
| `feat/type-registry` | arora-sdk branch | Emitted `records/` and a `records.json` id→name index per module; a wasm `Registry` in `arora-web` resolving ids to names. | Nothing | Unmerged; the crate it changes no longer exists |
| Module directories | `arora`, `module_dir` | A device loads every `<device dir>/modules/*/` (a `header.json` beside one artifact) at start, and each `--module` directory. | The `arora` runner | Merged |
| Guest module indexing | `arora`, `module_discovery` | A guest module whose module record the device's registry holds (`AroraBuilder::with_registry`, or `records/` in its module directory) has every export indexed with the record's frozen signature, checked against the header. Without a record, an export joins the method index only when its parameters and return are primitives or optional scalar primitives; a module loaded or reloaded on a running device is indexed from its header. `with_declared_module` indexes every export from a Rust declaration. | Every device | Merged (records: arora 13.1) |
| Module reload | `arora` | `Arora::load_module`, `reload_module`, `unload_module` on a running device, between steps, one module at a time, all or nothing; a shared, mutable method index; the engine's `reload_module`, `unload_module`, `contains_module`. What a caller asked of a reloaded or unloaded module and the device has not finished is interrupted, and a task run calling it is halted. A native library is refused. | Nothing yet | Merged (arora 13.0) |
| Module declarations | `arora-module`, `arora-module-macros` | A Rust declaration produces the header, the frozen **module record** (`AroraModule::record`), the exports and client stubs, and states the module's scope. A derived type produces its structure or enumeration record (`AroraType::record`), in the scope it states or its module's (`AroraModule::type_records`). | polly, test-rust-wasm, the animation modules | Merged (arora-types 3.8) |
| `module.yaml` pipeline | `arora-module-cli`, `arora-module-authoring` | Analyzes a `module.yaml` against a registry into a frozen module record and frozen type records, then generates C++/Rust bindings. | C++ modules, importing Rust modules | Merged |
| `arora-web` | arora-sdk | Browser runtime: `AroraRuntimeBuilder.withModule(headerJson, bytes)`, `withCompiledModule`, and `withRecords(records)`, the records its guest modules are described by. | arora-sdk demos | Merged (records: 10.1) |
| Bridge operations | `arora-bridge` 6.1 | `Get`, `Update`, `Call`, `ListKeys`, `ListMethods` (deprecated), `DescribeMethods`. Behaviors travel as calls to the interpreter module (`LOAD`, `EDIT`, `SPAWN`, `HALT`). Nothing carries modules or records. | Every bridge | Merged |
| Studio Bridge | studio-bridge | `AroraOp` mirrors `BridgeOp` (msgs 6.x); sender-addressed command keys `rpc/{device}/{principal}/{op}/{target}` the router authorizes per operation; protocol token `mgmt/{device}/liveliness/protocol/2`; the device's Studio credentials. | Studio, `arora --features studio-bridge` | Merged |
| Studio v2 assets | semio_studio | A package is `publishedAssets/{id}`; `scopedNames/{@scope:slug}` claims point at it, and a transfer keeps the id and leaves the old name as an alias. A release, `publishedAssets/{id}/releases/{id}`, has `artifacts: {key → {storage, contentHash, role}}`, keys `primary` and `preview` only; release dependencies are keyed by package id; asset types `Model`, `Animation`, `Face`; `resolveDependency`. No module, type or configuration asset. | Studio | Merged |
| `studio-arora` | semio_studio, Studio-on-Arora branches | Studio's wasm crate on arora 13 (`^13.1`). Loads `@vizij/animation-module` 4.1 (header, wasm and records from npm) once as a `CompiledModule`; builds each emulated device with `with_compiled_module` and `with_registry`; pushes the stepping behavior by `spawn_graph`, relinks it by `edit`. | Studio | Branch ([semio_studio#610](https://github.com/semio-ai/semio_studio/pull/610)) |
| `vizij-animation-module` | vizij-rs | The animation module (module record 2.0.0, `@vizij/animation`), declared in Rust. The npm package `@vizij/animation-module` ships its header beside the wasm and, from 4.1.0, its records under `artifact/records/`: the module record and the records of the structures its functions name, in the `vizij` scope. | Studio, vizij devices | Merged, published |
| `arora-animation-module` | arora-sdk, `modules/animation` | The animation module, moved from vizij-rs ([#372](https://github.com/semio-ai/arora-sdk/pull/372)) with its module id kept: module record 3.0.0, `@arora/animation`, its records in the `arora` scope at versions of their own, past those `@vizij/animation-module` ships. `module_dir::write` writes its module directory, `records/` included. | Studio | Merged, published (crate 2.0.0) |

### 1.2 The gap behind STUDIO-347

Without a module record, `AroraBuilder::build` freezes a guest export's
signature from its header. A header's `TypeRef` names a type by id without a
version, and a frozen signature pins every record reference to a version, so
an export that names a record — `step_values` returns `StepValues`,
`player_states` returns `PlayerState[]` — dispatches but is not indexed.

A graph calling either is accepted, and its run fails. arora-behavior-tree 9
resolves a graph's functions against the device's method index when it lowers
the graph, at the tick after `spawn_graph`, not when it takes it.
`spawn_graph` returns the run's handle; at the run's first tick, the node
calling `step_values` fails with `CallError::FunctionNotFound`, that tick
reports the error once, the run's status key reads `Failure`, and the
device's other runs tick on.

`with_declared_module::<M>()` indexes those exports, because the declaration
carries frozen signatures. It needs the declaring crate linked into the host.
Studio's devices take the module as header and wasm from npm, and the `arora`
runner takes module directories, so neither can use it.

The frozen signatures exist: the declaration produces them as the module
record (`AroraModule::record`), the form a registry holds. arora 13.1 gives
the device a registry and fills it from module directories (section 2), so a
module built with its records has those exports indexed. A module loaded or
reloaded on a running device is still indexed from its header (4.3).

### 1.3 Overlaps, and which component owns each concern

| Concern | Overlapping components | Owner | Why |
|---|---|---|---|
| What a published thing is | Records (`arora-types`); Studio v2 assets and `arora-index`'s `AssetType` / `ResolvedRef` | Packages that carry records | A record is already versioned, frozen and referenced by id and version; a module record is what a declaration and the authoring pipeline produce. A package is what a registry names, identifies and releases; a release of a record kind carries its record as the artifact `record`, so the record and its release metadata — artifact hashes, deprecation — never form two vocabularies. |
| Package identity | Studio's `publishedAssets/{id}` behind name claims and aliases; `arora-index`'s name-only pins | The registry, which binds a package id at first publish ([registry identity](registry-identity.md)) | Studio already keeps a package's id across a transfer; carrying it in releases, aliases and pins lets every client tell a reused name from the package it pinned, and follow a package to another registry. |
| Holding records in a process | `arora-registry`'s `LocalRegistry`; the `feat/type-registry` `Registry`; `arora-index`'s `resolved.json` | `arora-registry` | One registry for authoring (tag, freeze) and for devices (exact lookup). A record carries its name, so the `records.json` index is not needed. `ty::low::TypeRegistry` stays on the header vocabulary's axis ([design note](design_decisions.md#two-type-vocabularies-two-axes)). |
| Where records come from | `arora-registry-remote` (semio-db); `arora-index` (Studio v2); record directories | The store, through the `RegistrySource` trait in `arora-registry`, one implementation per backend (3.3) | `RemoteCachedRegistry` already pairs a local registry with a remote; generalized, a device's registry is a `LocalRegistry` in front of any source. `arora-index`'s selection, verification and sealing work over any source; the Studio source is Semio's `semio-studio-assets`. |
| Connecting to Studio, credentials | `arora-index`'s `TokenSource`; the Studio source's `/tool-login` sign-in; studio-bridge's client | Semio's `semio-studio-session`, which the bridge client, the Studio source and `arora-semio` all use | Arora works without Studio. Sign-in is one Semio crate; the bridge client keeps the device's credentials in its directory. |
| The device's cache directory | `arora-index`'s `<device dir>/assets/`; the Studio Bridge client's directory; `<device dir>/modules/` | One cache, in the device directory, shared with the bridge client | Two cache roots for one device drift. Operator-installed `modules/` stays a separate, unverified input. |
| Publishing | semio-cli (semio-db, then Studio through the Studio source); Studio's web publishing | The store's backend, through one publish path per backend | Records are published where they are read, and the backend decides at publication who owns a scope (2.2) and which package an id names (3.3). On Studio that path is its publish endpoint, the web app's included. |
| Release selection | `arora-registry`'s `resolve_tag`; `arora-index`; Studio's `resolveDependency` | `arora-index`'s resolver, with the source supplying releases and metadata | One rule for a range: the newest non-deprecated version that satisfies it and has the requested artifact, as Studio's `resolveDependency`. |

---

## 2. Loading a module with its records, on what exists

This part needs no store, no protocol change and no configuration. It unblocks
STUDIO-347 on both kinds of devices.

It has landed in arora-sdk: arora 13.1 (`with_registry`, `records/` in module
directories), arora-registry 2.1, arora-types 3.8 with its derive and the
module macros (records and scopes), and arora-web 10.1 (`withRecords`).
`@vizij/animation-module` 4.1.0 ships its records, and `arora-animation-module`
writes them into its module directory. Studio passes them in
[semio_studio#610](https://github.com/semio-ai/semio_studio/pull/610), open.

### 2.1 The runtime

- **`AroraBuilder::with_registry(registry)`** gives the device a
  `LocalRegistry`. The builder looks records up by exact id and version, which
  the registry answers from memory; it never tags or resolves a path.
- **At `build`, a guest module whose module record is in the registry is
  indexed from it.** The record is the one at the module's id and the
  header's version. Each export of the header must have the same id, name and
  parameter ids in the record, and each frozen type must be the header's
  `TypeRef` frozen: a primitive id as that primitive — a `Key` parameter as
  the string primitive — and any other id as a reference to it, at the
  version the record pins. A record that does not match its header fails the
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
      ├── folder/<scope id>.yaml
      ├── module/<module id>@2.0.0.yaml
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
  record for each `AroraType` (`AroraType::record`), at the version
  `arora_type_version()` already returns, so a declared module exports the
  closure of its records. The module macros and the derive freeze a `Key` as
  the string primitive, the one form the device's check accepts.
- **Scopes.** The top folders are scopes, `arora-index`'s term: a scope is
  the `@handle` of the account — a person or an organization — that
  publishes the records in it, one level deep. A scope is declared once by
  its owner, in code, as a `Scope`: a fixed id, and the handle as its
  folder's name. Every record published to the store states its scope,
  modules and types alike; a local registry also holds records with none,
  at the root — an operator's hand-installed module, a test's.
  - A module's declaration states its scope (`scope = VIZIJ`), and its own
    types take it: the module macros pass it to the types its functions
    name, and those to the types they name.
  - A type modules of different scopes share states its own,
    `#[arora(scope = ARORA)]`, and its record lives there whatever names it:
    `Status` and `TickId` live in the `arora` scope (`arora.Status`),
    vizij's types in `vizij`. A module directory, installed by hand,
    carries the closure of its records, that record and the scope's folder
    included. A published module does not: the type is published once, as a
    package of its own in its scope, and the module depends on it (3.3).
  - A name is unique within a scope by the registry's rule: a registry, on
    a device or a client, refuses a second record under a path another
    record holds, so two records cannot take one name in a scope there.
    The store is not required to keep names unique under a parent.
  - Code declares a scope; the store imposes scopes where records are
    exchanged. A local registry takes a scope as its code declares it.
    When records are published to the store or exchanged through it, the
    store's ownership of the scope decides: a scope declared in code is
    challenged against the backend's ownership of that scope, and the
    backend's answer prevails.
  - Scope ids follow each owner's fixed-id convention, outside the ranges
    its types, modules and functions use: `arora` is
    `61726f72-6100-0000-f01d-000000000000` and `vizij`
    `76697a69-6a00-0000-f01d-000000000000` (each led by the ASCII of the
    name, `f01d` marking a folder), and `semio` would be
    `73656d69-6f00-0000-f01d-000000000000`. An id is not derived from the
    handle, so a handle can be renamed.
  - Renaming a scope changes its handle in one place, the scope's folder
    record, and every registry and source holding that folder takes the
    update consistently, since a path is read from it. Records refer to
    their scope by UUID (`parent`), never by handle, so a renamed scope
    breaks no record: the UUID is the scope's identity and the handle its
    readable address, two complementary ways to reach it.
  - Moving a package to another scope re-parents its later versions only:
    each version published after the move names the new scope, and
    earlier versions keep theirs. A record embeds its parent, so a moved
    record is new content and takes a new version; its id stays. The
    package keeps its id (3.3), and each scope keeps its own, so a pin and
    a reference by id and version resolve across the move.
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
| **Requirements**: executors, interpreters with their versions, optionally a model family | A device refuses a configuration it cannot run before fetching anything but the configuration's own record. |

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
    pub graph: Vec<u8>,              // the graph, serialized as its format says
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
    pub packages: Vec<Pin>,          // where each record and artifact comes from (3.3)
}
```

- **Freezing locks a configuration.** As authored, a configuration names
  version requirements (the unfrozen form); freezing it against a registry
  pins each to the newest published, non-deprecated version that satisfies
  it, with the existing `Freeze` and `Resolver`, and records the pin of each
  release it draws from. A device applies only frozen configurations, so a
  device and the client that sent the configuration run the same records.
- **A behavior's graph is bytes in its format.** The record names the format
  (`graph_type`, `graph_version`) and the interpreter that runs it parses the
  graph, so `arora-types` holds no graph model and depends on no interpreter
  crate: `arora-behavior` already depends on `arora-types`, and the reverse
  would be a cycle. Another behavior language is another `graph_type`, not
  another record kind.
- **Freezing checks the behaviors against the modules.** Each function a
  behavior calls must be an export of the module version the configuration
  binds (below), with a matching signature. The device does not repeat this
  check for the behaviors a configuration carries: it carries both sides, and
  is checked where it is made. A behavior the device keeps running, with its
  live edits, is checked at apply (3.6, Prepare).
- **The record set is closed.** Every reference reachable from the
  configuration resolves to a record in the `record` artifact of a release
  the configuration pins. `packages` is the configuration's lock: freezing
  writes one pin per package it draws a record or an artifact from, and a
  device fetches exactly those.
- **One version of each package is bound for the whole configuration.** A
  record pins the versions it was built against: a module the types it
  names, a behavior the modules it calls. Its dependencies state the ranges
  it accepts, each including the version it pins. Freezing picks, for each
  package, the newest version in every range, and records it in the
  configuration's `types` or `modules`. On the device, a frozen reference
  resolves to the bound version, whatever version it pins: signatures, the
  closure check and the method index all use the binding, which is in the
  pinned version's range by construction, and no published record changes.
  So a module's patch release reaches a configuration without a new release
  of the behaviors that call it. A configuration where no version is in
  every range is refused, naming the package and the records that disagree.

**A release carries its record.** A release of a record kind carries the
frozen record as the artifact `record`, beside its other artifacts: for a
module, the header and one executable per executor (`wasm`, then native
targets). The module record's `executable` id becomes this map of keys to
hashes. A module's published form is therefore one package whose releases
carry the module record, the header and the executables; each type it names
is a package of its own, which the module's release depends on.

**A release is immutable.** A release is a package's version: the pair
(package id, version) names one set of artifact hashes, and so one frozen
record, forever. New content is a new version. A release can be deprecated,
which keeps it out of range selection (1.3) and is the only change after
publication; it is never edited, replaced or unpublished. Versions need not
be published in order: a fix on an older major, 1.4.1 after 2.0.0, is a
release like any other.

### 3.3 Packages, releases and registry sources

[Registry identity](registry-identity.md) states the scheme across Studio,
the open registry protocol and devices; this section states the part
arora-sdk holds.

A **package** is what a registry publishes under one name: a module with its
executables, a single type, a behavior, a configuration, a model. It wraps
nothing: a type's package is that type, released at each of its versions.
Records need a package only to be published. A module directory, a local
registry and an inline configuration carry records directly.

**Names address, ids identify, hashes verify.**

- **A name routes.** Every lookup, dependency and pin names a package by its
  scoped name, `@scope/name`, and a project maps each scope to one
  registry. A registry can therefore be plain files laid out by name.
- **An id identifies.** A package has an id of its own, a UUID its registry
  binds at the package's first publish: one it mints, or the one the
  publisher presents when no package holds it there.
  - An id another package holds is refused. Whether that package takes the
    publish is its owner's decision, and moving an id between owners goes
    through the registry's operator.
  - A deleted package's id is never bound again, so a name that is reused
    never answers with the id a pin holds.
  - A package's id is never a record's UUID. A package may carry several
    records, or none (a model), and a record's UUID is public in source.
- **A hash verifies.** Each artifact carries its SHA-256, and the resolver
  checks the bytes against it before they are used or cached.
- **A move keeps the id.** A transfer to another scope, a rename within a
  scope and a move to another registry each leave the old name pointing at
  the new one, as an alias or, for another registry, a redirect, carrying the
  id. Inside a registry an alias points at the package, not at a name, so a
  later move updates it and it stays one hop. A redirect to another registry
  can lead to another redirect. A source answers a move to another registry
  as `Listing::Moved`, and the resolver follows it: it maps the destination
  registry to a source through the same configuration that maps scopes to
  registries, follows a bounded number of hops, requires the id at every
  hop to be the id it set out with, and refuses a cycle or a registry it
  has no source for. A consumer holding the id keeps resolving across the
  move, with no new dependency and no breaking release.

**Types are packages.** Each type is published once, as a package of its
own in its scope, with its own semver; its release carries its record. A
module's release depends on the type packages it names, with the ranges its
module record accepts, and a configuration picks one version of each type
for all its modules (3.2). A type defined inside another package, such as a
profile inside the model it describes, is reached through a dependency on
that package.

**Three parties.** Producing, deploying and running need different access:

| Party | Does | Needs |
|---|---|---|
| An author | Publishes packages and releases | An account that owns the scope, on the backend's one publish path |
| A deployer, such as Studio or `arora` (3.11) | Resolves, freezes and applies a configuration | Nothing for public packages; read access to private ones |
| A device | Fetches what its configuration pins, and runs it | Nothing for public packages. For private ones, and for the Studio Bridge, its enrollment, which ties it to an account that manages it |

One trait serves every backend and every kind, records and plain assets
alike:

```rust
/// A registry: the releases of a package, and the bytes of their artifacts.
/// It maps its protocol onto `SourceError`; only `Network` permits falling
/// back to a pin, and only a definitive `NotFound` clears one.
// Native sources are `Send + Sync`; on wasm32 the trait is `?Send`, so a
// browser source can await JavaScript promises and hold page-local state.
// `MaybeSend` and `MaybeSync` are `Send` and `Sync` natively, and empty on
// wasm32; the resolver takes the same bounds.
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait RegistrySource: MaybeSend + MaybeSync {
    /// Every release of a package, in listing order, which breaks ties in
    /// selection. An alias inside this registry is followed; a move to
    /// another registry is answered as `Moved`, for the resolver to follow.
    async fn releases(
        &self,
        package: &PackageRef,
        kind: &Kind,
        limits: Limits,
    ) -> Result<Listing, SourceError>;
    /// One artifact of a release, by key. The caller verifies the bytes
    /// against the release's hash before it uses or caches them.
    async fn artifact(
        &self,
        release: &Release,
        key: &str,
        limits: Limits,
    ) -> Result<Vec<u8>, SourceError>;
}

pub enum Listing {
    Found(Vec<Release>),
    /// The package moved to another registry: where, under which name, and
    /// the id it keeps there.
    Moved { registry: RegistryName, name: ScopedName, package: PackageId },
}

pub enum PackageRef {
    Name(ScopedName), // `@scope/name`: every lookup the resolver makes
    Id(PackageId),    // for a person who holds only the id; optional, a
                      // source without an id index answers `NotFound`
}

pub struct Release {
    pub package: Option<PackageId>, // `None` from a registry that keeps no ids
    pub version: Version,           // unique per package
    pub scoped_name: ScopedName,    // the package's current name
    pub scope: Option<Uuid>,        // the scope it was published under
    pub kind: Kind,                 // an open string: model, face, animation, module, type, behavior, configuration…
    pub artifacts: BTreeMap<String, Artifact>, // key → SHA-256 and size
    pub dependencies: Vec<Dependency>,
    pub deprecated: bool,
    pub private: bool,
    pub family: Option<String>,
    pub locator: String,            // opaque to everyone but the source that made it
}

pub struct Dependency {
    pub name: ScopedName,           // routes the lookup
    pub kind: Kind,
    pub package: Option<PackageId>, // checks what the name answers
    pub range: VersionReq,
}

pub struct Pin {
    pub requested: ScopedName,      // the name it was asked for under
    pub kind: Kind,
    pub package: Option<PackageId>,
    pub registry: RegistryName,     // the registry the requested scope maps to
    pub version: Version,
    pub artifacts: BTreeMap<String, Sha256>, // the keys it uses
    pub locator: String,
}
```

- **A record travels as an artifact.** A release of a record kind (module,
  structure, enumeration, behavior, configuration) carries its frozen record
  under the artifact key `record`, beside its other artifacts: a module's
  `header` and one executable per executor. The record is then content-hashed
  and verified like every other artifact, and a source needs no record-shaped
  call. A model, a face or an animation has no `record` artifact.
- **A device creates the scope folders its records need.** `arora-registry`
  places a record under its parent folder, and a record's parent is its
  scope's id. Loading a record whose scope folder is absent first adds that
  folder, named by the id itself: a device resolves by id, never by a
  folder's name, so the scope's handle, which can change, is not needed. A
  release made before a move installs under the scope id it was published
  with, from its pin alone, and no registry publishes folders.
- **The resolver verifies, and only the resolver.** `arora-index`'s resolver
  selects a release (Studio's rules: the newest non-deprecated version in
  range that has the requested artifact), checks the pinned hash before a
  download, verifies the bytes, caches them by hash, and falls back to the
  last pin only on a `Network` error. A source transports; it is never
  trusted with the check. The resolver also refuses:
  - a release whose id differs from the pin's or the dependency's;
  - a record whose `parent` is not the release's `scope`;
  - two releases of one package and version whose hashes differ for one
    artifact key, and one record id and version arriving with two different
    contents: both are a corrupt source, `Malformed`.
- **A pin finds its release again.** A fetch through a pin asks the
  registry its requested name maps to for that name's releases, following
  aliases and redirects, takes the pinned version, and checks the id and
  every pinned hash before it downloads. A fresh device needs only the pin,
  and a pin made before a move still fetches. The locator is a shortcut,
  valid only on the registry that made it: a source may use it to skip the
  listing, never after a redirect to another registry. A pin with an id,
  served by a registry that keeps none, is accepted with a warning naming
  that registry.
- **Credentials are not in the trait.** A source is constructed with what it
  needs to authenticate, as the device, as a person or as nobody, from a
  credential provider. No call passes a token.
- **A device's registry is a `LocalRegistry` in front of a source**, the
  shape `RemoteCachedRegistry` has today. What persists is the cache of
  verified artifacts, by content hash and sealed when private, in the
  directory the bridge client already uses, not a second cache root. The
  registry itself is memory only: at start the device rebuilds it from the
  `record` artifacts its committed configuration pins, so no record has an
  unsealed copy on disk.
- **Implementations:** a local directory (`DirSource`, no network) and a
  static host over HTTPS, which share one layout: `index/<scope>/<name>.json`
  holds a package's id, or the alias a move left, and every release, and
  `blobs/sha256/<hash>` its artifacts ([registry identity](registry-identity.md)
  section 10); Studio's backend,
  `StudioSource` in Semio's `semio-studio-assets`, signed in through
  `semio-studio-session`; Studio's page, backed by its own store client.
- **Publishing is the backend's.** A backend publishes through one path that
  binds names and package ids, checks that the publishing account owns the
  scope of every record's `parent`, records that account on the release, and
  hashes each artifact before it writes the release. On Studio, that path is
  its publish endpoint, for the web app and `arora publish` alike.
- **Where it lives.** What records embed — `PackageId`, `Kind`,
  `Dependency` and `Pin`, which a configuration record carries — is in
  `arora-types`, beside the records. The trait and the types only sources use
  (`RegistrySource`, `Listing`, `Release`, `PackageRef`, `SourceError`) are in
  `arora-registry`,
  which depends on `arora-types` and already builds for `wasm32` in
  `arora-web`, so Studio's page uses the same types. `arora-index` is the
  client over it: resolver, cache, sealing, `DirSource` and the build helper.
- **semio-db is not a backend.** It is deprecated, so the design is not
  shaped by its structure — its account-rooted tree, its naming rules. Its
  record vocabulary, semio-record, is reused by a newer store client.
- **The store is the only source of records.** A client — Studio, a CLI —
  reads a record from the store through a source, as a device does; a device
  does not serve the records it holds. No arora-sdk code names a store: the
  trait lets a backend be distributed, and for now the only backend is
  Studio's.
- **A source authenticates as the device**, never as a remote that asks the
  device for something.

### 3.4 Naming a configuration

A configuration is a package like the others. What `arora-index` calls an
asset specification maps onto packages and records:

| `arora-index` | Packages and records |
|---|---|
| scoped name `@scope/name` | the package's name, which routes the lookup; inside a release, its record is `name` in the scope's folder, the path `scope.name` |
| version range | a dependency's `VersionReq`; on a device, only the exact version a pin holds |
| asset type | `Kind`; for a record kind, also the record's `RecordType`, checked by `type_of` |
| content hash | an artifact's SHA-256 in the release |

`@vizij/animation` is the package whose releases carry the module record
`animation` of the `vizij` scope. Each type it names is a package of its
own, in its scope (`vizij.StepValues`) or in the scope it states
(`arora.Status`), and the module's release depends on it. A configuration
is published the same way, under the scope of whoever publishes it.

Records reference each other and their scope by UUID, and a package keeps
its id across a move (3.3), so renaming a scope or moving a package to
another scope (2.2) breaks no reference and no pin: the path follows the
scope's current handle, and the old name points at the new one.

The bridge operation takes a configuration by its pin, plus the
configuration record's reference, because a device applies only what is
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
    /// What the device must run now for the request to apply, so a client
    /// never replaces a configuration it has not seen.
    pub expects: Expectation,
}

pub enum Expectation {
    Any,                  // apply over whatever runs
    Nothing,              // apply only if no configuration runs
    Running(Sha256),      // apply only over this configuration
}

pub enum ConfigurationSource {
    /// A published configuration: the pin of its release, and the
    /// reference of the record that release carries.
    Published { pin: Pin, reference: FrozenReference },
    /// A frozen configuration sent whole, with the records it pins that are
    /// not published: a project's behaviors as edited in Studio. Never a
    /// module's executable.
    Inline { configuration: ConfigurationFrozen, records: Vec<Record> },
}
```

**A configuration is identified by its hash.** For a published one, it is
the hash its pin holds for the `record` artifact, which covers everything
else through the pins' hashes. For an inline one, it is the SHA-256 of the
canonical serialization of the configuration together with the records it
carries, in id and version order, which the device computes on receipt: two
inline configurations whose unpublished records differ in any byte have
different hashes. Expectations, reports, progress keys and persistence all
use it.

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
    Requirements, Resolve, Fetch, Verify, Prepare, Stage, Swap, Commit, Busy, Replaced, Restart,
}
```

`Replaced` refuses a request whose expectation is not what the device runs;
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
being applied is refused at `Busy`. A request for the configuration already
running and committed, by hash, changes nothing and replies `Applied`, so a
client that lost a reply can retry safely. A request for the one running but
not committed, after a failed pointer write, skips to Commit and replies
`Applied` only once it succeeds.

1. **Requirements.** For a published configuration it does not hold, the
   device first fetches the configuration's own `record` artifact, verified
   against its pin. It then checks executors, interpreters and family
   against its own, before fetching anything else.
2. **Fetch.** The device fetches, through its source, each artifact of the
   configuration's pins that it does not hold, verified against the pin's
   hash, and loads the records the releases carry into a candidate registry
   layered over its own. Every reference the configuration makes must then
   resolve there. Swap makes the candidate's records the device's; a
   refusal drops them, so a refused apply leaves the registry as it was. The
   verified blobs stay in the cache either way.
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
   against it. It also checks each behavior that is kept as it runs, live
   edits included, against the new index, without touching its state: a
   kept behavior that calls a function the new configuration drops or
   changes refuses the apply here, naming the behavior and the function. A
   failure here changes nothing.
5. **Stage.** The device writes durably, to a generation directory of its
   own, `<device dir>/configuration/<hash>/`, everything a restart needs:
   the configuration's pin, its record being a cached artifact, or, inline,
   the configuration and its records, sealed as private artifacts are. A failure here, such as a full disk, changes nothing.
6. **Swap, between two steps.** The device instantiates every new and changed
   module before dropping any old instance, then reloads, unloads and loads,
   and starts, replaces or halts behaviors. A behavior that is kept keeps
   running as if nothing changed: its next call reaches the new instance of a
   reloaded module. A command from a bridge that is in flight to a module
   being reloaded or unloaded is interrupted, and its caller gets an error. If
   an instance fails to start, the new instances are dropped, the
   generation this request staged is removed unless `current` names it, and
   nothing has changed.
7. **Commit.** The device writes the new hash to `configuration/current.new`
   and renames it over the pointer file `configuration/current`: replacing a
   file by rename is atomic and repeatable, so every apply commits the same
   way. It then removes the previous generation, whose hash differs from the
   new one (3.6's same-hash rule), and replies `Applied`.
   `Applied` therefore always means the configuration runs and a restart
   runs it again. At start, the device runs the generation `current` names
   and removes every other one. Only `current` says what committed, so a
   leftover generation, from an apply interrupted before its commit or after
   it, reports nothing; a client that got no reply reads
   `DescribeConfiguration`. Should the pointer write itself fail, the device
   keeps running the new configuration, reports the failure in
   `arora/configuration/refused` with stage `Commit`, and returns to the
   generation `current` still names at its next start unless a retry
   commits.

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
| `arora/configuration/running` | The running configuration's hash and, when published, its pin. Unit when none. |
| `arora/configuration/pending` | While a configuration is being applied: its hash and the current stage. Unit otherwise. |
| `arora/configuration/refused` | The last refusal: hash, stage and reason. |

The reply to `ApplyConfiguration` is final: `Applied` or `Refused`. Fetching
can outlast a query's timeout; a client that loses the reply reads these keys,
which the Studio Bridge already syncs. `DescribeConfiguration` returns the
configuration record, its hash and its pins: references and hashes, never the
contents of the records it pins, published or inline. A client resolves the
published ones through its own source, as itself, so reading a device grants
no read of a private record (3.9).

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
- **The device fetches as itself.** Its source authenticates with the
  device's identity, never the sender's: a claimant cannot make a device fetch
  a package its owner may not use, and sends no token. A configuration of
  public packages needs no identity at all; private packages need the
  device's enrollment, which ties it to the account that manages it (3.3).
- **No executable travels inline.** An inline configuration carries records;
  every executable is an artifact of a published module record, verified by
  hash. Development keeps `--module` for unpublished modules.
- **Private artifacts stay sealed** in the device's cache, by the source that
  fetched them. `DescribeConfiguration` returns the configuration record and
  its pins, never another record's contents and never an executable (3.7).

### 3.10 Release and rollout

- **`arora-types` major.** `RecordType` gains `Behavior` and `Configuration`,
  and is matched exhaustively; `PackageId`, `Dependency` and `Pin` join the
  records that embed them. The record kinds' frozen serde shapes join the
  store's wire contract, with their golden tests; the store's backend
  accepts the new kinds.
- **`arora-registry` major, with `arora-types`'.** The registry source trait
  and its types, exact lookup, and the new kinds. Its public API carries
  `arora-types`' types, so it moves with that major.
- **`arora-index`.** Pins carry the package id and find their release by
  locator; `DirSource` reads the shared folder layout; the resolver
  makes the checks of 3.3.
- **Studio** ([semio_studio#616](https://github.com/semio-ai/semio_studio/pull/616)).
  Its publish endpoint becomes the one publish path, the web app's included:
  it binds package ids, checks every record's scope, and hashes artifacts.
  Every published package gets its `packageId`, the document id where that
  is a UUID; release dependencies hold `{ scopedName, packageId, range }`;
  aliases carry the id.
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
  Studio registry source ships as `semio-studio-assets`, publishing through
  the endpoint. A release across four repos.
- **The router** grants the new chunks to the claimant, compiled with the
  other grants.
- **Negotiation.** A device that answers the new operations announces
  protocol revision 3 (`mgmt/{device}/liveliness/protocol/3`). The studio
  client sends them only to a device announcing 3 or more. Studio shows an
  older device as one that cannot be configured remotely, and keeps using it
  as today: calls to the modules its operator installed. A new device answers
  an older client exactly as before.

### 3.11 Commands

A person types one command, `arora`. It runs a device, and its verbs manage a
project, sign in and enroll devices, reaching Studio by naming semio as the
provider. With no verb, `arora` runs, as devices, service units and scripts
start it today, so nothing breaks.

```text
$ arora -h
Usage: arora [OPTIONS]            run the device (the same as `arora run`)
       arora <COMMAND> [ARGS]

Running
  run [PROJECT]     Run the device: its directory's modules, those --module
                    names, and the configuration PROJECT freezes when given;
                    emulated HAL when there is no hardware
  call <FILE>       Call a loaded module's function as a YAML file describes,
                    with --repeat N and --benchmark

Account and devices
  auth login <PROVIDER>             Sign in through a provider, e.g. semio
  auth status [<PROVIDER>]          Show who is signed in
  auth logout <PROVIDER>            Forget the credential
  device enroll <PROVIDER>          Enroll this device by code or enrollment
                                    token; keep its credential in the
                                    device directory
  device list <PROVIDER>            List the devices you manage
  device revoke <PROVIDER> <DEVICE> Revoke a device
  token create <PROVIDER>           Create a publish token for CI (shown once)
  token list <PROVIDER>             List publish tokens
  token revoke <PROVIDER> <TOKEN>   Revoke a publish token

Packages (arora-pkg)
  init              Start a project (arora.toml)
  add <PKG@RANGE>   Depend on a package, e.g. @arora/animation@^3
  install           Resolve, verify and fetch what the project depends on
  update            Move dependencies to the newest versions in range
  show <PKG>        Show a package, its releases and dependencies
  build             Build the project's modules and records
  version <PKG>     Diff a package's interface against its last release
  publish <PKG>     Publish a release to the registry its scope maps to
  deprecate <PKG@V> Mark a release deprecated
  vendor            Copy every pinned artifact into the project

Run options
      --module <DIR>   Load a module directory besides the device directory's
      --groot <FILE>   Install a Groot behavior tree as the device's behavior
      --open           Let any bridge write any key with no meta of its own
      --studio         Connect to Semio Studio (Semio's runner build)
      --no-studio      Serve the open local bridge only

Providers (arora-<provider> on PATH) fill roles behind these verbs.
```

- **`arora run PROJECT`** freezes the project's configuration and applies it
  at start, as `--configuration` does (3.8).
- **`arora call`** is what `arora-cli` 0.1 does today: load modules, call a
  function a YAML file describes, repeat and time it. Its semio-db options
  are dropped (semio_studio#616 D10), the integration tests drive `arora
  call` instead, and the `arora-cli` crate retires.
- **The package verbs are a separate binary, `arora-pkg`**, which `arora`
  hands each of them to. A robot build leaves it out and keeps `run`, `auth`
  and `device`, so the runtime carries no range resolution or publishing
  code: it keeps only what a device needs, fetching and verifying what its
  configuration pins, with its own credential. Without `arora-pkg` installed,
  `arora add` says how to install it. It reaches registries through
  `RegistrySource` implementations (3.3), configured in the user's
  `~/.config/arora/config.toml`, never in a project's file, and links no
  Studio code.
- **Package verbs never name a provider.** Each scope maps to one registry,
  so `install`, `show` and `publish` reach Studio through the registry
  `@semio`, or any scope a project maps to Studio, routes to.
- **Device-specific runners**, such as Vizij's and the HAL runners, stay
  plain runners: they call `run_with_hal` and take no verbs.

**Providers fill roles behind `arora`'s verbs, and add none.** A provider is
a program on `PATH`, `arora-<provider>`, that `arora` starts and speaks a
protocol with, as git does with `git-remote-<name>`, docker with
`docker-credential-<name>` and Cargo with its credential providers. The verbs
stay `arora`'s, so its surface is the same whatever is installed, and an
unknown verb is an error.

- **A provider on its own is an argument:** `arora auth login semio`,
  `arora device enroll semio`, `arora token create semio`.
- **A target inside a provider is `provider:target`.** The remote role
  (`arora link semio:<project>`, `arora sync`) and the session role (`arora
  run my-robot --session semio:<project>`) take that form once they are
  defined.
- **The roles:** sign-in, registry, devices and tokens are defined
  (semio_studio#616); remote and session are defined the same way when their
  functionality lands. A provider fills the roles it supports. A registry
  entry in the user's config names its provider (`provider = "semio"`); an
  `https://` index or a `dir:` registry needs none.
- **Studio's provider is `arora-semio`**, from semio-cli. Sign-in and
  enrollment go through `semio-studio-session`
  ([studio-bridge#114](https://github.com/semio-ai/studio-bridge/pull/114)),
  the one crate the device client and the TUI share; the registry role goes
  through `semio-studio-assets`. Semio's runner build (`--studio`) still
  enrolls itself at start
  ([#423](https://github.com/semio-ai/arora-sdk/pull/423)).
- **A verb with no general meaning under `arora` is a `semio` verb**, in
  semio-cli's `semio` command, never `arora semio …`. Today that is only the
  deprecated semio-db commands, `semio db …`, which retire with semio-db.

**The help lists what is built or decided.** Verbs for the workflows still
to decide come with them and follow the same pattern: running one entity of
a project (`arora run <entity> [<behavior>]`), deploying and Arora's own
sessions, pairing with a robot, and choosing a robot's hardware components.

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

1. arora-sdk publishes `@arora/animation` 3.0.0: a release carrying the
   module record, the header and the `wasm` artifact. It depends on the
   packages of `StepValues`, `OutputKeys`, `PlayerState`, `InstanceState`,
   `AnimationClip` and the types they reference, each published in the
   `arora` scope.
2. A Studio project depends on `@arora/animation` `^3.0.0`. Studio freezes
   the project's configuration: the module record at 3.0.0, one version of
   each type it names, and the `animation-stepping` behavior record
   (`behavior-tree` 1.2), checked against the module record's `step_values`
   and `player_states`; `packages` pins each release.
3. **Emulated device:** Studio applies it on the loopback bridge; the page's
   source supplies the records and the compiled module; `step_values` is
   indexed from the module record; the behavior starts.
4. **Real device on the `arora` runner:** Studio applies it over the Studio
   Bridge. The device's Studio source fetches what the pins name, the device
   prepares, swaps between two steps, and replies
   `Applied`. Studio reads `DescribeMethods` again and finds `step_values`.
5. When the output key table changes, Studio edits the running behavior with
   a call (`EDIT` today, `edit_graph` on the branch once branches exist); the
   configuration is unchanged.
6. arora-sdk publishes `@arora/animation` 3.0.1 with a fix. Studio applies
   the configuration frozen against it: the device reloads the animation
   module between two steps, indexes it from its record, and the stepping
   behavior keeps running, its next `step_values` reaching the new instance
   (3.6). Configurations are meant to provide this; arora 13.1's
   `reload_module` does not. It halts a task run calling the reloaded
   module, so the stepping run ends `Failure`, and it indexes the new
   version from its header only, so `step_values` leaves the method index: a
   stepping graph spawned again is accepted and fails at its first tick
   (1.2).
7. The device restarts: it applies its persisted configuration again, from
   the cache when offline.

---

## 5. How this composes with `arora-index`

`arora-index`, drafted in
[#360](https://github.com/semio-ai/arora-sdk/pull/360), is the client over `RegistrySource` (3.3), and the Studio source is one
implementation of it: Semio's `semio-studio-assets`, from
[studio-bridge#109](https://github.com/semio-ai/studio-bridge/pull/109)'s
`StudioSource` and publish path. Converging on the one trait changes those
drafts as follows:

| Drafted | Converged |
|---|---|
| `arora_index::Source` with `releases(scoped_name, kind)` and `fetch(release, key)` | `arora_registry::RegistrySource` with `releases(package, kind)` and `artifact(release, key)`; the resolver looks up by name |
| `Release` and `ResolvedRef` keyed by scoped name, no id | `Release`, `Dependency` and `Pin` keep the name and add the package id; the pin keeps the name it was requested under |
| `fetch(pin)` finds the release under the pin's exact name, so a pin made before a transfer fails for every blob not cached | A pin finds its release by its requested name and version, following aliases and redirects, and checks the id and hashes; the locator is only a shortcut on the registry that made it |
| `DirSource`: `<scope>/<name>/<version>/release.json` | `index/<scope>/<name>.json`, the package's id or alias and every release, and `blobs/sha256/<hash>`: the layout a static host serves too, since it cannot list a directory |
| `release_id`, the source's opaque id | `locator`, still opaque |
| `SourceOptions` | `Limits`, the same fields |
| `TokenSource` in `arora-index` | Leaves the trait: a source is built with its credentials (3.3) |
| `asset type` | `Kind`, an open string; a record kind is checked by `type_of` |
| `resolved.json` | The device registry's cache of pins |

What stays as it is: selection, hash verification, the cache and its sealed
private blobs, the fallback policy, `SourceError`'s categories, `DirSource`
and the build helper. What the source must serve configurations:

1. several artifacts per release, each with its own hash, fetched by key;
2. an exact pin (name, package id, version and hash) checked before
   download;
3. the types buildable for `wasm32` without the transport, so Studio's page
   uses them;
4. an extensible set of kinds, so a new kind is not a breaking change.

---

## 6. Stages

Each stage lands on its own.

1. **Records beside headers** (section 2). `arora`: `with_registry`,
   indexing from module records, `records/` in module directories.
   `arora-types-derive`: structure and enumeration records. `arora-web`:
   records. vizij-rs: export and ship the records. Studio: pass them
   ([semio_studio#610](https://github.com/semio-ai/semio_studio/pull/610)).
   Unblocks STUDIO-347. Landed.
2. **Module reload** on a running device, from the module-reload work: load,
   reload and unload between steps, instantiation before any old instance is
   dropped, interrupted commands, native modules refused. A module loaded or
   reloaded with its record in the registry is checked as at build. Its
   record failures — a record that does not describe the header, a missing
   dependency — are one `ModuleError` variant, `Record(RecordError)`, and
   `ModuleError` stays exhaustive. arora 13.0 has load, reload and unload
   between steps, one module at a time, interrupted commands and the native
   refusal; a module it loads or reloads is indexed from its header, so the
   record check and `ModuleError::Record` are still to do.
3. **Packages, record kinds and sources** (3.2, 3.3). `arora-types`:
   behavior and configuration records. `arora-registry`: the source trait
   with package ids, dependencies and pins; exact lookup; freezing a
   configuration, its type versions chosen once, with its checks.
   `arora-index`: the resolver's checks, pins found by name and version, `DirSource`'s
   folder layout. `arora-pkg` (3.11): publishing to a folder registry, which binds
   the id, and moving a package, which leaves an alias. Studio: the publish
   endpoint as the one publish path, binding ids and checking scopes, with
   the backfill and the renames ([registry identity](registry-identity.md)).
   Types published as packages of their own.
4. **Configurations in the runtime.** `Arora::apply_configuration` with the
   diff and swap (3.6), persistence and the start-up apply,
   `--configuration` on the runner. No protocol change.
5. **The bridge operations** (3.5, 3.10): `arora-bridge` 7, the studio-bridge
   chain with the Studio source, the router grant, protocol revision 3, and
   Studio's apply.
6. **Commands** (3.11). `arora`'s verbs, its dispatch to `arora-pkg`, and
   the provider protocol behind `auth`, `device`, `token` and registries;
   `arora call` replacing `arora-cli`, with the integration tests moved onto
   it; `arora-semio`, Studio's provider, in semio-cli. Independent of the
   other stages except that `arora-pkg` publishes and installs through
   stage 3's trait.
7. **Behaviors as named branches**, once the behavior module of
   [behavior languages](proposal-behavior-languages.md) lands; until then a
   configuration carries at most one behavior, loaded with `LOAD`.

---

## 7. Open questions

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
- **Is `RecordError` `#[non_exhaustive]`?** Proposed: yes. A caller of
  `load_module` or `reload_module` reports a record failure and never handles
  each one, and a non-exhaustive `RecordError` is what lets a new record
  failure land as an `arora` minor; an exhaustive one makes each a major.
- **Model family in requirements**: required, or left to Studio's binding
  check?
- **A Studio scope's UUID.** Derived from the owning account's id, so
  nothing is stored and nothing drifts, but a scope can then never pass to
  another account; or minted when the handle is claimed and bound in a
  stored document. `arora`, `vizij` and `semio` keep their fixed ids either
  way.

---

## 8. Design decisions

### Published things are records, in packages

Arora already has a versioned, frozen, referenceable form for what it shares:
records, held in `arora-registry`. A module record is what a declaration and
the authoring pipeline produce. A package's release carries its record as an
artifact, so a registry stores and verifies records as it does any other
bytes, and the record never gains a second, asset-shaped description to be
translated at every boundary.

### arora-sdk knows a registry source, not Studio

Arora works without Studio. The source trait lets a device fill its registry
from local directories or any backend of the store alike. Studio's backend,
the only one for now, is reached through Semio's crates: `semio-studio-assets`
for the source and `semio-studio-session` for sign-in, which the bridge client
and `semio` use too. arora links them only in its opt-in Studio feature.

### One registry source for every kind

Models, faces, animations, modules, behaviors and configurations reach a
device and a client by one trait, `RegistrySource`, with two calls: list a
package's releases, fetch one artifact. A frozen record is the artifact
`record`, so it is hashed and verified like any other bytes, and a backend
that stores files (a directory, a static host, Studio's Storage) serves
records without a record-shaped endpoint. Two traits, one for assets and one
for records, would put two caches, two pins and two fallback policies on one
device.

### Names address, ids identify, hashes verify

Each question has one answer, and each answer one form. Where to look is the
scoped name: it routes to one registry, lays a registry out as plain files,
and is what people type, so every lookup, dependency and pin goes by it.
Which bytes is the hash, checked before anything is used. Which package is
the id: the one thing that stays when a name changes. It lets a pin refuse a
different package that later takes the name it holds, lets one package
reached under two names load once, and lets a consumer follow a package to
another scope or registry, checking the alias or redirect instead of
trusting it, with no new dependency and no breaking release. The id is never
a route, so no registry needs an index of ids.

### A package's id is the registry's, never a record's

A registry binds a package's id at its first publish and refuses it to any
other package, so an id names one package on that registry for good. A
record's UUID cannot serve: a package may carry several records or none, the
same record id ships in packages of different scopes when a package moves,
and record UUIDs are printed in source, where anyone can copy them. Who may
take an id another package holds is the holder's decision, and a move
between owners goes through the registry's operator, because no registry
can tell a rightful successor from a copy.

### Each type is a package of its own

A type is a record that modules and clients exchange values of, so its
version must say whether *that type* changed compatibly. Published once in
its own package, with its own semver, it is saved once and reused by every
module that names it, and a configuration picks one version every module
accepts. Shipped inside each module that names it, the same record would
reach a device from several packages; versioned by the release that ships
it, a module's major for a changed function would mark every type it carries
incompatible.

### A release is immutable

Each version of a package is published once. A pin, a configuration and a
frozen record reference that version, so the bytes behind it must never
change: a fix is a new version, and a bad release is deprecated, never edited
or removed. Deprecation is the one field that changes, because it only steers
selection and never changes what a pin fetches. Order is free: a fix on an
older major is a release, so a version need not be above the latest.

### Three parties, three kinds of access

Producing, deploying and running are done by different parties, often on
different machines. An author publishes, so the backend checks an account
that owns the scope. A deployer resolves and applies, and public packages
ask nothing of it, so deploying public work needs no account. A device runs
what it is given and fetches as itself, never as the sender; its enrollment
ties it to the account that manages it, which private packages and the
Studio Bridge require. Sharing one sign-in crate keeps the three on one
session model without merging their rights.

### The resolver verifies, not the source

A source is a transport, and some are third-party adapters. Verifying every
artifact against its hash in the resolver, before it is used or cached, keeps
the check in one place that every source passes through.

### A device does not serve records

The store is the one source of records. A device answering for the records it
holds would be a second, partial store whose contents depend on what the
device happens to run, and a second surface to authorize. A client reads
records where the device does, from the store through a source.

### Records live in scopes

A record's folder is part of the record, so a type's record must be the same
whichever module ships it. Scopes fix every record's home where its owner
declares it: a module and its own types live in the module's scope, and a
type modules of different scopes share states its scope. Placing a type in
the folder of each module naming it gives two records under one id and
version, which no registry holds together. A scope is one level deep and is
a scoped name's `@scope`, so a published record's path and its scoped name
agree.

### A scope is reached by UUID and by handle

Records refer to their scope by UUID, declared rather than derived from the
handle, so a renamed handle leaves every record intact: a rename is an
update of the scope's folder record, the one record that carries the
handle. The handle is the address people read and type; the UUID is what
records hold. Moving a package to another scope re-parents only the versions
published after the move, because an earlier version is frozen: a
configuration pins it, and changing its parent would change a record others
have already resolved. The package's id and its records' ids do not change,
so every pin and reference still resolves.

### Code declares a scope; the store decides at exchange

A scope declared in code lets a module's records be built, loaded and
tested in a local registry without a store. But anyone can write
`scope = ARORA`: only the store knows who owns a scope. So where records
are published or exchanged, a declared scope is challenged against the
backend's ownership of it, and the backend's answer prevails.

### Names are unique in a scope by the registry's rule

A path resolves to a record in a registry, on a device or a client, so that
is where a name must be unique in its scope: the registry refuses a second
record under a path another holds. The store keeps records by id and is not
required to keep names unique under a parent.

### One frozen form for a header type

A record's frozen type is the header's type frozen: a primitive id as that
primitive, a `Key` included, and any other id as a reference. The module
macros and the derive produce that one form, so the device checks a record
against its header with one rule, and a record cannot describe the same
parameter two ways.

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
payload, bypass the hash a pin carries, and let a claimant hand a
device bytes its owner is not entitled to. The device's own source and cache
make the result the same whoever sent the request, and let it restart offline.

### One `arora` command, its package verbs a separate binary, providers behind its verbs

A person runs a device and manages its project with one command, as `go`,
`deno` and `cargo` do, so `arora add` and `arora publish` explain themselves
next to `arora run`. The package verbs live in `arora-pkg`, which `arora`
hands them to, because a robot's runtime should carry no range resolution or
publishing code it never uses; it keeps `run`, `auth` and `device`, the fetch
and verification its configuration needs, and its own credential.

Studio is a provider, not a command: `arora auth login semio`, `arora device
enroll semio`, and package verbs that reach it through the registry a scope
maps to. `arora`'s verbs are arora's, and a provider program fills roles
behind them, as git remote helpers, docker credential helpers and Cargo's
credential providers do. So `arora`'s surface, its help and what a script may
call, is the same on every machine, and Arora ships nothing that reaches
Semio until a user installs `arora-semio` and names it.

### Alternatives considered

- **Extensions that add verbs** (`arora semio link`, through `arora-<verb>`
  on `PATH`, as cargo and git run external subcommands). `arora`'s surface
  would then depend on what is installed, and a provider's verbs would
  shadow or pre-empt verbs `arora` defines later. A verb with no general
  meaning under `arora` is a verb of the provider's own command, `semio …`.
- **A separate `semio` command for Studio's sign-in, devices and packages.**
  It duplicates `arora`'s verbs with Studio's name in front, and gives a
  person two tools for one task depending on where a package is published.
- **A project tool under a name of its own** (`arora-cli`, a short coined
  name). `arora-cli` already names the 2022 module runner, whose work is
  running modules, `arora`'s; a coined name has to be learnt and explained,
  where `arora add` reads as what it does.
- **Every verb compiled into `arora`.** The runtime built for robots, NAO
  included, would carry range resolution and publishing code.
- **A Studio asset vocabulary in arora-sdk** (asset types, resolved
  references, a Studio resolver as a crate of its own). It duplicates records
  and their registry, and puts Studio's protocol in a runtime that works
  without Studio.
- **A `record` call on the source, beside `versions` and `artifact`.** It
  needs every backend to serve records as such, and leaves the record out of
  the hash check its artifacts get.
- **Identity by scoped name alone, with aliases for moves**, as npm,
  crates.io, Maven and Go identify packages. An alias works only inside the
  registry that recorded it, and a registry without aliases turns a rename
  into a new package that leaves its consumers on a deprecated one. A name
  freed by a deletion can be taken by another package, which a pin by name
  cannot tell from the one it pinned. None of those registries lets a
  package move to another scope or registry without its consumers changing
  their dependency; the id is what this proposal adds to have that.
- **A lookup by id, with an index of ids in every registry.** Every registry
  and mirror would have to keep that index in step with its names. Lookups
  go by name, and the id sits next to the name it belongs to.
- **The record's UUID as the package's id.** See "A package's id is the
  registry's, never a record's".
- **A type versioned by the release that ships it, or shipped inside each
  module naming it.** See "Each type is a package of its own".
- **A release UUID.** A release is a package's version: the pair, with the
  artifacts' hashes, already names it once and for all. A registry's own
  release key stays inside the opaque locator.
- **Versions published in increasing order only.** It refuses a fix on an
  older major, which a published version's immutability does not require.
- **The trait in `arora-index`.** `arora-registry` is where a device's
  `LocalRegistry` sits in front of a source, and it already builds for
  `wasm32`; `arora-index` depends on it, not the reverse.
- **Load, reload and unload as remote operations.** See "One remote surface".
- **Replacing the whole engine on every apply.** It drops the state of
  modules the configuration did not change.
- **A configuration pushed call by call.** It is what Studio does today for
  one module it assumes is installed. It cannot add a module, is not atomic,
  and leaves nothing a device can restore at start.
- **The HAL in the configuration.** A HAL's models and components are fixed
  for its life; changing them is a restart.
- **A type's record under the module that defines it.** A module naming
  another module's type would depend on that whole module for the type's
  path to resolve, and a vocabulary no module defines (`Status`) would need
  one invented to own it.

# Changelog

All notable changes to `arora`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [16.0.0] - 2026-10-23

### Added

- **Several devices on one host**: `RunnerArgs`, the options every runner
  takes (the `arora` binary's `DeviceCli`, `arora-ros2`, `arora-restful`,
  `arora-naoqi`), gives the device the run is besides `--module`:
  `--device <NAME>` (`ARORA_DEVICE`), its name on this machine, `default`
  when unset; `--device-dir <PATH>` (`ARORA_DEVICE_DIR`), its directory,
  else `<data_local_dir>/semio/arora/devices/<name>`; and
  `--bridge-port <PORT>` (`ARORA_BRIDGE_PORT`), the port of its open local
  bridge, `device::DEFAULT_BRIDGE_PORT` (9000) when unset.
  `RunnerArgs::device()` resolves them into a `Device` (`name()`, `dir()`,
  `bridge_port()`), and `Device::from_env()` resolves them from the
  environment alone. `AroraBuilder::with_device(Device)` hands the device to
  `run`, whose default bridge serves on its port and connects to Studio as
  it; a builder given no device serves on 9000, and connects as
  `Device::from_env()`.
- `operator::EnrollmentCode` and `Operator::show_enrollment_code`: an operator
  shows the code that enrolls the device with Semio Studio, until the next
  call replaces or clears it, and returns whether it shows the code. The
  default shows nothing and returns `false`: a code no operator shows goes to
  standard error when the run's log shows no info lines. The terminal UI
  shows the code in its prompt line, with Studio's page approving it when the
  Studio client knows Studio's web address (`STUDIO_URL`, else the one baked
  into it).

### Changed

- Building with Semio Studio is the opt-in, and the only one. The `arora`
  binary built with `studio-bridge` connects to Studio on every run unless
  the run passes `--no-studio`, whatever the device holds, enrolling a device
  that is not enrolled first (below). A build without `studio-bridge` has no
  Studio option.
  - `studio::StudioArgs` is `--no-studio` alone, and `StudioArgs::choice`
    returns the `Choice` itself: `Choice::Local` with `--no-studio`, else
    `Choice::Studio`.
  - `run`, `run_with_hal`, `run_with`, and an `AroraBuilder` given no
    `with_studio`, serve the open local bridge in a `studio-bridge` build,
    whatever the device holds. A feature of arora is enabled for every crate
    a build links, so `studio-bridge` alone does not tell that the runner at
    hand was built with Studio.
  - A run logs which bridge it serves in one line, and no longer looks a
    saved credential up when it serves the local bridge.
  - An embedder calling a `studio::connect` function connects.

  To migrate, drop `--studio` and `ARORA_STUDIO` from the runs of a build
  with Studio, and pass `--no-studio` to a run of such a build that should
  serve the local bridge. An embedder building its device with
  `AroraBuilder` that connects passes `with_studio(studio::Choice::Studio)`.

- **A device enrolls with Semio Studio** (`arora-studio-bridge-client` 11).
  Studio creates the device, under the owner a person chooses, when it
  enrolls, and the device signs in with the credential its enrollment saved
  from then on; it never signs up as an anonymous account. A run that
  connects enrolls the device when its credentials hold no enrolled device:
  none, one that does not read, one Studio refuses (a revoked or deleted
  device), or one that is no device's, such as the anonymous account 13.x and
  earlier signed a device up as. It enrolls with the enrollment token
  `STUDIO_ENROLLMENT_TOKEN` holds, read from the environment only and never
  logged, else by a code a person enters in Studio. The code is logged, and
  shown to the operator under the terminal UI; one that expires undecided is
  replaced by another. Then the run connects.
  - A person refusing the code, or Studio refusing the token, fails the run.
  - Only a device the client finds not enrolled enrolls. Studio failing
    otherwise fails the run and keeps the credential; so does a credential of
    another Firebase project than the run's, the error naming its directory
    and saying to point the run at that project or delete the directory.
  - The device enrolls with its name — `DEVICE_NAME`; else, under the
    terminal UI, what the operator types; else generated — and its model,
    the operator typing the model family under the terminal UI when nothing
    gives one. Its name and owner are Studio's from then on: a run no longer
    writes them.
  - Each run reports the device's model: `MODEL_FAMILY`, `HARDWARE_VERSION`
    and `SOFTWARE_VERSION`, else what the HAL describes, writing only the
    fields given.
  - `studio::connect_with_operator` and
    `connect_with_operator_and_description` return the bridge
    (`Result<Box<dyn Bridge>>`): there is no owner left to decline Studio
    with.
  - Every `studio::connect` function enrolls a device that is not enrolled:
    `connect`, `connect_with_description` and `connect_with_device_dir` log
    the codes, and ask nobody.

  To migrate a device registered by arora 13 or earlier: run it once from a
  build with Studio, and enter the code it shows in Studio, choosing its
  owner; or create an enrollment token in Studio and set it in
  `STUDIO_ENROLLMENT_TOKEN` for that run. An embedder matching
  `connect_with_operator`'s `Ok(None)` drops that arm.

- **A device's Studio credential is its name's in Semio's device store**,
  apart from its directory: `<data_local_dir>/semio/devices/<name>/`, or
  under `SEMIO_DEVICE_STORE` (`semio-studio-auth`'s `DeviceStore`, through
  the client's `DeviceCredentials::in_store`). arora names the device and
  never computes where its credential is; the device directory holds none.
  - A run that connects to Studio given a device directory
    (`--device-dir`, `ARORA_DEVICE_DIR`) and no name is refused: "name this
    device with --device or ARORA_DEVICE". Two such devices would otherwise
    share the credential of the name `default`. Without Studio, a directory
    alone is fine.
  - A credential the device directory's `studio/` holds moves into the store
    under the device's name, once, when the store holds none for it, and the
    log says so. Its bridge certificate is reissued. Credentials from arora
    11.0 and earlier (`.semio/arora`) move into the store under `default`,
    and a deprecated `IDENTITY_FILE` given with a name migrates through
    `<IDENTITY_FILE>_dir/studio/` into the store.
  - `studio::connect_with_device_dir(device_dir, name)` takes the device's
    name, whose credential it connects with.
- `DeviceCli` flattens `RunnerArgs` as its `runner` field, in place of its
  own `module`.
- `device_dir::DEFAULT_LOCAL_ID` is `device_dir::DEFAULT_NAME`. A device's
  name is exactly one normal path component: besides `""`, `.`, `..` and
  names holding `/` or `\`, one Windows reads as a path on another drive,
  such as `D:bench`, is refused.

  To migrate, name each device on a host with `--device` or `ARORA_DEVICE`
  in place of `DEVICE_LOCAL_ID`, and give a directory with `--device-dir`
  or `ARORA_DEVICE_DIR` in place of `DEVICE_DIR`, with a name when the run
  connects to Studio. A runner with a command line of its own hands
  `RunnerArgs::device()` to `AroraBuilder::with_device`, and its `dir()` to
  `with_module_dirs`. `semio device enroll NAME` enrolls the device of that
  name.

### Deprecated

- `DEVICE_LOCAL_ID` names the device, as `ARORA_DEVICE` does, when neither
  `--device` nor `ARORA_DEVICE` does, with a warning.
- `DEVICE_DIR`, with a warning to name the device, is the device directory
  when neither `--device-dir` nor `ARORA_DEVICE_DIR` gives one. Given with a
  name, it acts as `ARORA_DEVICE_DIR`. Given alone, the run keeps its Studio
  credential in `<DEVICE_DIR>/studio/`, without store or move, so an
  upgraded runner keeps the credential it had and two such devices never
  share one. `IDENTITY_FILE` given alone does the same in
  `<IDENTITY_FILE>_dir/studio/`.

### Removed

- `device_dir::from_env`: `Device::from_env()?.dir()` is the device
  directory the environment configures.
- `--studio` and `ARORA_STUDIO`: a build with Studio connects without them.
- `studio::Choice::IfSaved`, connecting on a saved credential alone, and
  `Choice`'s `Default`.
- `RunnerArgs::studio`. A runner built with Semio Studio flattens
  `studio::StudioArgs` itself, under a feature of its own that enables
  arora's `studio-bridge`: an arora feature is enabled for every runner a
  build links, so a runner built without Studio beside one built with it
  would otherwise take `--no-studio` although it never connects.
  `RunnerArgs` carries `--module` alone.
- `DEVICE_OWNERS` and the owner prompt: the person approving the device
  chooses its owner in Studio. `DEVICE_DESCRIPTION` goes too: Studio's
  record holds the description.
- The saved registration, `studio/device.json`: a device enrolls once, so
  nothing is resolved again on later runs. A run neither reads nor writes it;
  delete it.

## [15.0.0] - 2026-10-23

### Changed

- **Breaking:** on arora-engine 7, whose types arora re-exports. A device's
  engine carries the component and native executors: a guest loads as a
  component, built for `wasm32-wasip2`. A module directory whose header
  names `wasm`, a `wasm32-wasip1` core module, is refused, naming the
  rebuild it needs.

## [14.1.0] - 2026-10-23

### Added

- `AroraBuilder::with_guest_limits` and `with_module_limits` set what guest
  modules may spend, for all or for one: the time of a call outside a step
  and the memory of an instance (`GuestLimits`, re-exported).
  `Arora::set_guest_limits` changes one module's later, from its next call
  for its budget and from its next load or reload for its memory.

### Changed

- `Arora::run` bounds the guest calls of each step by its period, through
  arora-engine 6.1's `Engine::set_step_budget`: a guest call still running
  when the period ends is interrupted, its call fails, and its module refuses
  its calls until it is reloaded. The device keeps stepping. A call made in a
  step whose time is used up is not made, and its module serves the next
  step. A guest's linear memories are capped at 256 MiB each, unless the
  device's limits say otherwise.

## [14.0.0] - 2026-10-23

### Changed

- **Breaking:** on arora-engine 6, whose types arora re-exports
  (`CompiledModule`, `HostModule`, `ModuleBuilder`). A call to a guest module
  whose arguments do not match the parameters its header declares — a
  parameter the function does not declare, or a value not of its parameter's
  type — fails with `CallError::Generic`, from a remote, the behavior tree or
  another guest alike, and reaches no guest.

## [13.7.0] - 2026-10-23

### Added

- A device runs `wasm-component` modules: its engine carries
  `ComponentExecutor` natively and `BrowserComponentExecutor` in the browser,
  beside the `wasm` and native executors. A module directory whose header
  names `wasm-component` holds its component as `<name>.wasm`.
- Requires arora-engine 5.5.

## [13.6.0] - 2026-10-19

### Changed

- A remote's `spawn_graph` of a graph whose root a live run has answers that
  run's handle, the run taking the graph as its program
  (arora-behavior-tree 9.1), so a client that lost a run's handle takes the
  run over. The device follows that run once, logging the take-over and what
  the graph now calls, rather than as a second run. Requires
  arora-behavior-tree 9.1.

## [13.5.0] - 2026-10-19

### Added

- `--studio` and `--no-studio`, in a `studio-bridge` build. `studio::StudioArgs`
  parses them, and `RunnerArgs` and `DeviceCli` flatten it as their `studio`
  field; `StudioArgs::choice` reads them with `ARORA_STUDIO`, and
  `AroraBuilder::with_studio(studio::Choice)` hands what they ask to `run`.
  The `arora` binary passes it.
  - `--studio` connects to Semio Studio, registering a device that holds no
    saved Studio credential. `ARORA_STUDIO=1` (or `true`, `yes`, `on`) asks
    the same; blank counts as unset, and a value that is not a boolean fails
    the start.
  - `--no-studio` serves the open local bridge, even for a device holding a
    credential. It wins over `--studio` and `ARORA_STUDIO`.

### Changed

- A `studio-bridge` run connects to Semio Studio only when the device holds a
  saved Studio credential (a refresh token that reads, in
  `<device dir>/studio/`) or when the run asks for it. Otherwise it serves the
  open local bridge, on a terminal or not, and logs
  `Studio: no saved credential in <path>; serving the local bridge. Pass --studio to register this device with Semio Studio.`
  - Started without a terminal, such a run used to connect at once, signing a
    device with no credential up as a new anonymous account on Studio's
    production project. On a terminal, it asked the operator for an owner.
  - A device holding a credential connects as before, and logs that it does,
    including a credential a headless run of 13.4 or earlier saved unasked:
    `--no-studio`, or deleting `studio/`, stops it.
  - A credential that does not read (its key missing, the file damaged, an
    `IDENTITY_FILE` naming nothing) no longer signs the device up anew, nor
    fails the start: the run serves the local bridge and warns.
  - The `studio::connect` functions are unchanged: an embedder calling one
    asks for Studio.

### Deprecated

- Connecting on a saved credential alone, what a run that asks nothing does.
  The next major connects only a run that asks for Studio (`--studio`).

## [13.4.0] - 2026-10-19

### Changed

- A guest module and a host module with one id fail the build, as two guest
  modules already do, and so do two host modules with one id. Among the host
  modules are the two every device registers: the HAL module
  (`arora_hal::hal_module::ID`) and the interpreter module
  (`interpreter_module::ID`). Such a build used to succeed with a device whose
  method list and dispatch disagree: the host module registered last took the
  id in dispatch, while `DescribeMethods` still listed the functions of the
  module it replaced, and a call to any of them failed with
  `FunctionNotFound`. The error names both modules: a guest module by its
  header's name, the HAL and interpreter modules as such, any other host
  module by the functions it describes. The two-guest error now names each
  as a guest module (`guest module 'a' and guest module 'b' both have id …`).

## [13.3.0] - 2026-10-19

### Added

- `AroraBuilder::with_module_dirs(device_dir, dirs)`: the modules a device
  runner carries, as module directories: every one under `device_dir`'s
  `modules/`, in name order, then each of `dirs`. Each loads at build as
  `with_module` loads one. At build, the records each directory's `records/`
  carries join the registry given with `with_registry`, whether that registry
  is given before or after, so a module whose record they hold has every
  export described, record-typed ones included.
  - It fails, naming the module, when a directory does not read as a module,
    when one module is in two directories, or when a directory's records do
    not read or differ from a record another directory carries.
  - The build fails, naming the record, when a directory's record differs
    from one the given registry holds under the same id and version, and, as
    for any guest module, when an artifact does not load or a record does not
    describe its module's header.
  - The `arora` binary loads its device directory's and `--module`
    directories with it, as the robot runners of `arora-hal-ros2`,
    `arora-hal-restful` and `arora-hal-naoqi` do.
- `RunnerArgs`: the options every device runner takes besides its own, as a
  clap helper to flatten into the runner's command line: `--module <DIR>` or
  `--module=<DIR>`, repeatable. The robot runners take it; `DeviceCli`'s
  `--module` reads the same.

## [13.2.0] - 2026-10-19

### Added

- `arora::list_keys(store, prefix)`: the keys a store holds or describes,
  with their meta. This is what a device answers `ListKeys` with, and an
  embedder holding a sibling handle of the store reads it without a step,
  including while `run` owns the device.
- `Arora::describe_methods(prefix)`: the device's methods with their
  signatures, as `DescribeMethods` answers, read without a step.

## [13.1.0] - 2026-10-18

### Added

- `AroraBuilder::with_registry(LocalRegistry)`: the records the device's
  guest modules are described by, looked up by exact id and version. A guest
  module whose module record the registry holds, at the version its header
  carries, has every export indexed with the record's frozen signature, so an
  export taking or returning a structure or an enumeration is listed by
  `DescribeMethods` and reachable from a behavior tree. The build fails,
  naming the module, when the record does not describe the header (naming the
  export) or the registry lacks a record it depends on (naming each). A
  record's type must be the header's frozen: a primitive id as that primitive
  (a `Key` as the string, as arora-module-macros 3.0 freezes it), any other
  id as a reference to it. A guest
  module without a record is indexed from its header, as before; a module
  loaded or reloaded on the running device is indexed from its header.
- A module directory may carry `records/` beside `header.json`, in
  arora-registry's layout. `module_dir::registry(&modules)` loads every
  module directory's records into one registry (`ModuleFiles::records_dir`),
  and the `arora` runner hands it to the device. A record two directories
  carry identically is held once, so modules of different scopes sharing a
  type that states its scope load together.
- `LocalRegistry` is re-exported.

### Changed

- Requires arora-types 3.8 and arora-registry 2.1.

## [13.0.0] - 2026-10-16

### Added

- Guest modules change on a running device, between steps:
  - `Arora::load_module(&CompiledModule)` loads one under a free id.
  - `Arora::reload_module(&CompiledModule)` replaces the guest module under
    the compiled module's id in one step, all or nothing: a version that
    does not load leaves the old one loaded, serving and described.
  - `Arora::unload_module(id)` removes a guest module from dispatch and the
    method index.

  Each refuses with a `ModuleError`, with nothing changed: a module already
  loaded under the id, no guest module under it (host modules — the HAL's,
  the interpreter's, an embedder's — stay for the device's life), a function
  id another module has, a native library (its executor cannot unload it:
  restart the device), or a module that does not load.
- What a remote or in-process caller asked of a reloaded or unloaded module
  and the device has not finished is interrupted: a call to it the device
  received and has not applied fails with the reason, as does a request to
  start a task run calling it, and a task run calling it is halted (its
  status key reports `Failure`; the log says it was interrupted, and why).
  The behavior keeps running as it was: nothing checks a new version against
  what the behavior calls, and a node calling a function the module no longer
  has fails as any missing function does.

### Changed

- **Breaking:** requires `arora-behavior-tree` 9, whose
  `BehaviorTreeInterpreter` (re-exported here) is built with `new()` and
  receives the device's function index at each tick. `ModuleFunction` and
  `FunctionIndex` are re-exported from `arora-behavior`.
- Task runs started through `spawn_graph` are followed and logged like those
  started through `spawn`. A run's start is logged at the end of the step it
  started in, naming the function it calls.

## [12.13.0] - 2026-10-13

### Changed

- The `arora` binary's Groot tree may call the functions of the modules the
  device carries (its device directory's and `--module`'s): the tree is
  installed with `AroraBuilder::with_groot`, against the device's method
  index. A tree naming a function the device lacks fails the start; a file
  that does not parse fails it before the device connects. With a Groot tree,
  a graph spawned or loaded by a remote reaches the device's modules too, as
  it does without one.

## [12.12.0] - 2026-10-13

### Added

- The interpreter module's task-run functions join the method index, so
  `DescribeMethods` lists them and `invoke` reaches them by name:
  `spawn(call, policy)`, `spawn_graph(graph_type, graph_version, graph,
  policy)` and `halt(task)`. A call, a graph and the returned handle are
  described as the dynamic key-value type they travel as, a policy as a
  string, a task as a uuid. `load` and `edit` are reached by their ids. A
  module function under one of the interpreter module's ids fails the build.

### Changed

- A loaded module that exports `spawn`, `spawn_graph` or `halt` shares the
  name with the interpreter module: `invoke` needs that module's id to reach
  it, and a bridge that invokes by name alone finds the name ambiguous.
- Depends on arora-behavior-tree 8.5.

## [12.11.0] - 2026-10-13

### Added

- The interpreter module answers `interpreter_module::SPAWN_GRAPH`: a remote's
  `Call` starts a graph, of a type the device's interpreter reads, as a task
  run and gets its handle back.
- `LocalCaller::spawn_graph(graph_type, graph)`: the in-process counterpart,
  answering with the run's handle; `halt` stops the run.

### Changed

- Depends on arora-behavior 9.2 and arora-behavior-tree 8.3.

## [12.10.0] - 2026-10-12

### Added

- **A device runs the string nodes with no module loaded.** `SetString`,
  `UnsetString`, `IsStringSet`, `WaitStringSet` and `RegexMatch` are core
  behavior-tree nodes now, native like the control nodes
  (`arora-behavior-tree` 8.2.0): a Groot tree installed with `with_groot`,
  `load_groot` or `--groot` uses them without the test module
  `test-behavior-tree-nodes`. A port bound to a key that is not set yet
  reads as unset — `IsStringSet` fails, `WaitStringSet` keeps running — and
  `SetString` creates the key it writes.
- **A module function's optional parameter bound to an unset key receives
  `None`** rather than failing the tick, and an optional output or return
  sets or unsets its key.

## [12.9.0] - 2026-10-12

### Added

- **A device registered once with Studio is asked nothing again.** Under the
  terminal UI, a `studio-bridge` run that asked the operator anything saves
  the owners and name it resolved — and the model family the operator typed,
  if any — in `studio/device.json` in the device directory, before it
  connects. Later runs register with it and ask nothing, logging
  `studio: registering as <name> for owner(s) <uids> (saved in <path>; delete it to answer again)`.
  Each field takes the first source that gives it: its env var
  (`DEVICE_OWNERS`, `DEVICE_NAME`, `MODEL_FAMILY`), then the saved
  registration, then the operator. Leaving the owner empty still runs without
  Studio, and saves nothing.
- **The model family comes from the HAL.** `AroraBuilder::run` passes the
  HAL's description (`Hal::describe`) to the Studio connection: the model
  family is the HAL's unless `MODEL_FAMILY` is set — the operator is no longer
  asked for it, and the HAL's wins over a saved one — and so are the hardware
  and software versions unless `HARDWARE_VERSION` / `SOFTWARE_VERSION` are.
- `studio::connect_with_operator_and_description` and
  `studio::connect_with_description`: `connect_with_operator` and `connect`
  completing the device info with an `arora_hal::HalDescription`.

### Changed

- **A headless run registers the saved registration.** `studio::connect` and
  `studio::connect_with_device_dir` complete the device info the environment
  gives with the registration saved in the device directory, and register it
  when the environment gives none — so a device set up once under the
  terminal UI can then run headless. A device directory no interactive run
  wrote to registers exactly what the environment gives, as before.
- A blank device-info env var counts as unset for a headless run too, as it
  already did under the terminal UI.

## [12.8.0] - 2026-10-12

### Added

- **The terminal UI lets the operator select text.** `m`, outside a prompt,
  toggles the mouse between scrolling the log pane (the default) and the
  terminal's own selection, so log lines can be selected and copied; `m`
  again gives the wheel back to the log pane. The footer says what the mouse
  does now and what `m` switches it to. Like `q`, `m` is the UI's own key: a
  `TuiCommand` registered on it never fires.
- **The device says what it runs.** Each call the interpreter module serves
  logs one line at info level, once it succeeded: a behavior loaded (`behavior
  loaded: 12 node(s), running from the next step`, or `behavior loaded from a
  Groot tree: …` for `Arora::load_groot` and `AroraBuilder::with_groot`), a
  behavior edited (the nodes and links it adds and removes), a task started
  (its id, the function it runs, its run policy), a task halted. A task run
  the device started is logged once more when it ends — `succeeded`,
  `failed`, or `halted`. The behavior finishing, and recovering after a
  failed tick, are logged once each; its failures already were.

## [12.7.0] - 2026-10-12

### Added

- **A device's HAL opens its own inputs.** The runtime states what the HAL
  says of its keys (`arora_hal::Hal::describe_keys`) to the store's key meta:
  the description the HAL holds when the device is built, then each later one
  at the next step, before that step applies any write. The HAL's inputs are
  open to remote writers with their declared type, its outputs closed, and
  every bridge lists them (`ListKeys`) before anything writes them. A key
  whose own meta the host states keeps it: the HAL's description fills the
  keys with none and replaces only what it stated itself, so a host narrows or
  closes one of its HAL's inputs by stating its meta. A HAL that describes no
  key leaves the device as it was.
- `LocalCaller::update`: write a change to the device's store as a remote
  does, checked against the store's key meta like every bridge's write.

### Changed

- A key the HAL describes takes the HAL's meta over a prefix's, as any key's
  own meta does: under `--open`, or in a subtree a host opened, the HAL's
  outputs are closed and its inputs take only their declared type. Every key
  the HAL does not describe keeps what the store says of it. `--open`'s help
  says so.
- Requires `arora-hal` 4.3.

## [12.3.0] - 2026-10-10

### Changed

- **A device's state keys reach Studio as the device wrote them.** The
  `studio-bridge` feature depends on `arora-studio-bridge-client` 10
  (studio-bridge msgs 7): each state key is published verbatim as its own
  Zenoh key, `state/{device}/{key}`, with only the characters Zenoh reserves
  `%XX`-escaped (and an empty segment written `%`), so Studio reads back
  `arora/time`, `animation/players`, `joint1.position` and
  `camera_front.resolution.width` unchanged. Studio's commands name keys the
  same way, and the device announces protocol 3
  (`mgmt/{device}/liveliness/protocol/3`).
- **Devices and Studio move together.** A device on this version and a Studio
  on a studio client older than 10 misread each other: the Studio reads a key
  holding both a `/` and a `.` as another key (`robot1/joint1.position` as
  `robot1.joint1.position`), and a studio client 9.1 that addresses its
  commands has those touching a key with a `.` or an empty segment refused by
  the device. Conversely, a Studio on studio client 10 reads a key holding a
  `.` from a device on an earlier `arora` as another key (`joint1.position` as
  `joint1/position`). A router's storage keeps the keys a device on an earlier
  `arora` published, in the old form, which a studio client 10 reads as other
  keys, so restart the router once its devices run this version.

## [12.2.0] - 2026-10-09

### Added

- The HAL module: every device registers its HAL as a module under
  `arora_hal::hal_module::ID`, whose described functions serve the HAL's
  components' models (`HalAssets`): `models()` lists them, `model_glb(component)`
  returns one model's GLB and `model_glbs()` every servable one's. A model the
  HAL does not state servable is never served. A HAL without assets has no
  models.

### Changed

- Depends on arora-hal 4.1.

## [12.1.0] - 2026-10-06

### Added

- `AroraBuilder::with_declared_module::<M>` loads a module declared in Rust
  as a guest executable with the declaration's frozen signatures, so every
  export — a `Status`-returning behavior leaf, a structure-typed parameter —
  joins the method index and is reachable from a behavior tree. `with_module`
  can index only primitive-typed exports, because a bare header carries no
  type versions; `build` keeps a declaration's signature over the primitive
  freeze it would otherwise derive from the header.
- `Arora::load_groot` installs a Groot tree into a built device: its tags
  resolve against the device's own method index and the lowered graph loads
  through the interpreter module. The `arora` binary's Groot option covers a
  device with no modules; a device that loads modules needs the index the
  builder assembled.
- `AroraBuilder::with_groot` hands the builder a Groot tree, which `build`
  resolves against the method index it assembled and installs, failing the
  build when the tree does not load. A device run through
  `AroraBuilder::run` has no built device to call `load_groot` on; this is
  how its tree reaches modules' leaves.
- `AroraBuilder::with_step_period` sets the step period `run` drives the
  device at (default `Arora::DEFAULT_STEP_PERIOD`), for a behavior sampled
  at a fixed control rate.

## [12.0.0] - 2026-10-06

### Changed

- **Breaking: a remote write must be of the type its key states.** A bridge
  `Update` whose value for a key is not of the type the key's meta states
  (`KeyMeta::ty`, checked with `Value::conforms_to`) is refused, beside the
  check that the key is an input, and writes nothing: the change is applied
  whole or not at all. The refusal names each such key with the type it
  states and the type it was sent — `not of the key's declared type:
  face/mouth (expected F64, got String)`. There is no conversion: an `F32`
  sent to an `F64` key is refused. An unset, a set to no value, and a key
  whose meta states no type are not checked. The device's own writers (its
  HAL, behavior and modules) are not checked either.
- Depends on arora-types 3.5.

## [11.9.0] - 2026-10-06

### Added

- `AroraBuilder::with_start_time(Duration)`: the time the device's clock reads
  before its first step, zero by default. The first `step(dt)` publishes
  `arora/time = start + dt` and `arora/dt = dt`, so a device that joins peers
  which have already run takes their timeline with an ordinary first frame;
  devices started at one time and stepped with the same `dt`s read the same
  `arora/time` at every step. The clock is set at build only.

## [11.8.0] - 2026-10-06

### Added

- `AroraBuilder::with_compiled_module(&CompiledModule)`: load a guest module
  compiled once into any number of devices. Each device instantiates the
  compiled code instead of compiling the executable, and the module goes
  through the checks `with_module` does at `build` (one module per id, its
  primitive-typed exports joining the method index).
- Re-exports `arora_engine::compiled::CompiledModule`.

### Changed

- `with_module` compiles its module at `build`, for that device alone, then
  loads it as `with_compiled_module` does.
- Depends on arora-engine 5.2.

## [11.7.0] - 2026-10-06

### Added

- `DescribeMethods` lists a guest (wasm) module's functions that take or
  return an optional over a scalar primitive (`Option<u64>`, `Option<String>`),
  with the parameter or return typed as a `FrozenOption` over the primitive —
  the signature the module's record declares for it. `LocalCaller::invoke` and
  remote clients call them by name; an optional argument may be left out,
  which the guest reads as `None`. A function naming a record type, a map or a fixed-length array still
  dispatches without being listed: a record needs a type registry to pin its
  version.

## [11.6.1] - 2026-10-06

### Changed

- A step lends its outbound change to the HAL instead of copying it when no
  key has to be left out — none of the HAL's own readings is still a
  frame-final value, which is every frame of a HAL that reports nothing. Only
  a frame that has readings to leave out builds the HAL a change of its own.
  What the HAL receives is unchanged.

## [11.6.0] - 2026-10-05

### Added

- `LocalCaller` has every operation a remote client has over a bridge, beside
  `call`: `list_keys` and `describe_methods` (introspection), `invoke` (call a
  method by name with arguments by parameter name; a task-shaped method is
  spawned and answers with its run's handle, `Invoked::Started`; an optional
  module id chooses among modules exporting one name, which otherwise fails
  naming them), `spawn` (start a call as a concurrent task run) and `halt`.
  Each sends the `BridgeOp` a remote sends, queued before the method returns
  and applied at the next step, in order with every other inbound op; `invoke`
  reads the signature on one step and applies its call on the next.
- Re-exports `TaskHandle`, `TaskId` and `MethodSignature`, which those
  operations answer with and take.

### Changed

- Depends on arora-bridge 6.1.

## [11.5.0] - 2026-10-05

### Changed

- **A device reaches a bridge that authenticates it.** The `studio-bridge`
  feature depends on `arora-studio-bridge-client` 9.1 and connects with the
  device's Studio credentials: over a TLS endpoint (`STUDIO_BRIDGE_ENDPOINT=tls/…`)
  the client obtains from Studio a certificate whose Common Name is the
  device's principal, keeps it with its key in the device directory's `studio/`
  subdirectory, and renews it while it runs. Over `tcp/` nothing changes.

## [11.4.0] - 2026-10-03

### Changed

- A device answers `ListKeys` and `DescribeMethods` over the Studio bridge: the
  `studio-bridge` feature depends on `arora-studio-bridge-client` 9 (studio-bridge
  msgs 6, whose `AroraOp` carries both), so a Studio's `listKeys` and
  `describeMethods` reach the device's `BridgeOp::ListKeys` and
  `BridgeOp::DescribeMethods` as every other bridge's do.

## [11.3.0] - 2026-10-02

### Added

- **The device loads the modules in its directory.** Every module directory
  under `<device directory>/modules/` is loaded at start, in name order, and
  `--module <DIR>` (repeatable) adds one from elsewhere. A module directory is
  `header.json` — the module's low-level `Header`, as JSON — beside the one
  file with the extension the header's executor names: `.wasm` for `wasm`,
  the platform's dynamic library (`.so`, `.dylib`, `.dll`) for `native`. The
  artifact's name is free, so a published artifact directory (the
  `@vizij/animation-module` package's `artifact/`) is a module directory as
  is. Entries whose name starts with `.` (OS metadata such as `.DS_Store` or
  an AppleDouble `._x.wasm`) are neither module directories nor artifacts. A
  module the device cannot load — a missing or malformed header, no artifact
  or several, an executor it does not run, one module in two directories, an
  artifact the engine rejects — fails the start, naming the module. A loaded
  module's functions are reachable by any call and `DescribeMethods` lists
  their primitive-only signatures. `module_dir::read` and
  `module_dir::in_device_dir` read module directories for an embedder;
  `DeviceCli::modules` reads what the command line names, for `with_module`.
- `device_dir::from_env()`: the device directory of a run configured from the
  environment — `DEVICE_DIR`, else `<IDENTITY_FILE>_dir` while that deprecated
  variable is set (with a deprecation warning), else the per-user directory of
  the device `DEVICE_LOCAL_ID` names. `device_dir` is part of every native
  build, no longer of the `studio-bridge` feature alone.
- `standard_frontend()`: the front end `run` picks when none is injected, for
  a binary that logs before `run` — the `arora` binary installs it before it
  reads its module directories, so what the loading says is captured.

### Changed

- Two guest modules with one id fail the build, naming both; the engine would
  load the first and the method index describe the last.
- A guest module the engine cannot load fails the build naming the module (its
  header's name and id) along with the engine's reason, and each guest module
  loaded is logged with its name and id.

## [11.2.0] - 2026-10-02

### Added

- A `ListKeys` answer carries a key's unit (`KeyMeta::unit`, arora-types 3.2)
  with the rest of its meta, so every bridge relays it as it relays the range.

### Changed

- Depends on arora-types 3.2.

## [11.1.0] - 2026-10-02

### Added

- **The device directory**: what a device keeps of its own from one run to the
  next, each use in a subdirectory of its own. It is
  `<data_local_dir>/semio/arora/devices/<local id>` (`~/Library/Application
  Support` on macOS, `~/.local/share` on Linux, `%LOCALAPPDATA%` on Windows),
  the local id coming from `DEVICE_LOCAL_ID`, `default` when unset; two devices
  on one host set different ids. `DEVICE_DIR` replaces the whole path, and so
  does `studio::connect_with_device_dir(&Path)` for an embedder whose platform
  gives it a data directory (Android, a Tauri app). `device_dir::of(local_id)`
  gives a local id's per-user directory. The README shows the tree.

### Changed

- **The Studio credentials live in the device directory's `studio/`**, kept by
  arora-studio-bridge-client 8.1's `DeviceCredentials`, which alone writes
  inside it. Rebuilding, reinstalling or moving the binary no longer registers
  the device anew. Credentials from arora 11.0 and earlier (`.semio/arora`
  under the executable's directory, the home directory or the current
  directory) move into `devices/default/studio/` at the first start, so the
  device keeps its Studio identity.
- No file is written to probe a directory's writability; the directories and
  files are readable by the current user alone on Unix.
- A saved refresh token that cannot be read says why when the device signs in
  anew.
- Depends on arora-studio-bridge-client 8.1, and no longer on
  `crypto_secretbox`.

### Deprecated

- `IDENTITY_FILE`. The device directory of a run that sets it is
  `<IDENTITY_FILE>_dir`: the file is copied there with the key it decrypts
  under, and is neither updated nor removed afterwards. Each run warns of the
  deprecation and that the file can be removed; a run with the file absent
  falls back to the directory. With neither the file nor the directory
  present, the run fails.

## [11.0.1] - 2026-09-29

### Fixed

- The `studio-bridge` feature builds: it depends on arora-studio-bridge-client 8,
  which implements the arora-bridge 6 `Bridge` the rest of arora speaks.

## [11.0.0] - 2026-09-29

### Added

- `arora --open`: every key is an input, for a sandbox or a bench. Without it the
  binary's device accepts no remote writes until something opens a key.
- `serve_local_ws_bridge(Arc<AroraWSServer>)`: serve a server you built — bound,
  spawned, and cancelled when the returned bridge is dropped, exactly as
  `local_ws_bridge` does it. The caller keeps the server, so it can reach it at
  any point in the run. `local_ws_bridge_with` is one line of it.

### Changed

- **Breaking:** `BridgeOp::ListKeys` answers with each key's
  `KeyMeta` — the store's meta, with the shape of the value it holds filled in
  where the store says nothing, and every key the store describes even if nothing
  has written it yet. A bridge relays that instead of keeping its own account of
  the device's keys.
- **Breaking:** a `BridgeOp::Update` reaches only the keys the device opened
  (`KeyMeta::editable`); any other is refused, naming the key. This is every
  bridge's inbound write — ws, ROS 2, Studio — checked once on the way in, so a
  device that opens nothing accepts no remote writes. The device's own writers
  (HAL, modules, behavior) are never checked.
- **Breaking:** depends on arora-types 3.1, arora-bridge 6 and arora-bridge-ws 7,
  the last of which this crate re-exports as `arora::bridge_ws`.

## [10.3.0] - 2026-09-28

### Added

- The methods the behavior interpreter describes (arora-behavior 9.1's
  `described_methods`) join the method index under the interpreter module,
  so a remote discovers and spawns them like any task run. A direct call to
  one fails, saying to spawn it. A function id described both by a module
  and by the interpreter fails the build.

## [10.2.0] - 2026-09-28

### Added

- The re-exported `HostModule` has `from_exports`: depends on arora-engine
  5.1.

## [10.1.0] - 2026-09-28

### Added

- `local_ws_bridge_with(ServerConfig)`: the open local bridge on another port, on
  a LAN-facing address, or serving the control panel — everything else is
  `local_ws_bridge`, so an app that needs one of those no longer rebuilds the
  bind, the serving task and its cancellation.
- `arora::bridge_ws`: the open local bridge's crate re-exported, so an embedder
  names `ServerConfig` through the arora it serves.

### Changed

- Depends on arora-bridge-ws 6, where a client reaches the device's own methods
  and subscribes to the keys it wants.

## [10.0.1] - 2026-09-26

### Fixed

- The `studio-bridge` feature builds: it depends on arora-studio-bridge-client
  7, which implements the arora-bridge 5 `Bridge` trait.

## [10.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3, arora-engine 5, arora-behavior 9,
  arora-behavior-tree 8, arora-bridge 5, arora-bridge-ws 5, arora-hal 4 and
  arora-simple-data-store 3.
- The `studio-bridge` feature does not build until arora-studio-bridge-client
  is released against arora-bridge 5: the client implements
  `arora_bridge::Bridge`.

## [9.11.0] - 2026-07-30

### Added

- Loaded guest (wasm) modules become discoverable: each export whose parameters
  and return are all primitives joins the device's method index with a frozen
  signature, so `DescribeMethods` lists it — the guest counterpart of 9.10's
  host-module discoverability. A guest header carries no type versions, so an
  export with a non-primitive type dispatches but stays undiscoverable (it
  needs a registry to pin versions); it is logged at build.

## [9.10.0] - 2026-07-29

### Added

- Host modules become discoverable: the described functions of every
  `with_host_module` module join the device's method index, so introspection
  (`DescribeMethods`) lists them with their signatures — including the
  action-shaped ones the ROS 2 bridge turns into actions. Declare them with
  `ModuleBuilder::described_function`; an undescribed function still
  dispatches, it is just not listed.

### Fixed

- The method index was never populated: `DescribeMethods` answered with an
  empty list on every built device, whatever modules it loaded.

## [9.9.0] - 2026-07-29

### Changed

- Re-pinned to `arora-behavior-tree` 7 — the runner-scaffold interpreter with
  reactive tick semantics (one runtime tick is one behavior-tree tick). Task
  runs and the loaded behavior are now tree structure under a parallel runner,
  introspectable through the interpreter's graph; the interpreter is a standing
  policy (always `Running`, never dropped). The binary's Groot flow loads with
  `load_groot(xml)` — the tree binds to the device's store at its first tick.

## [9.6.0] - 2026-07-27

### Added

- `arora::local_ws_bridge()`: the open local bridge constructor, now public.
  `AroraBuilder::run` still attaches it for you when you inject no bridge; a host
  that composes its own bridge set (the open local bridge *and* a ROS 2 or Studio
  bridge) attaches it explicitly with `with_bridge(local_ws_bridge().await?)`,
  the same way `studio::connect()` is composed.

## [9.5.0] - 2026-07-27

### Added

- `AroraBuilder::with_frontend(frontend)`: inject the operator front end
  instead of the standard terminal-detection pick.
- Application commands in the terminal UI: `tui::commands_frontend(commands)`
  builds the standard front end with key-driven commands (footer label,
  optional text prompt) delivered on a channel — `tui::TuiCommand`,
  `tui::TuiCommandEvent`.

### Changed

- Stopping a run is dropping its future, now as a documented, complete
  teardown: the operator flow polls the access-request serving in its own
  scope (no detached task), so dropping `AroraBuilder::run`'s future
  synchronously releases the front end (the terminal), the device, and its
  bridges; `Arora::run` documents drop-between-steps cancellation.

## [9.4.0] - 2026-07-27

### Added

- `DeviceCli`: the standard device binary's command line as a clap helper
  (native-only); the Groot argument moves there.

### Fixed

- The library no longer reads argv: `run`/`run_with`/`run_with_hal` and the
  run funnel took the process' first argument as a Groot file, which broke
  any embedding binary with its own CLI. The `arora` binary parses
  `DeviceCli` and composes the behavior-tree load itself.

## [9.3.0] - 2026-07-24

### Changed

- Re-pinned to `arora-behavior` 7 (the `LinkSource::Select` change, ARORA-72).
  No engine behavior change.

## [9.2.0] - 2026-07-23

### Changed

- Adopt `arora-behavior` 6's "golden"→"built-in" rename (ARORA-59): the runtime
  publishes the built-in clock keys via `arora_behavior::built_in`. Public API
  and the `arora/time`/`arora/dt` wire keys are unchanged.

## [9.1.0] - 2026-07-21

### Changed

- A failing behavior no longer stops the device: the step (and so `run`)
  goes on — the HAL keeps being driven, the behavior is ticked again next
  step. The failure is logged once per distinct message and stands on a
  watch — `Arora::behavior_error` hands a receiver that outlives `run`
  owning the device, so a self-paced device's embedder still observes it;
  nothing reaches the store.
- `LocalCaller` enqueues a call before `call` returns rather than when the
  returned future is first polled, so firing and stepping the device in the
  same breath lands the call on that step — a JS Promise, in particular, is
  first polled a microtask later.

### Fixed

- `run` yields its thread even when every step overruns the period: the
  metronome's overrun arm now suspends through the timer, so a saturated
  device shares its thread — on the web, an overrunning device used to
  freeze the whole page.
- `arora-studio-bridge-client` re-pinned to 6: claims inform the embedding
  application and no longer filter commands.

## [9.0.2] - 2026-07-21

### Changed

- `arora-studio-bridge-client` re-pinned to 5: the client now watches the
  device's per-studio claim set (`DeviceClient::claims`) and refuses a
  command whose named caller has not claimed the device. Nothing changes
  in this crate's surface — claims stay outside the runtime.

## [9.0.1] - 2026-07-21

### Fixed

- The optional `studio-bridge` feature compiles again:
  `arora-studio-bridge-client` re-pinned to 4, the release built on this
  crate's own arora-bridge 4 / arora-types 2.

## [9.0.0] - 2026-07-20

### Breaking

- The step-phase functions are private: the step pipeline is internal, `step`
  and `run` are the surface.
- `StepOutcome` is gone — `step` returns `Result<(), RuntimeError>`, and a
  remote reporting the device unregistered no longer stops it: the device keeps
  serving whoever else it is attached to.
- `Telemetry`/`TelemetrySnapshot`/`Arora::telemetry` are gone — indicators are
  state. `ReadyHook` hands a store `Subscription` (opening on the whole current
  state); the TUI derives the loop rate from `arora/dt`.
- The clock (`arora/time`, `arora/dt`) travels outbound like any other state;
  a consumer that does not want it filters it on its side.
- Outbound bridge writes are gated per endpoint on `Inbound::DataRequested`:
  a change reaches only the remotes that asked. The HAL is written either way.

### Added

- `Arora::call` dispatches a `Call` in-process, synchronously, between steps —
  the same module-scoped path a bridge command takes, including the
  interpreter module's LOAD/EDIT.
- `LocalCaller` (from `Arora::caller()`), the in-process `arora_bridge::Caller`:
  dispatch `Call`s into the device while `run` owns it — enqueued immediately,
  applied at the next step's event phase like a remote's, resolved on that
  step's reply.
- `Arora::engine()` exposes the engine's `CallBridge` seam (registering
  in-process callables).

## [8.1.0] - 2026-07-15

### Added

- `studio::connect()` is exposed as an injectable Studio Bridge.

### Changed

- The `studio-bridge` integration runs on `arora-studio-bridge-client` v3.1: the
  Firebase config and bridge endpoint are baked in, and the device prompts
  interactively for the owner / device name / model family on first connect.

## [8.0.0] - 2026-07-12

### Breaking

- `with_module` loads a guest module; host functions move to `with_host_module`.

## [7.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [7.0.0] - 2026-07-10

### Breaking

- The interpreter is a module — generic function modules replace HostFunction

### Added

- Predetermined slots bind to the store unless linked (API-consistency PR 6)

### Fixed

- A dead server ends the inbound stream; run_with_hal fails fast on bind

## [6.0.0] - 2026-07-10

### Breaking

- 6.0.0 — the runtime's call dispatch is part of its face
- The golden behavior edit — a Call reaches interpreter.apply through the engine (PR 5b)
- Dispatch BridgeOp::Call through the engine (API-consistency PR 5a)

## [5.0.0] - 2026-07-09

### Breaking

- 5.0.0 — the graph-model interpreter is part of arora's face
- 4.0.0 — the graph-lowered interpreter is its own major
- Call_bridge, and edition that defers lowering to the tick
- Shared graph model + GraphDiff + BehaviorInterpreter::apply

## [4.1.0] - 2026-07-09

### Breaking

- 1.0.0 — the engine on crates.io predates the workspace by months
- 3.0.0 — the empty-ready interpreter is its own major
- Echo-free frame — change-only store feed, HAL-origin subtraction, non-blocking HAL sends
- Stage the Studio connection out for the release ordering

### Added

- The Studio connection returns over the published client

## [4.0.0] - 2026-07-09

### Breaking

- Design B — run drains the seams; the device owns them
- Inject the interpreter once at build; async fixed-interval run
- One Arora builder, fold Runtime, functional step
- Delete the io pump; step drives the sync bridge/HAL seams

### Fixed

- Gate anyhow macro import so wasm build is warning-clean
- SilentBridge in namespaced-store test; rustfmt

### Changed

- Build the feature against arora-bridge 2's sync Bridge

## [3.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

## [2.0.0] - 2026-07-08

### Breaking

- Rename Behavior trait to BehaviorInterpreter

## [1.0.0] - 2026-07-08

### Breaking

- Golden clock keys (time/dt) in the store; drop ctx.dt
- Rename arora-websocket to arora-bridge-ws

### Added

- A terminal operator UI — logs, indicators, and the prompt line (ARORA-51)

### Fixed

- Accept arora-websocket 1.x

## [0.2.0] - 2026-07-06

### Breaking

- One run family at the crate root — arora::run()
- Drop the private semio-record dependency — type records live in arora-types

### Added

- Headless runner accepts an injected HAL (launch_with_hal)
- The Studio connection is opt-in — new `studio-bridge` feature
- Headless registers device info from the env
- Headless device runner as arora's binary
- Bundle the Zenoh bridge deps (validated, headless bin WIP)
- Runtime over Arc<dyn DataStore> + a NamespacedStore (#108)
- Bridge introspection — ListKeys / ListMethods (ARORA-42)
- A Behavior the runtime ticks, in an arora-behavior crate (VIZ-33)
- Bind behavior-tree variables to the data store
- Launch and launch_with take an injectable data store
- Launch_with — build the bridge inside arora's runtime
- Worked example of a device-specific arora
- Boot the binary on the portable runtime
- Run the opinionated arora runtime on wasm
- Single-thread, step-dispatched runtime loop (Phase 4b)

### Fixed

- Crates.io forbids wildcard version constraints
- Clippy ptr_arg in headless app-data (&PathBuf -> &Path)
- Update the device example for the data-store launch API

### Changed

- Depend on published arora-studio-bridge-client (crates.io)
- Point studio-bridge deps at main (reqwest-0.13 migration merged)
- A behavior writes + switches a namespaced store key (ARORA-39) (#109)
- Native control nodes; free arora of the BT-nodes module
- De-flake the runtime-loop tests (sleep, not yield)
- Extract the launcher into an injectable library API
- Portable single-thread runtime (sync step + async io pump)
- Group module-authoring crates under crates/arora-module-authoring/

## [0.1.0] - 2026-06-26

### Added

- Add the opinionated `arora` runtime wrapper
- Surface guest TYPE_ERROR as DispatchError::Guest / CallError::Guest
- Tick() with arguments, return_binding, and variables

### Fixed

- Resolve hand-written lints; allow on generated subtrees
- Track getrandom 0.4 for the wasm32-unknown-unknown browser build
- Remove cdylib from arora crate-type to fix duplicate arora-types compilation
- Use normalized_value() instead of deprecated unescape_value()

### Changed

- Rename the engine crate `arora` -> `arora-engine`
- Bring arora-types into the workspace as a path crate
- Repoint behavior-tree / web / nao / polly links to their new repos
- Exercise the browser executor in CI at the engine level
- Source the CallBridge interface from arora-types
- Make arora schema re-export private to avoid type namespace confusion
- Remove cos wrapper, fix build warnings


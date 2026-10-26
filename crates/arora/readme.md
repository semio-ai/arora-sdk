# arora

The **opinionated Arora runtime**. Where [`arora-engine`](../arora-engine) is
the bare, unopinionated engine, this crate wires the whole device into one
object, [`Arora`]: the engine (with the WebAssembly and native executors), the
shared data store, the HAL and bridge I/O seams, one behavior interpreter, and
the step loop that drives them. The core behavior-tree nodes — the control
nodes and the string nodes (`SetString`, `UnsetString`, `IsStringSet`,
`WaitStringSet`, `RegexMatch`) — are native in
[`arora-behavior-tree`](../arora-behavior-tree), so no module needs to be
loaded to run a tree of them.

> **Architecture, with diagrams:** [`docs/runtime-and-data-flow.md`](docs/runtime-and-data-flow.md)
> shows all of the device's components and how data flows between them, plus a
> sequence diagram of one `step`. It pairs with
> [arora-behavior's interpreter workflow](../arora-behavior/docs/interpreter-workflow.md).

## Build and run it

The crate is a default workspace member, so `cargo build` produces the `arora`
binary: the headless device runner — no HAL-attached display; a head like Vizij
embeds the library instead. It reads its configuration from the environment
and serves the open local bridge on `ws://127.0.0.1:9000`. A build with the
`studio-bridge` feature connects to Semio Studio instead when the run passes
`--studio` ([Semio Studio is opt-in](#semio-studio-is-opt-in)). The front end is the terminal operator UI when the process is attached to a
terminal (feature `tui`), headless otherwise. In the terminal UI the wheel and
PgUp/PgDn scroll the log pane, `m` hands the mouse to the terminal to select
and copy text (and `m` again takes it back), and `q` or Ctrl-C quits. The log
says when a behavior is loaded or edited and when a task starts, is halted and
ends.

```sh
cargo build                      # builds arora into target/debug/arora

# install a Groot behavior tree as the device's behavior, then serve; its
# leaves may call the functions of the modules the device carries
./target/debug/arora crates/arora/examples/hello_tree.groot.xml

# or just serve, waiting for a behavior over the bridge
./target/debug/arora

# load a module from a directory besides those in the device directory
./target/debug/arora --module node_modules/@vizij/animation-module/artifact
```

At start the device loads every module in its device directory's `modules/`
(the layout is [below](#where-a-device-keeps-its-data)), then each `--module`
directory. A module that cannot be loaded fails the start, naming it. The
robot runners — `arora-ros2` ([`arora-hal-ros2`](../arora-hal-ros2)),
`arora-restful` ([`arora-hal-restful`](../arora-hal-restful)) and
`arora-naoqi` ([`arora-hal-naoqi`](../arora-hal-naoqi)) — load their modules
the same way, and take `--module <dir>` too.

## Use it as a library

Build a device with the [builder](src/lib.rs): pick a data store, a HAL, zero
or more bridges, the modules whose functions behaviors may call, and the
behavior interpreter — each with a default, so `Arora::builder().build()` is a
self-contained in-process device (fake HAL, no bridge, an empty behavior, a
private `SimpleDataStore`). Drive it with `step` once per frame, or `run`, the
visible loop over `step`. The run family at the crate root is sugar over the
builder:

```rust,no_run
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The default device: in-process fake HAL, the open local bridge.
    arora::run().await
}
```

```rust,no_run
// A device over your hardware — the one call that turns a HAL into a running
// device. `run_with` also takes the bridge and the store; `run_with_frontend`
// the operator front end as well.
arora::run_with_hal(Box::new(my_hal)).await?;
```

Stopping a run is dropping its future: everything the run holds lives inside
it, so dropping it is a complete teardown, after which a host can build and run
a fresh device.

A device-specific Arora is built from the outside, with no fork and no
per-device feature flag: the [`device` example](examples/device.rs) provides
its own `Hal` and hands it to `run_with`; swap in a real robot HAL and the
studio-bridge connector and it is a real device build. `DeviceCli` is the clap
options the binary parses (the Groot file, `--open`, `--module`, and in a
`studio-bridge` build `--studio` and `--no-studio`); flatten it into a larger
CLI and inject the results through the builder. A runner's
modules, its device directory's and those its command line names, load with
`AroraBuilder::with_module_dirs(&device_dir::from_env()?, &dirs)`, records
included, as the `arora` binary and the robot runners load theirs. A runner
with a command line of its own flattens `RunnerArgs` into it for `--module`,
and, under a feature of its own enabling `studio-bridge`, `studio::StudioArgs`
for `--studio` and `--no-studio`, whose `choice()` it hands to
`AroraBuilder::with_studio`, as the robot runners do. The records join the registry `with_registry` gives
when the device is built, whichever of the two calls comes first.

On wasm, build the library with `--no-default-features`: the `native` feature
carries the wasmtime and dynamic-library hosts, the operator flow, and the
binary.

## Where a device keeps its data

A device keeps what is its own from one run to the next in its **device
directory**, each use in a subdirectory of its own:

```text
<data_local_dir>/semio/arora/      per user: ~/Library/Application Support (macOS),
│                                  ~/.local/share (Linux), %LOCALAPPDATA% (Windows)
└── devices/
    └── <local id>/                the device directory: DEVICE_LOCAL_ID, `default` when unset;
        │                          DEVICE_DIR replaces this whole path
        ├── modules/               the modules the device loads at start, in name order
        │   └── <name>/            a module directory (what --module names elsewhere):
        │       ├── header.json    the module's header (its low-level Header, as JSON)
        │       ├── *.wasm         its artifact: the one file with the executor's extension —
        │       │                  .wasm for wasm, .so / .dylib / .dll for native
        │       └── records/       optional: the module's record and the records it depends
        │                          on, in arora-registry's layout (module/, structure/, …)
        └── studio/                the Studio connection's credentials, kept by
            ├── key                arora-studio-bridge-client's DeviceCredentials
            └── refresh_token
```

The local id tells apart the devices one user runs on one host. It is local
only: the name a device shows can change without making it another device, and
the id Studio knows it by is assigned when it enrolls.
Anything else a device keeps goes in a subdirectory of its own beside these.

A module directory is the layout a module is published in: the
`@vizij/animation-module` package's `artifact/` — `header.json` beside
`vizij_animation_module.wasm`, and its `records/` when it ships them — copied
to `modules/animation/` is loaded at the next start. The artifact's name is free; its extension names it, so a module
directory holds one artifact. Entries whose name starts with `.` are neither
module directories nor artifacts: OS metadata (`.DS_Store`, an AppleDouble
`._x.wasm`) lands beside what it describes and is ignored. A module the device
cannot load — a header it cannot read, no artifact or several, an executor it
does not run, one module in two directories, an artifact the engine rejects —
fails the start, naming the module. A loaded module's functions are reachable
by any call — in-process (`Arora::call`), over a bridge. `DescribeMethods`
lists every function of a module whose record the device holds — the records
of every module directory fill one registry — with the record's signature;
a record that does not match its module's header, or that depends on a record
no directory carries, fails the start, naming the module. Without a record, a
function whose parameters and return are all primitives or optionals over a
scalar primitive is described; any other — one naming a record type, a map or
a fixed-length array — still dispatches
([`module_discovery`](src/module_discovery.rs)). A Groot tree the binary
installs resolves its tags against the device's method index: its nodes are
the native core nodes and the functions of the modules the device carries.
[`module_dir`](src/module_dir.rs) reads module directories for an embedder.

### Semio Studio is opt-in

A `studio-bridge` build contacts Semio Studio only when the run asks for it,
on a terminal or not:

| the run passes | the device |
|---|---|
| `--studio`, or sets `ARORA_STUDIO=1` | connects to Studio; a device that is not enrolled with Studio [enrolls](#enrolling-a-device-with-studio) first |
| nothing, or `--no-studio` | serves the open local bridge, whatever it holds |

`--no-studio` wins over `--studio` and `ARORA_STUDIO`, which is how a run
overrides the variable, and `--studio` wins over `ARORA_STUDIO`, which is a
boolean (`1`, `true`, `yes`, `on`, or their opposites); blank counts as
unset, and any other value fails the start.

The credential is the refresh token of the device's Studio session, in its
device directory's `studio/`. A device holds one once it has enrolled, and a
run passing `--studio` connects with it; deleting `studio/` makes it a device
with none. A saved credential does not connect a run on its own, because it is
not proof of an opt-in: a headless run of arora 13.4 or earlier signed the
device up unasked, and an embedder's `studio::connect` — Vizij's `--studio` —
keeps its credential in the same per-user `default` device unless
`DEVICE_LOCAL_ID` names another. The run says what it decided in one line, a
run that does not ask naming what the device holds:

```text
Studio: connecting, as --studio asks; a device not enrolled with Semio Studio enrolls first.
Studio: no saved credential in <path>; serving the local bridge. Pass --studio to enroll this device with Semio Studio.
Studio: serving the local bridge, as the run does not ask for Studio. Pass --studio to connect with the saved credential in <path>.
```

The robot runners take the same options. An embedder asks for Studio by
calling `studio::connect` (or one of its variants), which connects the device,
enrolling it first when it is not enrolled.

The chain down to the bridge's code, in a `studio-bridge` build:

1. [`device_dir`](src/device_dir.rs) resolves the device directory
   (`device_dir::from_env`): `DEVICE_DIR`; else `<IDENTITY_FILE>_dir` while
   that deprecated variable is set; else
   `device_dir::of(<DEVICE_LOCAL_ID, or default>)`. The per-user directory
   needs a home: a container whose user has no passwd entry sets `DEVICE_DIR`.
   An embedder whose platform gives it a data directory (Android, a Tauri app)
   passes its own to `studio::connect_with_device_dir`.
2. [`studio::credentials`](src/studio/credentials.rs) names `<device dir>/studio`
   and hands it to `DeviceCredentials::in_dir`. The credentials there are the
   client's; arora writes nothing there.
3. A run that does not ask for Studio reads the refresh token there
   (`DeviceCredentials::refresh_token`) only to say in its log whether the
   device holds one, and serves the open local bridge whatever it finds.
4. `studio::connect` passes the `DeviceCredentials` to
   `ZenohDeviceClient::new`, or to `new_endpoint` when `STUDIO_BRIDGE_ENDPOINT`
   names the bridge's endpoint. The client signs the device in with the
   refresh token the credentials hold and saves each rotated one there. When
   they hold no enrolled device's (`Error::NotEnrolled`), arora enrolls the
   device with `studio::enroll`, which saves the new credential there, and
   connects again.

### Enrolling a device with Studio

Studio creates a device, under the owner a person chooses, when the device
enrolls; the device signs in with the credential its enrollment saved from
then on. A run passing `--studio` enrolls the device when its credentials hold
no enrolled device: none, one that does not read (its key missing, the file
damaged, an `IDENTITY_FILE` naming nothing), one Studio refuses (a revoked or
deleted device), or one that is no device's, such as the anonymous account
arora 13 and earlier signed a device up as. It enrolls:

- **with an enrollment token**, when `STUDIO_ENROLLMENT_TOKEN` holds one: a
  person creates it in Studio (`semio_enroll_…`), and Studio creates the device
  under the token's owner. This is how a device with neither screen nor
  operator enrolls. The token is a secret: arora reads it from the
  environment at run time only — no option takes it, since a command line is
  visible to the host's other users — and never logs it.
- **by code** otherwise: the run shows a code, which a person enters in
  Studio, choosing the device's owner. The terminal UI shows it in its prompt
  line; the log says it too, headless included, and a headless run whose log
  shows no info lines — the default without `RUST_LOG` — writes it to
  standard error:

  ```text
  Studio: to enroll this device, enter the code BCDF-GHJK in Semio Studio, or open https://<studio>/approve?code=BCDF-GHJK
  ```

  The approval page is shown when the Studio client knows Studio's web
  address: `STUDIO_URL`, else the one baked into it. A code that expires
  undecided is replaced by another, shown in turn; the run waits for a
  person's decision for as long as it runs.

Then the run connects, and later runs with `--studio` connect without
enrolling. A person refusing the code, or Studio refusing the token, fails the
run, as Studio refused the device it asked for. Only a device whose
credentials hold no enrolled device enrolls: Studio failing otherwise fails the
run and keeps the credential, and so does a credential of another Firebase
project than the run's — the run names its directory, and either the run is
pointed at that project (the `FIREBASE_*` variables) or the directory is
deleted to enroll the device with the run's.

A device can also be enrolled before it runs, with Semio's own client:
`semio device enroll` enrolls it by code or enrollment token and saves the
credential in the same `studio/` of its device directory, which a run with
`--studio` then connects with.

The person approving the device sees its description: its name, and its model
family and hardware and software versions. The name is `DEVICE_NAME`; under
the terminal UI the operator is asked for one when it is unset, and it is
generated (`arora-device-<random>`) when left blank or headless. The model is
the device's (below); under the terminal UI the operator is asked for the
model family when nothing gives one. After enrollment, the device's name and
owner are Studio's: they change there, never from the device.

The device reports its model to Studio each time it connects. Each field comes
from its variable, else from the HAL (`Hal::describe`):

| field | sources, in order |
|---|---|
| model family | `MODEL_FAMILY`, the HAL |
| hardware / software version | `HARDWARE_VERSION` / `SOFTWARE_VERSION`, the HAL |

A report writes the fields it gives, the record keeping the others: a family
the operator typed at enrollment stays, as no report gives another.

Credentials from arora 11.0 and earlier, in `.semio/arora` under the
executable's directory, the home directory or the current directory, move into
`devices/default/studio/` at the device's first start. A deprecated
`IDENTITY_FILE` migrates to `<IDENTITY_FILE>_dir`, which that run uses as its
device directory.

See the [root map](../../readme.md) for where this sits in Arora.

[`Arora`]: src/lib.rs

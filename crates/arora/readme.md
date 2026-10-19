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
embeds the library instead. It reads its configuration from the environment,
serves the open local bridge on `ws://127.0.0.1:9000` in the default build,
and connects to Semio Studio in a build with the `studio-bridge` feature. The
front end is the terminal operator UI when the process is attached to a
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
options the binary parses (the Groot file, `--open`, `--module`); flatten it
into a larger CLI and inject the results through the builder. A runner's
modules, its device directory's and those its command line names, load with
`AroraBuilder::with_module_dirs(&device_dir::from_env()?, &dirs)`, records
included, as the `arora` binary and the robot runners load theirs.

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
            ├── refresh_token
            └── device.json        the saved registration: owners, name, model family
```

The local id tells apart the devices one user runs on one host. It is local
only: the name a device shows (`DEVICE_NAME`) can change without making it
another device, and the id Studio knows it by is assigned at its first sign-in.
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
   client's; arora writes only `device.json` beside them (below).
3. `studio::connect` passes the `DeviceCredentials` to
   `ZenohDeviceClient::new_with_credentials`, or to
   `new_endpoint_with_credentials` when `STUDIO_BRIDGE_ENDPOINT` names the
   bridge's endpoint. The client's Firebase authenticator signs the device in
   with the refresh token the credentials hold and saves each rotated one there.

### What the device registers with Studio as

A `studio-bridge` run registers the device with Studio: its owners (the Studio
user UIDs it belongs to), its name, its model family, and its hardware and
software versions. Each field comes from the first source that gives it:

| field | sources, in order |
|---|---|
| owners | `DEVICE_OWNERS` (comma-separated), `device.json`, the operator |
| name | `DEVICE_NAME`, `device.json`, the operator (generated when left blank) |
| model family | `MODEL_FAMILY`, the HAL (`Hal::describe`), `device.json`, the operator |
| hardware / software version | `HARDWARE_VERSION` / `SOFTWARE_VERSION`, the HAL |
| description | `DEVICE_DESCRIPTION` |

Under the terminal UI the operator is asked only for what nothing else gives,
and a run that asked anything saves the owners and name it resolved — and the
model family the operator typed, if any — in `studio/device.json`. The next
run asks nothing; it logs
`studio: registering as <name> for owner(s) <uids> (saved in <path>; delete it to answer again)`.
Delete the file to be asked again. Leaving the owner empty runs without Studio,
on the open local bridge, and saves nothing. A headless run asks nothing: it
registers what the environment and `device.json` give, completed by the HAL,
so a device set up once under the terminal UI can then run headless.

```json
{
  "owners": ["<owner uid>"],
  "name": "pepper-lab",
  "model_family": "pepper"
}
```

Credentials from arora 11.0 and earlier, in `.semio/arora` under the
executable's directory, the home directory or the current directory, move into
`devices/default/studio/` at the device's first start. A deprecated
`IDENTITY_FILE` migrates to `<IDENTITY_FILE>_dir`, which that run uses as its
device directory.

See the [root map](../../readme.md) for where this sits in Arora.

[`Arora`]: src/lib.rs

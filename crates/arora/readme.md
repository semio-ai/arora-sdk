# arora

The Arora device runtime. One object, [`Arora`], owns a device's state and
advances everything that reads or writes it — the hardware, remote clients,
the behavior, the modules — one step at a time, in a fixed order. The crate
also ships `arora`, a headless device runner built on that object.

## A device

An `Arora` is one **device**. At its center is a **data store**: values under
keys, where a key is a `/`-separated path such as `head/yaw`. Around the store
sit:

- the **HAL** (hardware abstraction layer, [`arora-hal`](../arora-hal)): it
  feeds the hardware's readings into the store and receives what the device
  writes;
- zero or more **bridges** ([`arora-bridge`](../arora-bridge)): the
  endpoints through which remote clients read keys, write the keys the device
  opens to them, and call functions;
- the **engine** ([`arora-engine`](../arora-engine)) and its **modules**:
  typed functions, in WebAssembly, in a native library or in Rust in the same
  process. A call takes in parameters and returns out parameters; the caller
  decides which keys, if any, the outputs are written to;
- one **behavior interpreter** ([`arora-behavior`](../arora-behavior)): it
  runs the device's behavior — a behavior tree by default — against the
  store, calling module functions as it goes.

Each **step** publishes the device's clock, applies the HAL's readings, applies
the bridges' commands, ticks the behavior, then sends what changed to the HAL
and to every bridge that asked for the device's data. The device is the one
owner of its state and applies every writer in its own phase of the step, so
no two writers race: the order of the step decides which write wins.

A remote client writes only the keys the device opens: a key is closed to
remote writers unless its meta in the store opens it. A HAL opens its own
inputs by describing its keys; a host opens others itself.

## Run the device runner

```sh
cargo build                     # builds the runner into target/debug/arora

# serve the open local bridge on ws://127.0.0.1:9000, waiting for a behavior
./target/debug/arora

# install a Groot behavior tree as the device's behavior first
./target/debug/arora crates/arora/examples/hello_tree.groot.xml

# let any client write any key: a sandbox, never a shared network
./target/debug/arora --open

# a second device on this machine: its own name, directory and port
./target/debug/arora --device bench-2 --bridge-port 9001
```

The runner is a device over the in-process fake HAL, which describes no key:
without `--open`, a client reads every key and writes none. On a terminal it
shows the terminal operator UI (feature `tui`), headless otherwise; the log
says when a behavior is loaded or edited and when a task run starts and ends.

The runner loads the modules its device directory carries, then each
`--module` directory (see [modules](#modules)). A Groot tree's leaves name the
native behavior-tree nodes and the functions of those modules. The control
nodes and the string nodes (`SetString`, `UnsetString`, `IsStringSet`,
`WaitStringSet`, `RegexMatch`) are native in
[`arora-behavior-tree`](../arora-behavior-tree): a tree of them runs with no
module loaded.

## Use it as a library

The smallest device is `Arora::builder().build()`: the fake HAL, no bridge, a
private `SimpleDataStore`, and an empty behavior-tree interpreter that idles
until a behavior is loaded. Drive it with `step`, once per frame of your own
clock:

```rust,ignore
use std::time::Duration;

let mut device = arora::Arora::builder().build()?;
let tree = std::fs::read_to_string("crates/arora/examples/hello_tree.groot.xml")?;
device.load_groot(&tree)?;
device.step(Duration::from_millis(10))?; // the tree ticks once
```

Or hand the device your HAL and let it pace itself. `AroraBuilder::run` picks
the operator front end, serves the open local bridge when no bridge was given,
and steps at `Arora::DEFAULT_STEP_PERIOD` (10 ms):

```rust,ignore
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    arora::Arora::builder()
        .with_hal(Box::new(my_hal))
        .run()
        .await
}
```

`arora::run_with_hal(hal)` is the same call; `arora::run` runs the default
device, and `run_with` / `run_with_frontend` take the bridge, the store and the
front end as well. Stopping a run is dropping its future: everything the run
holds lives inside it, so a host can then build and run a fresh device.

Every part has a builder setter: `with_data_store`, `with_hal`, `with_bridge`
(repeatable), `with_behavior_interpreter`, the module setters below,
`with_start_time`, `with_guest_limits`, and, for `run`, `with_step_period`,
`with_frontend`, `with_device` and, in a `studio-bridge` build, `with_studio`.
Between steps, an embedder
reads the store (`Arora::store`), calls a function (`Arora::call`), and
watches the behavior's failures (`Arora::behavior_error`). `Arora::caller`
hands out a `LocalCaller`, the in-process client: it writes inputs, calls and
invokes functions by name, and starts and halts task runs, while `run` owns
the device.

A device-specific build is a binary of its own around this crate: the
[`device` example](examples/device.rs) brings its own `Hal` and hands it to
`run_with`. A binary with a command line flattens `RunnerArgs` (the options
every runner takes) or `DeviceCli` (the `arora` binary's) into its own, as
[`src/main.rs`](src/main.rs) and the robot runners `arora-ros2`
([`arora-hal-ros2`](../arora-hal-ros2)) and `arora-restful`
([`arora-hal-restful`](../arora-hal-restful)) do.

[`docs/runtime-and-data-flow.md`](docs/runtime-and-data-flow.md) walks the
store, the seams, the step and the builder in detail.

## Modules

A device's modules load when it is built, through the builder:

| setter | the module |
|---|---|
| `with_module(header, bytes)` | a guest: its low-level `Header` and its artifact, for the executor the header names (`wasm-component` for a `wasm32-wasip2` component, `native` for a dynamic library) |
| `with_compiled_module(&compiled)` | a guest compiled once (`CompiledModule`), shared by several devices |
| `with_declared_module::<M>(executor, bytes)` | a guest declared in Rust: the header and every export's signature come from the declaration |
| `with_host_module(module)` | in-process Rust functions under a module id (`ModuleBuilder`, `HostModule`) |
| `with_module_dirs(device_dir, dirs)` | every module directory under the device directory's `modules/`, then each of `dirs` |

One module id is one module: a second module under a taken id fails the build,
naming both. The device's **method index** lists every function of its modules
with its signature: what a remote's `DescribeMethods` answers and what a Groot
tree's tags resolve against. A guest's exports are described from its record
when the device's registry holds it (`with_registry`, or the module
directory's `records/`), else from its header, which describes only functions
whose parameters and return are primitives or optionals over a scalar
primitive. A function left undescribed still dispatches.

A built device loads, replaces and unloads guests between steps with
`Arora::load_module`, `reload_module` and `unload_module`. The calls to a
replaced or unloaded module that the device has received and not applied
fail, and the task runs calling it halt. A native library is neither
reloaded nor unloaded: restarting the device changes it.

A **module directory** is the form a guest is published and carried in:

```text
<module directory>/
├── header.json   the module's low-level Header, as JSON
├── <artifact>    the one file with the executor's extension: .wasm, or .so / .dylib / .dll
└── records/      optional: the module's record and those it depends on (arora-registry's layout)
```

The animation module writes one:

```sh
cargo build -p arora-animation-module --target wasm32-wasip2 --release
cargo run -p arora-animation-module --features module-dir --example module_dir -- \
    ./animation target/wasm32-wasip2/release/arora_animation_module.wasm
./target/debug/arora --module ./animation
```

A module the device cannot load — a header it cannot read, no artifact or
several, one module in two directories, an artifact the engine rejects, a
record that does not match its header or misses a record it depends on —
fails the start, naming the module. Entries whose name starts with `.` (OS
metadata) are ignored. [`module_dir`](src/module_dir.rs) reads module
directories for an embedder.

## Where a device keeps its data

A runner's device has a name on its machine, `--device` (`ARORA_DEVICE`),
`default` when unset: one path component, local to the machine. Several
devices on one host are several names, each with its own directory and, to
serve side by side, its own `--bridge-port`. The **device directory** holds
what the device keeps from one run to the next:

```text
<data_local_dir>/semio/arora/devices/<name>/    --device-dir (ARORA_DEVICE_DIR) replaces this path
└── modules/                                    the module directories loaded at start, in name order
```

`<data_local_dir>` is per user: `~/Library/Application Support` on macOS,
`~/.local/share` on Linux, `%LOCALAPPDATA%` on Windows. `RunnerArgs::device`
resolves the options into a `Device` ([`device`](src/device.rs)).

## Semio Studio is opt-in

A build with the `studio-bridge` feature can connect a device to Semio Studio.
The `arora` binary built with it connects unless the run passes
`--no-studio`; a robot runner does the same under its own `studio` feature. A
build without the feature has no Studio code and no Studio option.

```sh
cargo build -p arora --features studio-bridge
./target/debug/arora                  # connects to Studio
./target/debug/arora --no-studio      # serves the open local bridge instead
```

A device not enrolled with Studio enrolls first: with the enrollment token in
`STUDIO_ENROLLMENT_TOKEN`, or else by a code the run shows, which a person
enters in Studio. Its credential is kept under its name in Semio's device
store, `<data_local_dir>/semio/devices/<name>/` (`SEMIO_DEVICE_STORE` replaces
the root), apart from its device directory. A run that connects given
`--device-dir` and no `--device` is refused, since two such devices would share
the credential of `default`.

The library's `run`, `run_with_hal`, `run_with` and an `AroraBuilder` given no
choice serve the open local bridge: a Cargo feature is on for every crate a
build links, so the feature alone does not say that the runner at hand chose
Studio. An embedder connects with `studio::connect` (or a variant) and
`with_bridge`, or hands `studio::Choice::Studio` to `with_studio`. An app that
connects only a device holding a credential, and enrolls it when a person
asks, keeps the device's Studio side instead, the device client's
`StudioDevice`: `studio::of(device)` gives it, or
`studio::in_device_dir(device_dir, name, store)` on a platform that gives the
app its data directory at run time. Getting it contacts nobody, and neither
does its `holds_credential`; its `connect` never enrolls, and
`studio::enroll` enrolls the device as a run does. The
[`studio` module docs](src/studio/mod.rs) cover enrollment, the credential, the
model the device reports and every variable.

## Reference

### Features

| feature | default | what it adds |
|---|---|---|
| `native` | yes | the wasmtime component and native-library executors, the device runner and its helpers (`DeviceCli`, `RunnerArgs`, `Device`, `module_dir`, `operator`), the open local bridge ([`arora-bridge-ws`](../arora-bridge-ws)) |
| `tui` | yes | the terminal operator UI (`tui`); implies `native` |
| `studio-bridge` | no | the Semio Studio connection (`studio`); implies `native` |

For the browser, build with `--no-default-features` for `wasm32-unknown-unknown`:
the engine runs components on the browser's `WebAssembly`, and
[`arora-web`](../arora-web) wraps the device for JavaScript. A native build
without `native` carries no executor, only host modules.

### Command line

| option | variable | default | meaning |
|---|---|---|---|
| `<GROOT>` | | | a Groot behavior tree to install as the device's behavior |
| `--open` | | off | open every key without meta of its own to remote writers (`arora` only) |
| `--module <DIR>` | | | a module directory to load besides the device directory's; repeatable |
| `--device <NAME>` | `ARORA_DEVICE` | `default` | the device's name on this machine |
| `--device-dir <PATH>` | `ARORA_DEVICE_DIR` | per-user directory of the name | the device directory |
| `--bridge-port <PORT>` | `ARORA_BRIDGE_PORT` | `9000` | the open local bridge's port, `ws://127.0.0.1:<PORT>`; 0 is refused |
| `--no-studio` | | | serve the open local bridge instead of connecting (`studio-bridge` builds) |

`RUST_LOG` filters the log: errors only by default headless, `info` under the
terminal UI. In the terminal UI the wheel and PgUp/PgDn scroll the log, `m`
hands the mouse to the terminal to select text and back, and `q` or Ctrl-C
quits.

The deprecated variables `DEVICE_LOCAL_ID` (the name), `DEVICE_DIR` (the
directory) and `IDENTITY_FILE` still name a device, each with a warning; the
[`device` module docs](src/device.rs) say how.

See the [root map](../../readme.md) for where this crate sits in Arora.

[`Arora`]: src/lib.rs

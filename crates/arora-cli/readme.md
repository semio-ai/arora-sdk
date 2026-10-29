# `arora-cli`

A command line that loads Arora modules into an engine and calls one of their
functions: the quickest way to try a module without writing a host for it.
It drives the [Arora engine](../arora-engine/readme.md) directly, with no
device around it — no store, HAL, bridge or behavior. To run a device, use the
`arora` runner of the [`arora`](../arora/readme.md) crate.

The crate is not published on crates.io: build it from this workspace.

## Smallest use

Build the command and the `test-cpp` module — whose build writes its module
directory, `target/debug/modules/test-cpp/` — then call the module's
`test_optional` with `41`:

```sh
cargo build -p arora-cli -p test-cpp
target/debug/arora-cli \
  --header target/debug/modules/test-cpp/header.json \
  --exe target/debug/modules/test-cpp/test-cpp.wasm \
  --call "id: a24fd5ab-baf8-4f44-af41-053a54b3fa81
args:
- id: b3ec8dd2-2df1-43ae-bf2d-0a567c998243
  value:
    u32: 41"
```

It prints the call's result as YAML, here `42` as an optional `u32`.

## How it works

1. **The registry.** The command builds a registry of type records: a local
   one, or a remote one when it is given a registry URL. Each `--include`
   directory adds its records (a module directory's `records/`, or any
   directory in the [`arora-registry`](../arora-registry/readme.md) record
   layout).
2. **The modules.** Each `--header` is a module's header (YAML or JSON); the
   command freezes it against the registry — so every type the module names
   must be there — and loads the matching `--exe` into the engine. Headers and
   executables pair up in order. The engine runs `wasm-component` modules (a
   `wasm32-wasip2` component) and `native` ones (a dynamic library).
3. **The call.** `--call` is an `arora_types::call::Call` in YAML: the
   function's `id`, its `args` (each a parameter `id` and a `value` in the
   `Value` YAML form, `u32: 42`, `str: hi`), and optionally the `module_id`.
   Without `module_id`, the command finds the module that exports the
   function among those it loaded.

Without `--header` and `--call`, the command prints its help.

## A remote registry

Records can come from Semio's hosted record store. Log in with `semio-cli`,
which writes its configuration file (`~/.semio/cli.yaml`), and pass that file:

```sh
target/debug/arora-cli --config ~/.semio/cli.yaml --header … --exe … --call …
```

The command reads the registry's URL from the file, refreshes the session
token, and writes the file back. `--registry-url` names the registry instead
of the file. Prefer the configuration file to `--user-name` and `--password`:
a password given on the command line stays in the shell's history and is
visible in the process list.

## Reference

| Option | Meaning |
|---|---|
| `-H`, `--header <FILE>` | A module's header. Repeatable, paired in order with `--exe`. |
| `-e`, `--exe <FILE>` | A module's executable. Repeatable, paired in order with `--header`. |
| `-c`, `--call <YAML>` | The call to make, as a YAML `Call`. |
| `-i`, `--include <DIR>` | A directory of records to add to the registry. Repeatable. A missing directory fails the command. |
| `-n`, `--repeat <N>` | Make the call `N` times (default 1), printing each result. |
| `-b`, `--benchmark` | Print the total and per-call time of the calls. |
| `--config <FILE>` | A `semio-cli` configuration file: the remote registry's URL and credentials, updated in place. |
| `-r`, `--registry-url <URL>` | The remote registry's URL. With neither this nor `--config`, the registry is local. |
| `-u`, `--user-name <NAME>` | The user to authenticate as against the remote registry. |
| `-p`, `--password <PASSWORD>` | The user's password. See above. |

The workspace's integration tests (`tests/tests/integration.rs`) drive the
command this way against the test modules.

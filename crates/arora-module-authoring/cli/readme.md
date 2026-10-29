# `arora-module-cli`

Generates the bindings that implement a module in a language other than Rust
from its `module.yaml`, and writes the module's header beside them. A C++
module's build runs it on every build
([`modules/test-cpp`](../../../modules/test-cpp/CMakeLists.txt)):

```bash
arora-module-cli generate -l cpp -m module.yaml -o build/arora
```

It reads `module.yaml`, resolves it with
[`arora-module-core`](../core/readme.md), hands the result to the generator
for the language, and writes what the generator returns under the output
directory, then the module's header there as `module.yaml` and
`header.json`.

## Finding the generator

The generator for `-l <language>` is the executable `arora-module-<language>`
in the same directory as `arora-module-cli`:
[`arora-module-cpp`](../cpp/readme.md) or
[`arora-module-rust`](../rust/readme.md). A build script receives both as
artifact dependencies and copies them into one directory
([`modules/test-cpp/build.rs`](../../../modules/test-cpp/build.rs)).

The generator is started with `--self-id <module id>` and
`--self-version <module version>`, then any arguments after the
subcommand's own. It reads the module's assets on its standard input and
writes back one [`arora-vfs`](../../arora-vfs/readme.md) directory, framed as
[`arora-module-core`](../core/readme.md#talking-to-a-generator-process)
describes.

## Arguments

Before the subcommand:

| | |
|---|---|
| `-i`, `--include <dir>` | load the record files under `<dir>` into the registry; repeatable |
| `-r`, `--registry-url <url>` | resolve against Semio's record store at `<url>` instead of a local registry |
| `-c`, `--config <file>` | a `semio-cli` configuration file holding the store's URL and credentials, updated with a refreshed token |
| `-u`, `--user-name`, `-p`, `--password` | the store's credentials, written to the configuration file |

Without `--registry-url` or a configuration naming one, the registry is local
and holds what `--include` loads.

`generate`:

| | |
|---|---|
| `-m`, `--module-file <file>` | the module's `module.yaml` |
| `-l`, `--language <language>` | the generator to run |
| `-o`, `--output-directory <dir>` | where the generated files go |
| `--dry-run` | list the generated files instead of writing them |

The crate depends on `semio-client` from git, so it is not published to
crates.io.

# `arora-module-cli`

Command-line tool for generating language-specific build-files, exporting types, and exporting modules.
`arora-module-cli` interfaces with language-specific executables to generate code.
It processes the [module headers](../arora-schema/readme.md#module) for them and
analyzes them against a [registry](../arora-registry/readme.md)
or local includes using [`arora-module-core`](../arora-module-core/readme.md).
Then it sends the resolved type dependencies and
human-readable exports and imports to consider to the generators

## Common Arguments

  - `--config` / `-c` - Path to a `semio-cli` configuration file to reuse
    and potentially update.
  - `--include` / `-i` - Include records in the registry.
    It should be the path to a directory of records.
  - `--user-name` / `-u` - User name to authenticate with.
    Overrides and updates the configuration file if provided.
  - `--password` / `-p` - Password to authenticate with.
    Updates the configuration file if provided.
  - `--registry-url` - URL of the registry to use.
    Overrides and updates the configuration file if provided.
    Default is `http://localhost:8080`.
  - `--help` - Print available options and subcommands

## Code Generation

`arora-module-cli generate` also accepts these specific parameters:

  - `--module-file` / `-m` - The module file to generate code for.
    It should be the path to a module header file (`.yaml`).
  - `--language` / `-l` - The language to generate files for
    (*e.g.*, `cpp`, `rust`)
  - `--output-directory` / `-o` - The location generated files will be placed

Results depend on the generators.
See [`arora-module-cpp`](../arora-module-cpp/readme.md)
or [`arora-module-rust`](../arora-module-rust/readme.md)
for more details.

### Communication with the Code Generator

`arora-module-cli` finds the generators from
the `--language` option value `<language>`,
by looking for the executable named `arora-module-<language>`.
The generator will be started with the args
`--self-id` and `--self-version`,
corresponding respectively to the UUID of the module to generate,
and to its version tag.

Then, it is fed the list of resolved
[`ModuleAsset`s](../arora-module-core/readme.md)
in the standard input (serialized using [`serde`](https://serde.rs/)).
It contains the description of all the dependent types and modules,
and ends with the description of the module to generate.

In return, the generator produces a serialized virtual directory
using [`arora-vfs`](../arora-vfs/readme.md),
that `arora-module-cli` will write to the location specified
with the option `--output-directory`.

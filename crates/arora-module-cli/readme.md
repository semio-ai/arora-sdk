# `arora-module-cli`

Command-line tool for generating language-specific build-files, exporting types, and exporting modules. `arora-module-cli` interfaces with language-specific executables to generate code. It processes the module headers for them and sends filtered
type dependencies and human-readable exports and imports to consider.

See the corresponding crates (`arora-module-<lang>`)
for details on code generation.

## Common Arguments

  - `--config` / `-c` - Path to a `semio-cli` configuration file to reuse
    and potentially update.
  - `--include` / `-i` - Include entities in the registry.
    It should be the path to a directory of entities.
  - `--user-name` / `-u` - User name to authenticate with.
    Overrides and updates the configuration file if provided.
  - `--password` / `-p` - Password to authenticate with.
    Updates the configuration file if provided.
  - `--registry-url` - URL of the registry to use.
    Overrides and updates the configuration file if provided.
    Default is `http://localhost:8080`.
  - `--help` - Print available options and subcommands

## `arora-module-cli generate`

  - `--language` / `-l` - The language to generate files for (e.g., cpp, rust)
  - `--output-directory` / `-o` - The location generated files will be placed

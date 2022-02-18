# `arora-module-cli`

Command-line tool for generating language-specific build-files, exporting types, and exporting modules. `arora-module-cli` interfaces with language-specific executables to generate code. It processes the module headers for them and sends filtered
type dependencies and human-readable exports and imports to consider.

See the corresponding crates (`arora-module-<lang>`)
for details on code generation.

## Common Arguments

  - `--registry-uri` - Specify an alternate URI for the registry
  - `--help` - Print available options and subcommands

## `arora-module-cli generate`

  - `--language` / `-l` - The language to generate files for (e.g., cpp, rust)
  - `--output-directory` / `-o` - The location generated files will be placed

## `arora-module-cli export-type`

  - `--input-file` / `-i` - The Type input YAML
  - `--no-resolution` / `-n` - Specify this type is already in the low-level format and does not need name resolution
  - `--output-directory` / `-o` - The root of the arora-registry repository
  - `--help` - Print subcommand-specific help

## `arora-module-cli export-module`

  - `--executable-file` / `-e` - The module's binary executable
  - `--configuration-file` / `-c` - The ModuleDefinition input YAML
  - `--help` - Print subcommand-specific help

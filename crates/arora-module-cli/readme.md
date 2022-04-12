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

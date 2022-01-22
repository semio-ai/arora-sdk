# engine-module-cli

Command-line tool for generating language-specific build-files, exporting types, and exporting modules. engine-module-cli interfaces with language-specific executables to generate code; please see those crates for more information.

## Common Arguments

  - `--registry-uri` - Specify an alternate URI for the registry
  - `--help` - Print available options and subcommands

## `engine-module-cli generate`

  - `--language` / `-l` - The language to generate files for (e.g., cpp)
  - `--output-directory` / `-o` - The location generated files will be placed

## `engine-module-cli export-type`

  - `--input-file` / `-i` - The Type input YAML
  - `--no-resolution` / `-n` - Specify this type is already in the low-level format and does not need name resolution
  - `--output-directory` / `-o` - The root of the engine-registry repository
  - `--help` - Print subcommand-specific help

## `engine-module-cli export-module`

  - `--executable-file` / `-e` - The module's binary executable
  - `--configuration-file` / `-c` - The ModuleDefinition input YAML
  - `--help` - Print subcommand-specific help

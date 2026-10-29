# arora-behavior-tree-types-yaml

The record files of the behavior tree types
([`arora-behavior-tree-types`](../arora-behavior-tree-types/readme.md)):
`arora.Status` and `arora.TickId`, for a tool that reads records from files
rather than from Rust, such as the code generator of a `module.yaml` naming
them.

Its build script writes them under `records/` in the crate's directory, in
[`arora-registry`](../arora-registry/readme.md)'s YAML layout:

```text
records/
├── folder/<arora scope id>.yaml
├── enumeration/325a5767-e344-4532-860e-0749bcf2e428@<version>.yaml   arora.Status
└── structure/6f49e650-84ca-4899-a9bd-1f3bf17fab51@<version>.yaml     arora.TickId
```

A tool takes the directory with its `--include` option, as
[`arora-module-cli`](../arora-module-authoring/cli/readme.md) and
[`arora-cli`](../arora-cli/readme.md) do, and
`arora_registry::local_yaml::load_records_from_yaml_dir` loads it into a
registry.

```sh
cargo build -p arora-behavior-tree-types-yaml   # writes records/
cargo test -p arora-behavior-tree-types-yaml    # loads them back
```

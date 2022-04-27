# Behavior Tree Types in YAML

This crate produces YAML files describing
the [behavior tree types](../arora-behavior-tree-types/readme.md).
After the build, the YAML files are available under records/,
in a layout specified for [registries](../arora-registry/readme.md#yaml-records-layout).

In this project, `<project>/crates/arora-behavior-tree-types-yaml`
is often used as an `--include` option to CLI tools such as
[`arora-cli`](../arora-cli/readme.md)
or [`arora-module-cli`](../arora-module-cli/readme.md).

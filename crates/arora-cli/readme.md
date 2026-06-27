# `arora-cli`

A CLI version of the [Arora Engine](../arora-engine/readme.md).
Can be used to load and execute modules
without integrating the [`arora` library](../arora-engine/readme.md)
into another codebase.

## In a Nutshell

`arora-cli` loads modules by parsing their `--header` file,
and loading their associated `--exe`, a WebAssembly binary.
The modules may depend on other modules or types,
which can be provided locally as a folder to `--include`,
or as a remote registry, reachable given the right `--config` file.
See [Semio Client](https://github.com/semio-ai/semio-client)
and [`semio-cli`](https://github.com/semio-ai/semio-cli)
for information about client configuration files.

Then, a function can be called by providing a `--call` description in YAML.
The description format corresponds to the YAML serialization
of the [`Call` structure of the Arora Engine](../arora-engine/src/call.rs),
using the usual [generic `Value` YAML serialization](https://github.com/semio-ai/arora-types/blob/main/src/value.rs).

For instance, a call to `arora-cli` may look like this:
```bash
$ target/debug/arora-cli \
--include crates/arora-behavior-tree-types-yaml/records \
--header build/modules/test-cpp/arora/module.yaml \
--exe build/modules/test-cpp/test-cpp \
--call "id: b213a552-77ad-465a-a26d-352e8eccfd63
args:
- id: 55dbec70-1c3a-433e-a6e6-27446b7f065e
  value:
    u32: 42
- id: abf9ca4e-e03f-431a-a32b-4911f809c399
  value:
    u32: 64"
```

To connect to a remote registry,
you can use [`semio-cli`](https://github.com/semio-ai/semio-cli).
As you login or signup,
a configuration file will be created under `~/.semio/cli.yaml`.
You can then pass it to `arora-cli` with the `--config` option.

Try `arora-cli --help` for more options.

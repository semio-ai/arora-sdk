# `arora-cli`

A CLI version of the [Arora Engine](../arora/readme.md).
Can be used to load and execute modules
without integrating the [`arora` library](../arora/readme.md)
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
of the [`Call` structure of the Arora Engine](../arora/src/call.rs),
using the usual [generic `Value` YAML serialization](../arora-schema/src/value.rs).

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

## Help
```
arora-cli 0.1.0

USAGE:
    arora-cli [OPTIONS]

OPTIONS:
    -b, --benchmark
            Measure time taken to perform the tasks, and print them

    -c, --call <CALL>
            If set, performs a call described in yaml

        --config <CONFIG>
            Path to a semio-cli configuration file to reuse and potentially update.
                If absent and no registry URL is provided, a local registry will be used.

    -e, --exe <EXE>
            Binaries of modules to load. Order must match --header arguments

    -h, --header <HEADER>
            Headers of modules to load. Order must match --exe arguments

        --help
            Print help information

    -i, --include <INCLUDE>
            Include records in the registry.
                It should be the path to a directory of records.

    -n, --repeat <REPEAT>
            Number of times to perform a call. Still performs the call is set to 0. Ignored if
            --call is not set [default: 1]

    -p, --password <PASSWORD>
            Password to authenticate with.
                Updates the configuration file if provided.
                Ignored if no registry URL is provided.

    -r, --registry-url <REGISTRY_URL>
            URL of the registry to use.
                If absent and no configuration file is provided, a local registry will be used.

    -u, --user-name <user-name>
            User name to authenticate with.
                Overrides and updates the configuration file if provided.
                Ignored if no registry URL is provided.

    -V, --version
            Print version information
```
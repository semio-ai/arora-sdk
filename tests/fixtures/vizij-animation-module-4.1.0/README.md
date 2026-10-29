# `@vizij/animation-module` 4.1.0: header and records

`header.json` and `records/` are copied unchanged from the `artifact/`
directory of the npm package `@vizij/animation-module` 4.1.0 (MIT,
[vizij-ai/vizij-rs](https://github.com/vizij-ai/vizij-rs)), the module
directory of the module `@vizij/animation` 2.0.0 without its guest
(`vizij_animation_module.wasm`). The package's tarball has the SHA-256
`fada65302ea99b7f789561d69df80eef2a1bcb3bab2fbaa683a996740d89d2a8`.

The records live in the `vizij` scope. The module `@arora/animation`
(`modules/animation`) keeps their ids, at later versions in the `arora` scope,
and
[`vizij_s_published_records_and_the_module_directory_s_load_into_one_registry`](../../tests/animation.rs)
holds that a device loads both modules' records into one registry, each record
at its own version and scope.

The test reads `records/`, as a device reads a module directory's records, and
parses `header.json`; it loads no guest, so the `wasm` executor the header
names, which no Arora host provides, does not come into play.

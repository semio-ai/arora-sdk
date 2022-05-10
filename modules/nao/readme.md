# NAO Support Module

This module is meant to be compiled into a native binary,
and be loaded with the native executor.
It compiles and uses libQi to perform calls for the robot NAO.

This module exports some behavior tree nodes to demonstrate
the new Arora engine on a NAO v5,
running NAOqi v2.1.4 in a Linux system,
on an i686 processor.

## Cross-compilation on Mac

Install [a cross-toolchain](https://github.com/messense/homebrew-macos-cross-toolchains)
with [`brew`](https://brew.sh/):

```shell
brew tap messense/macos-cross-toolchains
brew install i686-unknown-linux-musl
```

You need the Arora tools built for the host first:

```shell
cargo build
```

Build using CMake and the provided cross-toolchain:

```shell
cmake -B build-nao -DCMAKE_TOOLCHAIN_FILE=mac-homebrew-i686.toolchain.cmake -DARORA_BINARY_DIR=../../target/debug -DARORA_BEHAVIOR_TREE_INCLUDE=../../crates/behavior-tree-types-yaml/records
cmake --build build-nao
```

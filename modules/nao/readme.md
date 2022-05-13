# NAO Support Module

This module is meant to be compiled into a native binary,
and be loaded with the [native executor](../../crates/arora/readme.md#native-executor).
It compiles and uses Semio's patched [libQi](https://github.com/semio-ai/libqi)
to perform calls for the robot NAO.

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

Configure and build the engine project with NAO support:
  
```shell
cmake -B build-nao -DNAO=1
cmake --build build-nao
```

You should find the result binary under
`<repo>/target/i686-unknown-linux-musl/debug/libnao.so`.

> Currently `cmake` automatically configures itself for the target `wasi`,
> then calls `cargo` for the target `i686-unknown-linux-musl`,
> which in turn calls `cmake` for that target from the [`build.rs`](build.rs).
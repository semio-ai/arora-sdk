# Semio Arora

Semio Arora is a C library (written in Rust) and associated tooling for executing behavior trees in a sandboxed environment.

## Prerequisites
  - Rust. You may need to add first the generic WebAssembly target:
    ```bash
    rustup target add wasm32-unknown-unknown
    ```
  - Python

## Build

```bash
./configure.py
cd build
make
```

## Modules

Modules are the building blocks of Semio arora. Each module exports symbols for other modules to use.
# arora-util

C helpers for Arora guest modules written in C or C++, implemented in Rust so
they agree with Rust guests byte for byte.

It exports one function over the C ABI:

```c
int32_t arora_uuid_compare(const uint8_t *a, const uint8_t *b);
```

It orders two 16-byte UUIDs as Rust's `uuid::Uuid` orders them: `-1`, `0` or
`1`. A generated C++ module compares type, field and function ids with it.

The crate builds as a `staticlib` as well as an `rlib`. Its build script
generates the C header `arora/util.h` under its `OUT_DIR`, and its `links` key,
`arora_util`, hands the header's directory to the build script of a crate
depending on it, as `DEP_ARORA_UTIL_INCLUDE`. A C++ guest includes the header
and links the `wasm32-wasip2` static library, beside
[`arora-buffers`](../arora-buffers/readme.md)'s.

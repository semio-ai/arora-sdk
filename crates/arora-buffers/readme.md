# Arora Buffers

The buffers are used for exchanges across the module boundaries.
It can represent all the data structures into an `[u8]`.
It starts with 4 bytes (32 bits) representing the size of the whole buffer.
It is limited to 32 bits to match WASM's current implementation.

Please refer to the [read](src/read.rs) and [write](src/write.rs) functions
for the details of the buffer format.

This library provides functions to [serialize and deserialize buffers
from generic `Value`s](src/serde_uuid.rs)),
as defined in the [`arora-types`](https://github.com/semio-ai/arora-sdk)

This library also provides [an exported function to allocate or free a buffer](src/alloc.rs).

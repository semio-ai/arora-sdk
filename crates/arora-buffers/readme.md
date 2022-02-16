# Arora Buffers

The buffers are used for exchanges across the module boundaries.
It can represent all the data structures into an `[u8]`.
It starts with 4 bytes (32 bits) representing the size of the whole buffer.
It is limited to 32 bits to match WASM's current implementation.

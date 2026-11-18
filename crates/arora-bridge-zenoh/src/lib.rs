//! A device's bridge over Zenoh: [`ZenohBridge`] on the device, [`ZenohClient`]
//! on the client.
//!
//! Requests are Zenoh queries and changes are Zenoh samples, one key
//! expression per key, so a client subscribes with a key expression and Zenoh
//! routes it only what it asked for, through whatever routers, transports and
//! access control the application's sessions are configured with. Values are
//! Arora Buffers, the binary form of a value modules already exchange
//! (`arora-buffers`): they travel as they are, with no text encoding.
//!
//! The bridge and the client are given a [`zenoh::Session`] and a prefix, a key
//! expression naming the device (`robots/nao-1`): the application configures
//! the session (mode, endpoints, scouting, transports through the features of
//! its own `zenoh` dependency), and one session can serve or reach several
//! devices under different prefixes.
//!
//! # Key space
//!
//! | Key expression | What it carries |
//! |---|---|
//! | `PREFIX/rpc` | a queryable: a query's payload is a request, its reply the answer |
//! | `PREFIX/state/PATH` | the key `PATH`'s value: a put when the key is set, a delete when it is unset |
//!
//! The names are those of Semio Studio's Zenoh key space (`rpc`, `state`).
//!
//! ## Requests
//!
//! A query on `PREFIX/rpc` carries one request, `get`, `update`, `call`,
//! `list_keys` or `describe_methods` ([messages](#messages)), and the device
//! replies with its answer. A query that is not a request is answered with an
//! error, and so is one beyond the
//! [`REQUESTS_IN_FLIGHT`](ZenohBridge::REQUESTS_IN_FLIGHT) the device holds.
//!
//! A client subscribes with Zenoh subscribers, then sends `subscribed`: Zenoh delivers a session's declarations
//! ahead of its later queries, so the device answers it knowing the client's
//! subscribers, once it has told its runtime someone wants its changes. A
//! request the client sends after that answer is applied by a device already
//! publishing to it. A query waits for its reply as long as
//! the querying session's timeout allows (Zenoh's `queries_default_timeout`).
//! A session learns of the device's queryable a moment after it connects: a
//! [`ZenohClient`] waits for it before sending a request, where a bare query
//! sent too early reaches no one.
//!
//! ## Values
//!
//! Each step's change is published key by key under `PREFIX/state/`: a set key
//! as a put whose payload is one Arora Buffer holding an option, some value or
//! none when the key is set to no value; an unset key as a delete. A key whose
//! path is not a Zenoh key expression without wildcards (an empty chunk, `*`,
//! `**`, `$`, `?`, `#`) is not published, and the device logs it once: Zenoh
//! delivers a put on a wildcard to every key it covers.
//!
//! A subscriber to `PREFIX/state/joints/**` receives the key `joints` and the
//! keys under it, the subtree a store's prefix covers
//! ([`prefix_covers`](arora_types::data::prefix_covers)); to
//! `PREFIX/state/**`, every key. A subscriber receives changes only: a client
//! reads the current values with a `get`.
//!
//! The device publishes only while a subscriber matches `PREFIX/state/**`
//! (`DataRequested`), so a device nobody watches encodes nothing. Publishing
//! runs on a thread of its own, never on the device's step: when the network
//! is slower than the device, what waits is each key's newest value only, and
//! a step's change is let go, its newest small values copied out, once later
//! changes replaced most of what it held. A slow link skips camera frames
//! instead of falling behind, what waits weighs about the newest values at
//! most, and every key still ends at its last value. Once the bridge is
//! dropped, the batch being published stops before its next put.
//!
//! A [`ZenohClient`] holds what its reader has not read the same way: each
//! key once, with its newest value.
//!
//! ## Messages
//!
//! A request and an answer are each one Arora Buffer holding one value: an
//! array whose first element, a `u8`, is the message's kind. A message that
//! longer than 64 MiB, followed by other bytes, or that would take more than
//! 128 MiB decoded is refused before decoding; so is a key's value.
//!
//! | Kind | Request | Elements after the kind |
//! |---|---|---|
//! | 1 | `get` | the keys' paths (`string[]`) |
//! | 2 | `update` | the paths of the keys set (`string[]`), their values in the same order (an array of options, none setting the key to no value), the paths of the keys unset (`string[]`) |
//! | 3 | `call` | the module id (`uuid` option), the function id (`uuid`), [the arguments](#fields) |
//! | 4 | `list_keys` | a path prefix (`string` option) |
//! | 5 | `describe_methods` | a name prefix (`string` option) |
//! | 6 | `subscribed` | none |
//!
//! | Kind | Answer | Elements after the kind |
//! |---|---|---|
//! | 64 | `ok` | the return value, [the mutated parameters](#fields) |
//! | 65 | `err` | the reason (`string`) |
//!
//! The answers are those of [`arora_bridge::BridgeOp`]: `get` returns an array
//! of optional values in request order, `update` unit or the keys it refused,
//! `call` the function's return value and mutated parameters, and
//! `list_keys` and `describe_methods` their lists on the value plane;
//! `subscribed` returns unit.
//!
//! ### Fields
//!
//! Two elements: the ids (an array of `uuid` values), and the values in the
//! same order (an array of values): a call's arguments by parameter id, or a
//! result's mutated parameters.

mod bridge;
mod client;
mod messages;
mod pending;

pub use bridge::ZenohBridge;
pub use client::{Changes, ZenohClient};

/// The key expression of a device's requests.
fn rpc(prefix: &str) -> String {
    format!("{prefix}/rpc")
}

/// The key expression under which a device's keys are published, with no
/// trailing `/`.
fn state(prefix: &str) -> String {
    format!("{prefix}/state")
}

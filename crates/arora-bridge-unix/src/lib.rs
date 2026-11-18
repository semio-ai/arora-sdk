//! A device's bridge over a Unix socket file, for clients on its machine or at
//! the end of an `ssh` forward of the file: [`UnixBridge`] on the device,
//! [`UnixClient`] on the client.
//!
//! It carries every [`BridgeOp`](arora_bridge::BridgeOp) but the deprecated
//! `ListMethods`, and pushes the keys a connection subscribes to by prefix.
//! The messages are Arora Buffers, the binary form of a value modules already
//! exchange (`arora-buffers`): values travel as they are, with no text
//! encoding and no framing of the transport's own.
//!
//! # Wire format
//!
//! A connection carries messages back to back, each one Arora Buffer: a
//! little-endian `u32` counting the whole buffer, then one value, an array of
//! values whose first element, a `u8`, is the message's kind. A message longer
//! than [`wire::MAX_MESSAGE_LEN`], or one that is not a buffer of the shapes
//! below, closes the connection.
//!
//! A client sends requests; each carries an `id: u32` of the client's choice,
//! which the device's answer echoes. The device answers in the order its
//! answers are ready, not necessarily the order of the requests.
//!
//! | Kind | Request | Elements after the kind |
//! |---|---|---|
//! | 1 | `get` | `id`, the keys' paths (`string[]`) |
//! | 2 | `update` | `id`, [a change](#a-change) |
//! | 3 | `call` | `id`, the module id (`uuid` option), the function id (`uuid`), [the arguments](#fields) |
//! | 4 | `list_keys` | `id`, a path prefix (`string` option) |
//! | 5 | `describe_methods` | `id`, a name prefix (`string` option) |
//! | 6 | `subscribe` | `id`, the key prefixes (`string[]`), optionally the prefixes [pushed newest only](#newest-only) (`string[]`) |
//!
//! | Kind | Message | Elements after the kind |
//! |---|---|---|
//! | 64 | `ok` | `id`, the return value, [the mutated parameters](#fields) |
//! | 65 | `err` | `id`, the reason (`string`) |
//! | 66 | `changed` | [a change](#a-change) |
//!
//! The answers are those of [`arora_bridge::BridgeOp`]: `get` returns an array
//! of optional values in request order, `update` unit or the keys it refused,
//! `call` the function's return value and mutated parameters, and
//! `list_keys` and `describe_methods` their lists on the value plane. A
//! `subscribe` replaces the connection's prefixes and returns unit: from then
//! on, each step that changes a key one of them covers sends one `changed`
//! holding those of its keys. A prefix covers the key itself and the subtree
//! under it, on a `/` boundary, as a store's prefix does
//! ([`prefix_covers`](arora_types::data::prefix_covers)): `joints` covers
//! `joints/head_yaw` but not `joints_extra`. The empty prefix covers every
//! key; a connection starts with none, so a client that only commands is sent
//! nothing.
//!
//! ## A slow client
//!
//! The device queues for each connection the changes it has not written yet.
//! A connection whose queue holds more than
//! [`UnixBridge::BACKLOG_BYTES`] of values is closed: a client missing a
//! change would hold a stale state with no sign of it, so it reconnects and
//! reads again. The answers it has not written have a cap of the same size,
//! and a [`UnixClient`] closes the connection the same way when its
//! [`Changes`] are read too slowly.
//!
//! A client that closes its writing half (as the relay does when its input
//! ends) is still written the answers it waits for; the device then closes
//! the connection.
//!
//! ### Newest only
//!
//! A `subscribe`'s optional second list names prefixes, among the subscribed
//! keys, whose keys are pushed newest value only: a key one of them covers is
//! never queued, and a `changed` carries the newest value it has not sent,
//! beside the queued keys of an older step. A client too slow for a stream of
//! large values (camera frames) then skips values of those keys instead of
//! falling behind on the others. Such a key holds no intermediate value: use it
//! for a state, not for a series every value of which matters.
//!
//! ## A change
//!
//! Three elements: the paths of the keys set (`string[]`), their values in the
//! same order (an array of options, none setting the key to no value), and the
//! paths of the keys unset (`string[]`).
//!
//! ## Fields
//!
//! Two elements: the ids (an array of `uuid`s), and the values in the same
//! order (an array of values) — a call's arguments by parameter id, or a
//! result's mutated parameters.
//!
//! # Reaching a device through ssh
//!
//! OpenSSH forwards a socket file to a socket file: on the client's machine,
//!
//! ```text
//! ssh -N -o StreamLocalBindUnlink=yes -L /tmp/robot.sock:/run/user/1000/arora/device.sock robot
//! ```
//!
//! makes `/tmp/robot.sock` the device's bridge, as the user `ssh` logs in as;
//! `-J` relays through other hosts on the way. An sshd before OpenSSH 6.7
//! forwards no socket file: there, `relay.py`, beside this crate, joins the
//! standard streams of `ssh robot python relay.py SOCKET` to the file, and
//! [`UnixClient::over`] speaks over them.

mod bridge;
mod client;
pub mod wire;

pub use bridge::UnixBridge;
pub use client::{Changes, UnixClient};

/// The stream's framing: each frame is one whole Arora Buffer, its length
/// prefix included, as [`wire`] decodes it.
fn codec() -> tokio_util::codec::LengthDelimitedCodec {
    tokio_util::codec::LengthDelimitedCodec::builder()
        .little_endian()
        .length_field_length(4)
        // The prefix stays at the head of the frame (no skip), and counts
        // itself: it is the length of the whole frame, unadjusted.
        .num_skip(0)
        .length_adjustment(0)
        .max_frame_length(wire::MAX_MESSAGE_LEN)
        .new_codec()
}

#[cfg(test)]
mod tests {
    use super::wire;
    use tokio_util::codec::Decoder;

    /// The codec cuts a stream into whole buffers, prefix included, whatever
    /// pieces the bytes arrive in.
    #[test]
    fn the_codec_frames_whole_buffers() {
        let one = wire::encode_request(&wire::Request::Subscribe {
            id: 1,
            prefixes: vec!["a".into()],
            latest: vec![],
        })
        .unwrap();
        let two = wire::encode_message(&wire::Message::Changed(Default::default()));
        let stream = [&one[..], &two[..]].concat();
        for cut in 0..stream.len() {
            let mut codec = super::codec();
            let mut buffer = bytes::BytesMut::new();
            let mut frames = Vec::new();
            for piece in [&stream[..cut], &stream[cut..]] {
                buffer.extend_from_slice(piece);
                while let Some(frame) = codec.decode(&mut buffer).unwrap() {
                    frames.push(frame.to_vec());
                }
            }
            assert_eq!(frames, [one.to_vec(), two.to_vec()], "cut at {cut}");
        }
    }
}

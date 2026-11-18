# arora-bridge-zenoh

A device's bridge over [Zenoh](https://zenoh.io), in Arora Buffers: an
[`arora_bridge::Bridge`](../arora-bridge/readme.md) on the device side
(`ZenohBridge`) and a client (`ZenohClient`). For clients anywhere a Zenoh
session reaches: the same machine, a network, routers between them.

- **Zenoh's own forms.** Requests are queries on `PREFIX/rpc`; each key's value
  is put on `PREFIX/state/PATH`, and an unset key is a delete there. A client
  subscribes with a key expression, and Zenoh routes it only the keys it
  asked for. The names are those of Semio Studio's Zenoh key space.
- **Every operation.** `get`, `update`, `call`, `list_keys` and
  `describe_methods`, each request and answer one Arora Buffer, many in
  flight at once.
- **Values as they are.** A key's value is one Arora Buffer, the binary form
  modules already exchange, with no text encoding.
- **The application owns the session.** The bridge and the client are given
  a `zenoh::Session` and a prefix naming the device: mode, endpoints,
  scouting, access control and transports (the features of the application's
  own `zenoh` dependency) are the application's. One session serves or
  reaches several devices under different prefixes.
- **Nothing on the step's thread.** The step hands its change to a thread
  that encodes and publishes it. While no subscriber matches the device's
  keys, the device sends it nothing.
- **A slow link or reader skips, never lags.** What waits to be published,
  and what a client has not read, is each key's newest value only: a slow
  link or reader skips camera frames, holds no replaced one, and every key
  still ends at its last value.

The crate documentation describes the key space and the payloads, for a
client in another language.

## Serve a device

```rust,ignore
let session = zenoh::open(zenoh::Config::from_file("zenoh.json5")?).await?;
let bridge = arora_bridge_zenoh::ZenohBridge::new(&session, "robots/nao-1").await?;
arora::Arora::builder()
    .with_hal(hal)
    .with_bridge(Box::new(bridge))
    .run()
    .await?;
```

Dropping the bridge stops serving and publishing, and leaves the session open.

## Drive it

```rust,ignore
use arora_bridge::BridgeOp;
use arora_types::{data::StateChange, value::Value};
use futures::StreamExt;

let (client, mut changes) = arora_bridge_zenoh::ZenohClient::new(&session, "robots/nao-1").await?;
client.subscribe(vec!["HeadYaw".into()]).await?;
client
    .ask(BridgeOp::Update(StateChange::set("HeadYaw/target.position", Value::F64(0.3))))
    .await?;
while let Some(change) = changes.next().await {
    println!("{change:?}");
}
```

A subscription receives changes only; read the current values with a `get`
once subscribed.

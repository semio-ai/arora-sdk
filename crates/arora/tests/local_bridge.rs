//! The open local bridge end to end: a WebSocket client writes a key, the
//! device applies it to its store.

#![cfg(feature = "native")]

use std::time::Duration;

use arora::Arora;
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key};
use arora_types::value::Value;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

/// A client may write any key: the bridge carries the device's whole store,
/// not a registered subset of it.
#[tokio::test]
async fn a_client_write_through_the_local_bridge_reaches_the_store() {
    let bridge = arora::local_ws_bridge()
        .await
        .expect("serve the local bridge");
    let store = SimpleDataStore::new();
    let mut arora = Arora::builder()
        .with_data_store(Box::new(store.clone()))
        .with_bridge(bridge)
        .build()
        .expect("build the device");

    let (mut client, _) = tokio_tungstenite::connect_async("ws://127.0.0.1:9000")
        .await
        .expect("connect to the local bridge");
    client
        .send(Message::Text(
            r#"{"type":"write_values","values":{"command.vx":{"f32":0.3}}}"#.into(),
        ))
        .await
        .expect("send the write");

    let reply = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = client
                .next()
                .await
                .expect("an open socket")
                .expect("a message");
            if let Message::Text(text) = message {
                let reply: serde_json::Value = serde_json::from_str(&text).expect("JSON");
                if reply["type"] == "write_values_resp" {
                    return reply;
                }
            }
        }
    })
    .await
    .expect("the write is answered");
    assert_eq!(reply["success"], true, "the write is accepted: {reply}");

    let key = Key::from("command.vx");
    for _ in 0..100 {
        arora.step(Duration::from_millis(10)).expect("step");
        if store.read(std::slice::from_ref(&key)) == vec![Some(Value::F32(0.3))] {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the written value never reached the store");
}

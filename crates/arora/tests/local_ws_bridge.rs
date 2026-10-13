//! The open local bridge over a running device: a WebSocket client on the
//! socket, an Arora device with a HAL behind it.
//!
//! What these pin is what a client of the local bridge can count on from the
//! device itself — the HAL's inputs listed open and typed and its outputs
//! closed, a write in either target form moving what the HAL senses, and a
//! refused write answered with the device's reason.

#![cfg(feature = "native")]

use std::sync::Arc;
use std::time::Duration;

use arora::bridge_ws::{AroraWSServer, ServerConfig};
use arora::Arora;
use arora_hal::FakeHal;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value as Json};
use tokio_tungstenite::tungstenite::Message;

/// A device over `hal`, serving the local bridge on a free loopback port, and
/// the bridge's address.
async fn device_over(hal: FakeHal) -> (Arora, String) {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let server = Arc::new(AroraWSServer::new(ServerConfig::with_port(port)));
    let bridge = arora::serve_local_ws_bridge(server)
        .await
        .expect("serve the local bridge");
    let device = Arora::builder()
        .with_hal(Box::new(hal))
        .with_bridge(bridge)
        .build()
        .expect("build the device");
    (device, format!("ws://127.0.0.1:{port}"))
}

/// One client on the socket, sending JSON and reading the answers.
struct Client {
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl Client {
    async fn connect(url: &str) -> Self {
        let (socket, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("connect to the local bridge");
        Self { socket }
    }

    async fn send(&mut self, message: Json) {
        self.socket
            .send(Message::Text(message.to_string()))
            .await
            .expect("send a message");
    }

    /// The next message of type `kind`, skipping any other.
    async fn next_of(&mut self, kind: &str) -> Json {
        let wait = async {
            loop {
                match self.socket.next().await {
                    Some(Ok(Message::Text(text))) => {
                        let message: Json = serde_json::from_str(&text).expect("JSON");
                        if message["type"] == kind {
                            return message;
                        }
                    }
                    Some(Ok(_)) => continue,
                    other => panic!("the socket ended waiting for {kind}: {other:?}"),
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .unwrap_or_else(|_| panic!("no {kind} within five seconds"))
    }

    async fn write(&mut self, values: Json) -> Json {
        self.send(json!({"type": "write_values", "values": values}))
            .await;
        self.next_of("write_values_resp").await
    }

    /// The value pushed for `key` once it equals `expected`.
    async fn sees(&mut self, key: &str, expected: Json) {
        loop {
            let pushed = self.next_of("values_changed").await;
            if pushed["values"][key] == expected {
                return;
            }
        }
    }
}

/// Drive `device` at its default cadence until `client` finishes.
async fn with_device<T>(device: &mut Arora, client: impl std::future::Future<Output = T>) -> T {
    tokio::select! {
        result = device.run(Arora::DEFAULT_STEP_PERIOD) => panic!("the device stopped: {result:?}"),
        output = client => output,
    }
}

fn joint() -> FakeHal {
    let hal = FakeHal::new();
    hal.add_joints(["hip_yaw"]);
    hal
}

/// `list_keys` shows the HAL's inputs — both target forms — open and typed,
/// and what it senses closed.
#[tokio::test]
async fn the_hal_s_keys_are_listed_with_their_meta() {
    let (mut device, url) = device_over(joint()).await;
    let keys = with_device(&mut device, async {
        let mut client = Client::connect(&url).await;
        client
            .send(json!({"type": "list_keys", "path": "hip_yaw"}))
            .await;
        client.next_of("list_keys_resp").await
    })
    .await;
    let listed: Vec<(&str, &Json, &Json)> = keys["keys"]
        .as_array()
        .expect("the listed keys")
        .iter()
        .map(|key| {
            (
                key["path"].as_str().expect("a path"),
                &key["__meta"]["editable"],
                &key["__meta"]["ty"],
            )
        })
        .collect();
    assert_eq!(
        listed,
        vec![
            ("hip_yaw.position", &json!(false), &json!("f64")),
            ("hip_yaw.target_position", &json!(true), &json!("f64")),
            ("hip_yaw/target.position", &json!(true), &json!("f64")),
        ]
    );
}

/// A write in either target form is acknowledged and moves the sensed
/// position, which the client is pushed. A write the device refuses is
/// answered `success: false` with the device's reason, and a batch holding one
/// refused key is refused whole: nothing in it reaches the HAL.
#[tokio::test]
async fn a_write_is_acknowledged_in_either_target_form_or_refused_with_a_reason() {
    let (mut device, url) = device_over(joint()).await;
    with_device(&mut device, async {
        let mut client = Client::connect(&url).await;
        client
            .send(json!({"type": "subscribe", "keys": ["hip_yaw.position"]}))
            .await;
        client.next_of("subscribe_resp").await;
        for (values, position) in [
            (json!({"hip_yaw/target.position": {"f64": 0.5}}), 0.5),
            (json!({"hip_yaw.target_position": {"f64": 0.25}}), 0.25),
        ] {
            let answer = client.write(values).await;
            assert_eq!(
                answer,
                json!({"type": "write_values_resp", "success": true})
            );
            client
                .sees("hip_yaw.position", json!({"f64": position}))
                .await;
        }

        for (values, why) in [
            (
                json!({"hip_yaw.target_position": {"i64": 1}}),
                "declared type",
            ),
            (
                json!({"hip_yaw/target.position": {"i64": 1}}),
                "declared type",
            ),
            (json!({"hip_yaw.position": {"f64": 1.0}}), "not an input"),
            (
                json!({
                    "hip_yaw.target_position": {"f64": 1.0},
                    "hip_yaw.position": {"f64": 1.0},
                }),
                "not an input",
            ),
        ] {
            let answer = client.write(values.clone()).await;
            assert_eq!(answer["success"], json!(false), "{values}: {answer}");
            let message = answer["message"].as_str().unwrap_or_default();
            assert!(message.contains(why), "{values}: {answer}");
        }

        // The read is answered in a later step than the refused batch, and each
        // step takes in what the HAL sensed before it answers: a target that
        // slipped through would be read back here.
        client
            .send(json!({"type": "read_values", "keys": ["hip_yaw.position"]}))
            .await;
        let read = client.next_of("read_values_resp").await;
        assert_eq!(read["values"]["hip_yaw.position"], json!({"f64": 0.25}));
    })
    .await;
}

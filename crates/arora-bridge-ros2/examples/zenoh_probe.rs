//! Interop probe for the ROS 2-over-Zenoh backend.
//!
//! Stands up a `Ros2Bridge` (this crate, now built on the ros2-client Zenoh
//! backend) with a couple of input keys (subscribed topics) and two output keys
//! it publishes on a timer, then logs anything it receives inbound. Run it
//! against a real ROS 2 stack using `rmw_zenoh_cpp` + a `zenohd` router to check
//! that a device's keys interoperate with the standard `ros2` CLI over Zenoh.
//!
//! The Zenoh `Context` reads its session config from the environment
//! (`ZENOH_CONFIG_OVERRIDE` / `ZENOH_SESSION_CONFIG_URI`), so point it at the
//! router with e.g.
//!   ZENOH_CONFIG_OVERRIDE='mode="client";connect/endpoints=["tcp/localhost:7447"]'
//! and keep `ROS_DOMAIN_ID` aligned with the ROS 2 side.

use std::time::Duration;

use arora_bridge::{Bridge, BridgeOp, Inbound};
use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig, Type, Value};
use arora_types::call::CallResult;
use arora_types::data::{Key, StateChange};
use futures::StreamExt;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let namespace = std::env::var("ROS_NS").unwrap_or_else(|_| "vizij".to_string());
    let domain_id: u16 = std::env::var("ROS_DOMAIN_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Input keys become subscribed topics (inbound commands).
    let config = Ros2BridgeConfig::new(namespace.clone(), domain_id)
        .with_input("face/mouth/open", Type::F64)
        .with_input("enabled", Type::Boolean);

    let mut bridge = Ros2Bridge::new(config).await;
    let mut inbound = bridge.take_inbound();

    println!("probe: Ros2Bridge up over ZENOH  (namespace=/{namespace}, domain_id={domain_id})");
    println!(
        "probe: SUBSCRIBED (inputs):  /{namespace}/keys/face/mouth/open  /{namespace}/keys/enabled"
    );
    println!("probe: PUBLISHING (outputs): /{namespace}/keys/battery/level (Float64)  /{namespace}/keys/status (String)");
    println!(
        "probe: ---- ready; drive me with the ros2 CLI (RMW_IMPLEMENTATION=rmw_zenoh_cpp) ----"
    );

    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    let mut tick: f64 = 0.0;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                tick += 0.5;
                let level = 0.5 + 0.5 * tick.sin();
                publish(&mut bridge, "battery/level", Value::F64(level));
                publish(&mut bridge, "status", Value::String(format!("alive t={tick:.1}s")));
            }
            maybe = inbound.next() => match maybe {
                Some(Inbound::Command(cmd)) => {
                    if let BridgeOp::Update(change) = &cmd.op {
                        println!("probe: <= INBOUND update from ROS: {:?}", change.set);
                    }
                    cmd.reply(Ok(CallResult { ret: Value::Unit, mutated: Vec::new() }));
                }
                // DataRequested / DeviceInfo signals — not relevant to this probe.
                Some(_) => {}
                None => {
                    println!("probe: inbound stream ended; exiting");
                    break;
                }
            }
        }
    }
}

/// Fan one changed key out to its ROS 2 topic.
fn publish(bridge: &mut Ros2Bridge, path: &str, value: Value) {
    let mut change = StateChange::new();
    change.set.insert(Key::from(path.to_string()), Some(value));
    bridge.try_send(&change);
}

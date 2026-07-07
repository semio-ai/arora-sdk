# arora-bridge-ros2

ROS 2 as an Arora bridge: exposes a device's keys over ROS 2 topics and accepts
commands from the graph, implementing [`arora_bridge::Bridge`].

An Arora device is one blackboard with four seams around it (store, HAL, bridge,
behavior). This crate is a **bridge** whose remote is a ROS 2 graph — ROS is the
external control/data plane into an Arora runtime. It is a sibling to
`arora-bridge-ws` (whose remote is a local WebSocket app) and is **distinct from
`arora-hal-ros2`**: there ROS is the device's own hardware (a HAL); here ROS is
the remote (a bridge).

## Topic convention

Keys map to topics under a namespace: the key `face/mouth` is the topic
`/{namespace}/keys/face/mouth`. Values speak the Arora data vocabulary
(`arora_types::Value`).

| Direction | Bridge method | ROS 2 |
| --- | --- | --- |
| Runtime → ROS | `send_data(StateChange)` | Publishes each changed key to its topic. The `std_msgs` type is chosen from the value's type (`Value::F64` → `std_msgs/Float64`, …); non-scalar values fall back to a JSON-encoded `std_msgs/String`. |
| ROS → Runtime | `commands()` | Each message received on a declared input topic becomes a `BridgeOp::Update` the runtime applies to its store. |

Input keys must be declared in the config (path + value type): a ROS 2 topic is
typed, and the subscription is created before any message arrives. Output keys
need no declaration — a publisher is created from each changed value's type on
first use.

## Configuration

```rust,no_run
use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig, Type};

# async fn example() {
let config = Ros2BridgeConfig::new("robot", 0)
    .with_input("face/mouth/open", Type::F64)
    .with_input("enabled", Type::Boolean);
let bridge = Ros2Bridge::new(config).await;
// Hand `bridge` to the Arora runtime as its `Bridge`.
# let _ = bridge;
# }
```

`Ros2Bridge::new` spins a DDS node in a background task; call it from within a
Tokio runtime. Dropping the bridge stops the task.

## Bridge-method mapping

- `send_data(StateChange)` → publish each changed key to its topic.
- `commands()` → input-topic messages become `BridgeOp::Update`.
- `data_requested()` → yields `true` once (a ROS 2 graph is a data consumer; DDS
  exposes no clean per-subscriber claim/release toggle).
- `get_device_info` / `device_info_updated` / `update_device_info` → stubs; ROS 2
  has no device-registration concept.

Method invocation (`BridgeOp::Call`) and introspection (`ListKeys` /
`ListMethods`) are not wired yet — see the design notes in the crate docs.

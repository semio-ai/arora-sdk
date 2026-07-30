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

## Methods as services

Where the topic plane mirrors a device's *keys*, the method plane mirrors its
*methods*. The bridge asks the runtime for every method's signature
(`BridgeOp::DescribeMethods`) and, for each one whose parameter and return types
are representable in ROS 2, stands up a service at `/{namespace}/methods/{name}`.
Discovery is automatic — a device's methods are its own surface, so nothing is
declared.

A method maps onto a `.srv` the way ROS already models one: the parameter list is
the **request** (one field per parameter), the return value is the **response**.
Both messages are synthesised as runtime types and driven through the shared CDR
codec (`arora_msgs_ros2::cdr`) against the registry — real ROS 2 message types on
the wire, no ad-hoc encoding. Signatures ROS 2 cannot represent are skipped and
reported, not silently dropped.

## Task runs as actions

Task-run methods — those whose return type is the behavior-tree `Status`
enumeration, i.e. tickable, long-running, cancellable work — are mirrored as ROS
2 **actions** instead. That enum is also the one return type the service plane
cannot carry, so actions claim exactly the methods services skip; the two planes
never overlap. A goal spawns the run (through `BridgeOp::Call` to the
interpreter's `SPAWN`), feedback and result are typed from what the run writes,
and cancel/status ride ros2-client's `RawActionServer`. The goal lifecycle lives
in a `GoalBook`.

Introspection over the bridge (`ListKeys` / `ListMethods`) and `BridgeOp::Call`
over topics remain unwired — services and actions are the method surface.

## ROS4HRI

This bridge is where a device meets a ROS 2 graph, so it is the natural home for
[ROS4HRI](https://ros.org/reps/rep-0155.html) interop. The ROS4HRI message vocabulary
(`hri_msgs/Expression`, `FacialActionUnits`, `Gaze`, …) is available as typed ROS
2 messages in [`arora-msgs-ros2`](../arora-msgs-ros2/README.md), and the Vizij
face standard consumes it (see [vizij-rs's ROS4HRI
docs](https://github.com/vizij-ai/vizij-rs/blob/main/docs/ros4hri.md)).

Today the **key** topic plane carries values as `std_msgs` scalars (non-scalars
as JSON `std_msgs/String`); publishing/subscribing device keys as **typed
`hri_msgs` topics** under a ROS4HRI naming profile (glob include + prefix
rewrite, a `ros4hri` preset) is planned, not yet implemented. Until then a
ROS4HRI face is driven by writing the `standard/ros4hri/*` keys directly.

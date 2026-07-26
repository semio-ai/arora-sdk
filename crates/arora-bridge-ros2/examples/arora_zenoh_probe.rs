//! Manual real-ROS-2 check of the runtime-type CDR codec over Zenoh.
//!
//! Drives `arora_bridge_ros2::cdr` (which encodes/decodes an arora `Value`
//! against a runtime `ty::low::Type`) over a real `rmw_zenoh` wire, using the
//! fork's `publish_raw`/`take_raw`. `geometry_msgs/PointStamped` is described at
//! runtime here — it is NOT a compiled Rust struct.
//!
//! Runs against a Zenoh router (as `rmw_zenoh` uses); point it at one with
//! `ZENOH_CONFIG_OVERRIDE`:
//!
//! ```console
//! # Case 1 (inbound): decode what a native `ros2 topic pub` sends.
//! ZENOH_CONFIG_OVERRIDE='mode="client";connect/endpoints=["tcp/127.0.0.1:7447"]' \
//!   cargo run -p arora-bridge-ros2 --no-default-features --features zenoh \
//!   --example arora_zenoh_probe -- sub
//!
//! # Case 2 (outbound): publish an arora Value; a native `ros2 topic echo` reads it.
//! ZENOH_CONFIG_OVERRIDE='mode="client";connect/endpoints=["tcp/127.0.0.1:7447"]' \
//!   cargo run ... --example arora_zenoh_probe -- pub
//! ```

use std::time::Duration;

use arora_bridge_ros2::cdr;
use arora_types::module::low::TypeRef;
use arora_types::ty::{self, low, TypeRegistry};
use arora_types::value::{Structure, StructureField, Value};
use arora_types::Uuid;
use ros2_client::{Context, MessageTypeName, Name, NodeName, NodeOptions, QosProfile};

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn scalar(name: &str, type_id: Uuid) -> low::StructureField {
    low::StructureField {
        name: name.to_string(),
        type_ref: TypeRef::Scalar { id: type_id },
    }
}

fn structure(type_id: Uuid, fields: Vec<(Uuid, low::StructureField)>) -> low::Type {
    low::Type {
        name: String::new(),
        id: type_id,
        description: String::new(),
        kind: low::TypeKind::Structure(low::Structure::from_fields(fields)),
    }
}

// The PointStamped schema, described at runtime (no compiled Rust struct):
//   builtin_interfaces/Time { int32 sec; uint32 nanosec }
//   std_msgs/Header        { Time stamp; string frame_id }
//   geometry_msgs/Point    { float64 x, y, z }
//   geometry_msgs/PointStamped { Header header; Point point }
fn point_stamped_types() -> (low::Type, TypeRegistry) {
    let time = structure(
        id(0x11),
        vec![
            (id(0x111), scalar("sec", *ty::I32_ID)),
            (id(0x112), scalar("nanosec", *ty::U32_ID)),
        ],
    );
    let header = structure(
        id(0x22),
        vec![
            (id(0x221), scalar("stamp", id(0x11))),
            (id(0x222), scalar("frame_id", *ty::STRING_ID)),
        ],
    );
    let point = structure(
        id(0x33),
        vec![
            (id(0x331), scalar("x", *ty::F64_ID)),
            (id(0x332), scalar("y", *ty::F64_ID)),
            (id(0x333), scalar("z", *ty::F64_ID)),
        ],
    );
    let point_stamped = structure(
        id(0x44),
        vec![
            (id(0x441), scalar("header", id(0x22))),
            (id(0x442), scalar("point", id(0x33))),
        ],
    );
    let mut registry = TypeRegistry::new();
    for t in [time, header, point, point_stamped.clone()] {
        registry.insert(t.id, t);
    }
    (point_stamped, registry)
}

fn sf(field_id: u128, value: Value) -> StructureField {
    StructureField {
        id: id(field_id),
        value: Box::new(value),
    }
}
fn st(type_id: u128, fields: Vec<StructureField>) -> Value {
    Value::Structure(Structure {
        id: id(type_id),
        fields,
    })
}

// A concrete PointStamped Value: sec=42, nanosec=7, frame_id, point=(1,2,3).
fn point_stamped_value(frame: &str) -> Value {
    st(
        0x44,
        vec![
            sf(
                0x441,
                st(
                    0x22,
                    vec![
                        sf(
                            0x221,
                            st(
                                0x11,
                                vec![sf(0x111, Value::I32(42)), sf(0x112, Value::U32(7))],
                            ),
                        ),
                        sf(0x222, Value::String(frame.to_string())),
                    ],
                ),
            ),
            sf(
                0x442,
                st(
                    0x33,
                    vec![
                        sf(0x331, Value::F64(1.0)),
                        sf(0x332, Value::F64(2.0)),
                        sf(0x333, Value::F64(3.0)),
                    ],
                ),
            ),
        ],
    )
}

// Zenoh's runtime requires a multi-thread tokio scheduler.
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();

    let context = Context::new().expect("open zenoh context (set ZENOH_CONFIG_OVERRIDE)");
    let node = context.new_node(
        NodeName::new("/", "arora_zenoh_probe").unwrap(),
        NodeOptions::new(),
    );
    let topic = node.create_topic(
        &Name::new("/", "point").unwrap(),
        MessageTypeName::new("geometry_msgs", "PointStamped"),
        &QosProfile::default(),
    );
    let (ty, registry) = point_stamped_types();

    match mode.as_str() {
        "sub" => {
            let sub = node
                .create_subscription::<()>(&topic, None)
                .expect("create subscription");
            eprintln!("[arora] subscribed /point (geometry_msgs/PointStamped); waiting for a native publisher...");
            loop {
                let (bytes, info) = sub.take_raw().await.expect("take_raw");
                match cdr::decode(&ty, &registry, &bytes) {
                    Ok(value) => println!(
                        "[arora] DECODED /point (seq {}, {} bytes): {value:?}",
                        info.sequence_number(),
                        bytes.len()
                    ),
                    Err(e) => println!("[arora] decode error: {e} ({} bytes)", bytes.len()),
                }
            }
        }
        "pub" => {
            let publisher = node
                .create_publisher::<()>(&topic, None)
                .expect("create publisher");
            let bytes = cdr::encode(&ty, &registry, &point_stamped_value("arora_frame"))
                .expect("cdr encode");
            eprintln!(
                "[arora] publishing /point (geometry_msgs/PointStamped, {} bytes) every 500ms; run `ros2 topic echo /point geometry_msgs/msg/PointStamped`",
                bytes.len()
            );
            loop {
                publisher.publish_raw(&bytes).await.expect("publish_raw");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        other => {
            eprintln!("usage: arora_zenoh_probe <sub|pub>  (got {other:?})");
            std::process::exit(2);
        }
    }
}

//! ROS 2 CDR-over-Zenoh interop for the runtime-type codec
//! ([`arora_bridge_ros2::cdr`]).
//!
//! Proves the codec drives a real `rmw_zenoh` wire end-to-end, in-process and
//! self-contained (no stock ROS 2, no external router): two peer-mode
//! `ros2_client` nodes connect over fixed loopback endpoints with discovery
//! multicast off, and a `geometry_msgs/PointStamped` — described at *runtime* as
//! an arora value type, never a compiled Rust struct — makes the round trip:
//!
//! ```text
//! arora Value --cdr::encode--> CDR bytes --publish_raw-->
//!     zenoh wire --take_raw--> CDR bytes --cdr::decode--> arora Value
//! ```
//!
//! This is the automatic form of both validation cases:
//!   * decoding a structured type not known a priori — the subscriber turns
//!     opaque CDR into a `Value` using only the runtime `low::Type`; and
//!   * publishing a value that originates in the Arora realm — the publisher
//!     serialises an arora `Value` onto the wire.
//!
//! Interop against *stock* `rmw_zenoh` (`ros2 topic echo`/`pub`) additionally
//! needs the message's REP-2016 RIHS01 type hash in the Zenoh key; a
//! `ros2_client`↔`ros2_client` pair uses the wildcard subscriber key, so that
//! is not exercised here. That path is covered by the live probe
//! (`examples/arora_zenoh_probe.rs`) and the fork's `type_description` hash
//! tests.

#![cfg(feature = "zenoh")]

use std::time::{Duration, Instant};

use arora_bridge_ros2::cdr;
use arora_types::module::low::TypeRef;
use arora_types::ty::{self, low, TypeRegistry};
use arora_types::value::{Structure, StructureField, Value};
use arora_types::Uuid;
use ros2_client::{
    Context, ContextOptions, MessageTypeName, Name, NodeName, NodeOptions, QosProfile,
};
use zenoh::Config;

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

// geometry_msgs/PointStamped, described at runtime (no compiled Rust struct):
//   builtin_interfaces/Time { int32 sec; uint32 nanosec }
//   std_msgs/Header          { Time stamp; string frame_id }
//   geometry_msgs/Point      { float64 x, y, z }
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
                                vec![sf(0x111, Value::I32(7)), sf(0x112, Value::U32(250_000_000))],
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
                        sf(0x331, Value::F64(1.5)),
                        sf(0x332, Value::F64(-2.5)),
                        sf(0x333, Value::F64(3.75)),
                    ],
                ),
            ),
        ],
    )
}

// Peer config on IPv4 loopback, discovery multicast off, endpoints pinned so the
// two in-process peers connect directly — no router, no auto-discovery (mirrors
// the fork's `zenoh_backend::pubsub` peer tests).
fn peer_config(listen_port: u16, connect_port: Option<u16>) -> Config {
    let mut c = Config::default();
    c.insert_json5("mode", "\"peer\"").unwrap();
    c.insert_json5("scouting/multicast/enabled", "false")
        .unwrap();
    c.insert_json5(
        "listen/endpoints",
        &format!("[\"tcp/127.0.0.1:{listen_port}\"]"),
    )
    .unwrap();
    if let Some(p) = connect_port {
        c.insert_json5("connect/endpoints", &format!("[\"tcp/127.0.0.1:{p}\"]"))
            .unwrap();
    }
    c
}

/// Full round trip of a runtime-typed `PointStamped` over a real Zenoh wire.
/// Zenoh's async API requires a multi-thread tokio runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn point_stamped_round_trips_over_zenoh() {
    // Distinct fixed ports, clear of the fork's peer tests (17513–17516).
    let sub_port = 17517;
    let pub_port = 17518;

    let sub_ctx =
        Context::with_options(ContextOptions::new().zenoh_config(peer_config(sub_port, None)))
            .expect("open subscriber context");
    let pub_ctx = Context::with_options(
        ContextOptions::new().zenoh_config(peer_config(pub_port, Some(sub_port))),
    )
    .expect("open publisher context");

    let sub_node = sub_ctx.new_node(
        NodeName::new("/", "arora_cdr_sub").unwrap(),
        NodeOptions::new(),
    );
    let pub_node = pub_ctx.new_node(
        NodeName::new("/", "arora_cdr_pub").unwrap(),
        NodeOptions::new(),
    );

    let make_topic = |n: &ros2_client::Node| {
        n.create_topic(
            &Name::new("/", "point").unwrap(),
            MessageTypeName::new("geometry_msgs", "PointStamped"),
            &QosProfile::default(),
        )
    };
    // `M` is unused by the raw path; a unit placeholder is enough.
    let sub = sub_node
        .create_subscription::<()>(&make_topic(&sub_node), None)
        .expect("create subscription");
    let publisher = pub_node
        .create_publisher::<()>(&make_topic(&pub_node), None)
        .expect("create publisher");

    let (ty, registry) = point_stamped_types();
    let value = point_stamped_value("arora_frame");
    // Outbound direction: an arora Value serialised onto the wire.
    let bytes = cdr::encode(&ty, &registry, &value).expect("cdr encode");

    // Publish until the peers connect and a sample arrives (discovery is not
    // instantaneous), within a deadline.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        publisher.publish_raw(&bytes).await.expect("publish_raw");
        if let Ok(Ok((wire, info))) =
            tokio::time::timeout(Duration::from_millis(200), sub.take_raw()).await
        {
            // Inbound direction: decode a structured type not known a priori,
            // using only the runtime `low::Type`.
            let decoded = cdr::decode(&ty, &registry, &wire).expect("cdr decode");
            assert_eq!(decoded, value, "runtime-typed Value must survive the wire");
            assert!(info.sequence_number() >= 1);
            return;
        }
        assert!(Instant::now() < deadline, "no message within timeout");
    }
}

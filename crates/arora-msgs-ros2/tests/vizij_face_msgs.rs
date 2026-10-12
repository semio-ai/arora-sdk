//! `vizij_face_msgs`, Vizij's face package: the content, observation and timing
//! layers and the skills built on them are registered, ride CDR, and hash.

use arora_msgs_ros2::{builtin_interfaces, std_msgs, std_skills, vizij_face_msgs};
use arora_types::value_serde::bridge::to_value_seeded;
use arora_types::AroraType;

/// Every message of the package, under the name form a profile uses.
const MESSAGES: &[&str] = &[
    // content
    "vizij_face_msgs/FacialActionUnits",
    "vizij_face_msgs/Expression",
    "vizij_face_msgs/Viseme",
    // observation
    "vizij_face_msgs/PerceivedFacialActionUnits",
    "vizij_face_msgs/PerceivedExpression",
    "vizij_face_msgs/PerceivedViseme",
    // timing
    "vizij_face_msgs/Timing",
    "vizij_face_msgs/TimedViseme",
    "vizij_face_msgs/VisemeTimeline",
    // skills
    "vizij_face_msgs/SetFacialActionUnits",
    "vizij_face_msgs/SetViseme",
    "vizij_face_msgs/PlayVisemes_Goal",
    "vizij_face_msgs/PlayVisemes_Result",
    "vizij_face_msgs/PlayVisemes_Feedback",
];

#[test]
fn the_package_registers_every_message() {
    let registry = arora_msgs_ros2::registry();
    for name in MESSAGES {
        assert!(registry.get_by_name(name).is_some(), "missing {name}");
    }
    // The qualified forms resolve to the same types.
    assert!(registry
        .get_by_name("vizij_face_msgs/msg/FacialActionUnits")
        .is_some());
    assert!(registry
        .get_by_name("vizij_face_msgs/action/PlayVisemes_Goal")
        .is_some());
    // The 15-shape constants ride on the generated struct.
    assert_eq!(vizij_face_msgs::Viseme::SIL, 0);
    assert_eq!(vizij_face_msgs::Viseme::AA, 10);
    assert_eq!(vizij_face_msgs::Viseme::OU, 14);
}

/// Encode a generated message to CDR and decode it back against the
/// registry type, as a bridge receiving it from a ROS node does.
fn round_trips<M: AroraType + serde::Serialize>(name: &str, message: &M) {
    let registry = arora_msgs_ros2::registry();
    let message_type = registry
        .get_by_name(name)
        .unwrap_or_else(|| panic!("{name} is registered"))
        .clone();
    let (ty, reg) = M::arora_type_with_registry();
    let value = to_value_seeded(message, &ty, &reg).expect("message to value");
    let bytes =
        arora_msgs_ros2::cdr::encode(&message_type, registry.types(), &value).expect("encode");
    let back =
        arora_msgs_ros2::cdr::decode(&message_type, registry.types(), &bytes).expect("decode");
    assert_eq!(back, value, "{name}");
}

fn meta() -> std_skills::Meta {
    std_skills::Meta {
        caller: "test".into(),
        priority: std_skills::Meta::NORMAL_PRIORITY,
    }
}

#[test]
fn the_skill_messages_round_trip_through_cdr() {
    round_trips(
        "vizij_face_msgs/SetFacialActionUnits",
        &vizij_face_msgs::SetFacialActionUnits {
            meta: meta(),
            facial_action_units: vizij_face_msgs::FacialActionUnits {
                codes: vec![1, 6, 12],
                intensities: vec![0.25, 0.5, 1.0],
            },
        },
    );
    round_trips(
        "vizij_face_msgs/SetViseme",
        &vizij_face_msgs::SetViseme {
            meta: meta(),
            viseme: vizij_face_msgs::Viseme {
                value: vizij_face_msgs::Viseme::OH,
            },
        },
    );
    let timed = |value, start_ns: u32, duration_ns: u32| vizij_face_msgs::TimedViseme {
        viseme: vizij_face_msgs::Viseme { value },
        timing: vizij_face_msgs::Timing {
            start: builtin_interfaces::Duration {
                sec: 0,
                nanosec: start_ns,
            },
            duration: builtin_interfaces::Duration {
                sec: 0,
                nanosec: duration_ns,
            },
        },
    };
    round_trips(
        "vizij_face_msgs/PlayVisemes_Goal",
        &vizij_face_msgs::PlayVisemes_Goal {
            meta: meta(),
            timeline: vizij_face_msgs::VisemeTimeline {
                header: std_msgs::Header {
                    stamp: builtin_interfaces::Time { sec: 1, nanosec: 2 },
                    frame_id: String::new(),
                },
                visemes: vec![
                    timed(vizij_face_msgs::Viseme::PP, 0, 100_000_000),
                    timed(vizij_face_msgs::Viseme::AA, 100_000_000, 250_000_000),
                ],
            },
        },
    );
    round_trips(
        "vizij_face_msgs/PlayVisemes_Feedback",
        &vizij_face_msgs::PlayVisemes_Feedback {
            viseme: vizij_face_msgs::Viseme {
                value: vizij_face_msgs::Viseme::AA,
            },
        },
    );
    round_trips(
        "vizij_face_msgs/PerceivedFacialActionUnits",
        &vizij_face_msgs::PerceivedFacialActionUnits {
            header: std_msgs::Header::default(),
            facial_action_units: vizij_face_msgs::FacialActionUnits {
                codes: vec![12],
                intensities: vec![0.8],
            },
            confidences: vec![0.9],
        },
    );
}

/// Every message hashes, so a bridge keys its endpoints on what a native
/// `rmw_zenoh` node built from these files announces. No ROS install ships
/// this package, so the values are checked for form, not against an
/// authority; the hasher's agreement with `rosidl` is pinned on the standard
/// packages (`rep2016_hashes.rs`).
#[cfg(feature = "interop")]
#[test]
fn every_message_and_the_action_hash() {
    let registry = arora_msgs_ros2::registry();
    for name in MESSAGES {
        let ty = registry.get_by_name(name).unwrap();
        let hash = arora_msgs_ros2::rihs01(ty, registry.types())
            .unwrap_or_else(|e| panic!("{name} does not hash: {e}"));
        assert!(
            hash.starts_with("RIHS01_") && hash.len() == 71,
            "{name}: {hash}"
        );
    }
    let get = |suffix: &str| {
        registry
            .get_by_name(&format!("vizij_face_msgs/PlayVisemes_{suffix}"))
            .unwrap()
    };
    let hashes = arora_msgs_ros2::action_hashes(
        "vizij_face_msgs/action/PlayVisemes",
        get("Goal"),
        get("Result"),
        get("Feedback"),
        registry.types(),
    )
    .expect("PlayVisemes hashes");
    for hash in [
        &hashes.send_goal,
        &hashes.get_result,
        &hashes.feedback_message,
    ] {
        assert!(hash.starts_with("RIHS01_") && hash.len() == 71, "{hash}");
    }
}

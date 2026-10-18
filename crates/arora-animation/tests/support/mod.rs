//! What the integration tests share: Studio's stored v2 animation, read into
//! clip format 2.0.

use arora_animation::clip::{Clip, Keypoint, Pairing, Track, Transition, Transitions};
use arora_animation::time::nanos_from_millis;
use arora_animation::Value;
use serde_json::Value as Json;

/// A stored Studio animation of format 2 (stamps and handle times in
/// milliseconds) as a clip, with the ids of the tracks Studio mutes.
pub fn clip_of_studio_v2(animation: &Json) -> (Clip, Vec<String>) {
    assert_eq!(animation["formatVersion"], 2, "a format 2 animation");
    let mut muted = Vec::new();
    let tracks = animation["tracks"]
        .as_array()
        .expect("tracks")
        .iter()
        .map(|track| {
            let id = track["id"].as_str().expect("a track id").to_string();
            if track["settings"]["muted"] == Json::Bool(true) {
                muted.push(id.clone());
            }
            Track {
                name: track["name"].as_str().unwrap_or_default().to_string(),
                target: track["animatableId"]
                    .as_str()
                    .expect("an animatable")
                    .to_string(),
                keypoints: track["points"]
                    .as_array()
                    .expect("points")
                    .iter()
                    .map(keypoint_of_studio_v2)
                    .collect(),
                id,
            }
        })
        .collect();
    let clip = Clip {
        name: animation["name"].as_str().unwrap_or_default().to_string(),
        length: None,
        tracks,
    };
    clip.validate().expect("a clip that plays");
    (clip, muted)
}

fn keypoint_of_studio_v2(point: &Json) -> Keypoint {
    let value = match &point["value"] {
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::Number(n.as_f64().expect("a number")),
        Json::String(s) => Value::Text(s.clone()),
        other => panic!("a scalar value, not {other}"),
    };
    let side = |side: &Json| -> Option<Transition> {
        match side {
            Json::Null => None,
            Json::String(name) => Some(Transition::named(name).expect("a known transition")),
            Json::Object(handle) => Some(Transition::Handle {
                dt: handle["x"].as_f64().expect("x") * 1e6,
                dv: handle["y"].as_f64().expect("y"),
            }),
            other => panic!("a transition, not {other}"),
        }
    };
    let transitions = &point["transitions"];
    Keypoint {
        id: point["id"].as_str().expect("a keypoint id").to_string(),
        time: nanos_from_millis(point["stamp"].as_f64().expect("a stamp")).expect("a time"),
        value,
        transitions: Transitions {
            arrive: side(&transitions["in"]),
            leave: side(&transitions["out"]),
            pairing: match transitions["pairing"].as_str() {
                Some("paired") => Some(Pairing::Paired),
                Some("free") => Some(Pairing::Free),
                _ => None,
            },
        },
    }
}

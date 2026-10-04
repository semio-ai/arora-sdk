//! A synthetic robot model for tests: a GLB whose only chunk is JSON, with
//! `RobotData` joints and no mesh.
//!
//! Tests use it in place of a real robot model. No build step provides a real
//! model, and a real model can carry a licensed mesh, so none is in the
//! repository. The fixture is built here in code, so a reader can see all of it.

use std::sync::OnceLock;

use serde_json::{json, Value};

/// The animated joints of the fixture: the ROS joint name and its Arora id.
pub(crate) const FIXTURE_JOINTS: [(&str, &str); 2] = [
    ("HeadYaw", "9e48ada8-f7c4-4c15-8ec0-cb7b2d3abc3e"),
    ("HeadPitch", "f7d93a6b-9cc0-494f-a7ec-11c61a369a68"),
];

/// The path of the fixture GLB. The first call writes it to the temp directory,
/// once per test process.
///
/// The write deletes what is at the path first and then creates a new file.
/// Thus it never writes through a symlink that is already at the path, and a
/// stale file from an earlier process with the same id does not stay.
pub(crate) fn fixture_glb_path() -> String {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let path =
            std::env::temp_dir().join(format!("arora-hal-ros2-fixture-{}.glb", std::process::id()));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => panic!("remove the old fixture GLB: {e}"),
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("create the fixture GLB");
        std::io::Write::write_all(&mut file, &fixture_glb_bytes()).expect("write the fixture GLB");
        path.to_str().expect("a UTF-8 temp path").to_string()
    })
    .clone()
}

/// The bytes of the fixture GLB: a glTF 2.0 binary with one JSON chunk.
///
/// Besides the animated joints of [`FIXTURE_JOINTS`], the nodes include a
/// plain node and a joint that is not animated. The joint-id parser must skip
/// both.
pub(crate) fn fixture_glb_bytes() -> Vec<u8> {
    let mut nodes: Vec<Value> = FIXTURE_JOINTS
        .iter()
        .map(|(name, id)| joint_node(name, id, true))
        .collect();
    nodes.push(json!({ "name": "base_link" }));
    nodes.push(joint_node(
        "StaticJoint",
        "b7312386-138a-4ec3-b247-99ede64be517",
        false,
    ));

    let mut chunk =
        serde_json::to_vec(&json!({ "asset": { "version": "2.0" }, "nodes": nodes })).unwrap();
    // A GLB chunk length is a multiple of 4; JSON chunks pad with spaces.
    while !chunk.len().is_multiple_of(4) {
        chunk.push(b' ');
    }

    const GLB_HEADER_LEN: usize = 12;
    const CHUNK_HEADER_LEN: usize = 8;
    const JSON_CHUNK_TYPE: u32 = 0x4E4F_534A; // "JSON"
    let total = GLB_HEADER_LEN + CHUNK_HEADER_LEN + chunk.len();

    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&JSON_CHUNK_TYPE.to_le_bytes());
    bytes.extend_from_slice(&chunk);
    bytes
}

fn joint_node(name: &str, id: &str, animated: bool) -> Value {
    json!({
        "name": name,
        "extensions": {
            "RobotData": {
                "features": {
                    "jointValue": {
                        "animated": animated,
                        "value": { "name": name, "id": id }
                    }
                }
            }
        }
    })
}

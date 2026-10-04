/// The default model path of a built-in robot:
/// `default_model_path!("nao")` expands to `$CARGO_MANIFEST_DIR/models/nao.glb`.
///
/// Nothing fills `models/` at build time. To run a built-in robot, put its GLB
/// at this path, or set `model_glb_path` in a robot config or an overrides file
/// (`arora-ros2 nao overrides.json`).
#[macro_export]
macro_rules! default_model_path {
    ($name:expr) => {
        concat!(env!("CARGO_MANIFEST_DIR"), "/models/", $name, ".glb")
    };
}

// Re-export the robot configurations
pub mod nao;
pub mod pepper;
pub mod quori;
pub mod unitree_g1;
pub mod ur3;
pub mod ur5;

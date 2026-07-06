/// Macro to get the path to a model file at compile time.
/// Usage: `default_model_path!("nao")` expands to the full path to "$CARGO_MANIFEST_DIR/models/nao.glb"
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

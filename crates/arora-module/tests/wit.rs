//! The `arora:module` world the guest bindings are generated from is the one
//! the engine hosts components against.

#[test]
fn the_guest_and_the_engine_share_one_world() {
    let guest = include_str!("../wit/arora-module.wit");
    let engine = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../arora-engine/wit/arora-module.wit"
    ))
    .expect("the engine's world, beside this crate in the workspace");
    assert_eq!(
        guest, engine,
        "wit/arora-module.wit differs from arora-engine's"
    );
}

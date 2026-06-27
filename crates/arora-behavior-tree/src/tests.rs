//! Engine-free tests. Tests that need a real engine moved to `arora-sdk`.
use crate::load_behavior_tree_yaml;
use anyhow::Result;

#[test]
pub fn load_parse_error() -> Result<()> {
    let tree_yaml = "I'm singing in the rain...";
    assert!(load_behavior_tree_yaml(tree_yaml).is_err());
    Ok(())
}

#[test]
pub fn load_simple_tree() -> Result<()> {
    let tree_yaml = &crate::schema::tests::SIMPLE_TREE_YAML;
    load_behavior_tree_yaml(tree_yaml)?;
    Ok(())
}

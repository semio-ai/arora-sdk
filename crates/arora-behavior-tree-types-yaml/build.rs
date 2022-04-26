use arora_behavior_tree_types::{
  declare_behavior_tree_folder, declare_status_enumeration, declare_tick_id_structure,
  BEHAVIOR_TREE_FOLDER_ID, STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION, TICK_ID_STRUCTURE_ID,
  TICK_ID_STRUCTURE_VERSION,
};
use arora_registry::{local::LocalRegistry, EditableRegistry};
use std::fs::{create_dir_all, write};
use uuid::Uuid;

#[tokio::main]
pub async fn main() {
  // We put everything in a registry so we can freeze the version numbers properly,
  // and produce a consistent set of files.
  let mut registry = LocalRegistry::new();

  // Folders. They cannot be frozen, we can serialize them as-is.
  let folders_dir = "records/folder/";
  create_dir_all(folders_dir).unwrap();
  println!("cargo:rerun-if-changed={}", folders_dir);

  // behavior_tree
  let behavior_tree_folder = declare_behavior_tree_folder(ROOT_ID);
  let behavior_tree_folder_yaml = serde_yaml::to_string(&behavior_tree_folder).unwrap();
  registry
    .add_folder(BEHAVIOR_TREE_FOLDER_ID, behavior_tree_folder)
    .await
    .unwrap();
  let behavior_tree_folder_path = format!("{}/{}.yaml", folders_dir, BEHAVIOR_TREE_FOLDER_ID);
  write(&behavior_tree_folder_path, behavior_tree_folder_yaml).unwrap();
  println!("cargo:rerun-if-changed={}", behavior_tree_folder_path);

  // Enumerations. They can be frozen, we serialize their frozen version.
  let enumerations_dir = "records/enumeration/";
  create_dir_all(&enumerations_dir).unwrap();
  println!("cargo:rerun-if-changed={}", enumerations_dir);

  // behavior_tree::Status
  let status_enumeration = declare_status_enumeration(BEHAVIOR_TREE_FOLDER_ID);
  let status_enumeration = registry
    .tag_enumeration(
      STATUS_ENUMERATION_ID,
      STATUS_ENUMERATION_VERSION,
      status_enumeration,
    )
    .await
    .unwrap();
  let status_enumeration_path = format!(
    "{}/{}@{}.yaml",
    enumerations_dir, STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION
  );
  let status_enumeration_yaml = serde_yaml::to_string(&status_enumeration).unwrap();
  write(&status_enumeration_path, status_enumeration_yaml).unwrap();
  println!("cargo:rerun-if-changed={}", status_enumeration_path);

  // Structures. They can be frozen, we serialize their frozen version.
  let structures_dir = "records/structure/";
  create_dir_all(&structures_dir).unwrap();
  println!("cargo:rerun-if-changed={}", structures_dir);

  // behavior_tree::TickId
  let tick_id_structure = declare_tick_id_structure(BEHAVIOR_TREE_FOLDER_ID);
  let tick_id_structure = registry
    .tag_structure(
      TICK_ID_STRUCTURE_ID,
      TICK_ID_STRUCTURE_VERSION,
      tick_id_structure,
    )
    .await
    .unwrap();
  let tick_id_structure_path = format!(
    "{}/{}@{}.yaml",
    structures_dir, TICK_ID_STRUCTURE_ID, TICK_ID_STRUCTURE_VERSION
  );
  let tick_id_structure_yaml = serde_yaml::to_string(&tick_id_structure).unwrap();
  write(&tick_id_structure_path, tick_id_structure_yaml).unwrap();
  println!("cargo:rerun-if-changed={}", tick_id_structure_path);
}

pub const ROOT_ID: Uuid = Uuid::from_bytes([
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
]);

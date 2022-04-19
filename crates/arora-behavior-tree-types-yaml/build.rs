use arora_behavior_tree_types::{
  declare_behavior_tree_folder, declare_status_enumeration, declare_tick_id_structure,
  BEHAVIOR_TREE_FOLDER_ID, STATUS_ENUMERATION_ID, TICK_ID_STRUCTURE_ID,
};
use std::fs::{create_dir_all, write};
use uuid::Uuid;

pub fn main() {
  // Folders.
  let folders_dir = "records/folder/";
  create_dir_all(folders_dir).unwrap();
  // behavior_tree
  let behavior_tree_folder_path = format!("{}/{}.yaml", folders_dir, BEHAVIOR_TREE_FOLDER_ID);
  let behavior_tree_folder = declare_behavior_tree_folder(ROOT_ID);
  let behavior_tree_folder_yaml = serde_yaml::to_string(&behavior_tree_folder).unwrap();
  write(behavior_tree_folder_path, behavior_tree_folder_yaml).unwrap();

  // Enumerations
  let enumerations_dir = "records/enumeration/";
  create_dir_all(&enumerations_dir).unwrap();
  // behavior_tree::Status
  let status_enumeration_path = format!("{}/{}.yaml", enumerations_dir, STATUS_ENUMERATION_ID);
  let status_enumeration = declare_status_enumeration(BEHAVIOR_TREE_FOLDER_ID);
  let status_enumeration_yaml = serde_yaml::to_string(&status_enumeration).unwrap();
  write(status_enumeration_path, status_enumeration_yaml).unwrap();

  // Structures.
  let structures_dir = "records/structure/";
  create_dir_all(&structures_dir).unwrap();
  // behavior_tree::TickId
  let tick_id_structure_path = format!("{}/{}.yaml", structures_dir, TICK_ID_STRUCTURE_ID);
  let tick_id_structure = declare_tick_id_structure(BEHAVIOR_TREE_FOLDER_ID);
  let tick_id_structure_yaml = serde_yaml::to_string(&tick_id_structure).unwrap();
  write(&tick_id_structure_path, tick_id_structure_yaml).unwrap();
}

pub const ROOT_ID: Uuid = Uuid::from_bytes([
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
]);

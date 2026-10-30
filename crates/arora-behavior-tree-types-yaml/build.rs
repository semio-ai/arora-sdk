use arora_behavior_tree_types::{
    declare_status_enumeration, declare_tick_id_structure, STATUS_ENUMERATION_ID,
    STATUS_ENUMERATION_VERSION, TICK_ID_STRUCTURE_ID, TICK_ID_STRUCTURE_VERSION,
};
use arora_registry::{local::LocalRegistry, EditableRegistry};
use arora_types::record::scope::ARORA;
use std::fs::{create_dir_all, write};

#[tokio::main]
pub async fn main() {
    // We put everything in a registry so we can freeze the version numbers properly,
    // and produce a consistent set of files.
    let mut registry = LocalRegistry::new();

    // Folders. They cannot be frozen, we can serialize them as-is.
    let crate_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let folders_dir = crate_dir.join("records/folder/");
    create_dir_all(&folders_dir).unwrap();
    let folders_dir_str = folders_dir.to_str().unwrap();
    println!("cargo:rerun-if-changed={}", folders_dir_str);

    // The `arora` scope, where `Status` and `TickId` live.
    let arora_folder = ARORA.record();
    let arora_folder_yaml = yaml_serde::to_string(&arora_folder).unwrap();
    registry.add_folder(ARORA.id, arora_folder).await.unwrap();
    let arora_folder_path = format!("{}/{}.yaml", folders_dir_str, ARORA.id);
    write(&arora_folder_path, arora_folder_yaml).unwrap();
    println!("cargo:rerun-if-changed={}", arora_folder_path);

    // Enumerations. They can be frozen, we serialize their frozen version.
    let enumerations_dir = crate_dir.join("records/enumeration/");
    let enumerations_dir_str = enumerations_dir.to_str().unwrap();
    create_dir_all(&enumerations_dir).unwrap();
    println!("cargo:rerun-if-changed={}", enumerations_dir_str);

    // arora.Status
    let status_enumeration = declare_status_enumeration(ARORA.id);
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
        enumerations_dir_str, STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION
    );
    let status_enumeration_yaml = yaml_serde::to_string(&status_enumeration).unwrap();
    write(&status_enumeration_path, status_enumeration_yaml).unwrap();
    println!("cargo:rerun-if-changed={}", status_enumeration_path);

    // Structures. They can be frozen, we serialize their frozen version.
    let structures_dir = crate_dir.join("records/structure/");
    let structures_dir_str = structures_dir.to_str().unwrap();
    create_dir_all(&structures_dir).unwrap();
    println!("cargo:rerun-if-changed={}", structures_dir_str);

    // arora.TickId
    let tick_id_structure = declare_tick_id_structure(ARORA.id);
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
        structures_dir_str, TICK_ID_STRUCTURE_ID, TICK_ID_STRUCTURE_VERSION
    );
    let tick_id_structure_yaml = yaml_serde::to_string(&tick_id_structure).unwrap();
    write(&tick_id_structure_path, tick_id_structure_yaml).unwrap();
    println!("cargo:rerun-if-changed={}", tick_id_structure_path);
}

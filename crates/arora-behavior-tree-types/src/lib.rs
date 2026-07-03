mod status;
mod tick_id;
use arora_types::record::folder::public::Public as FolderPublic;
pub use status::*;
pub use tick_id::*;
use uuid::Uuid;

pub fn declare_behavior_tree_folder(parent: Uuid) -> FolderPublic {
    FolderPublic {
        name: "behavior_tree".to_string(),
        parent,
    }
}

/// Use this ID to register the folder to a registry.
pub const BEHAVIOR_TREE_FOLDER_ID: Uuid = Uuid::from_bytes([
    0x12, 0x32, 0xd7, 0xc4, 0xd5, 0xaf, 0x4f, 0x91, 0x9a, 0x34, 0x8c, 0x70, 0x7b, 0x0c, 0x96, 0x93,
]);

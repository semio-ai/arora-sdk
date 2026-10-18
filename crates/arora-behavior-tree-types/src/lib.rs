mod status;
mod tick_id;
use arora_types::record::folder::public::Public as FolderPublic;
pub use status::*;
pub use tick_id::*;
use uuid::Uuid;

pub use arora_behavior::BEHAVIOR_TREE_FOLDER;

pub fn declare_behavior_tree_folder(parent: Uuid) -> FolderPublic {
    FolderPublic {
        name: BEHAVIOR_TREE_FOLDER.name.to_string(),
        parent,
    }
}

/// Use this ID to register the folder to a registry.
pub const BEHAVIOR_TREE_FOLDER_ID: Uuid = BEHAVIOR_TREE_FOLDER.id;

mod status;
mod tick_id;
use arora_types::record::folder::public::Public as FolderPublic;
pub use status::*;
pub use tick_id::*;
use uuid::Uuid;

/// The `behavior_tree` folder's record, under `parent`.
#[deprecated(
    since = "2.2.0",
    note = "`Status` and `TickId` live in the `arora` scope: `arora_types::record::scope::ARORA`"
)]
pub fn declare_behavior_tree_folder(parent: Uuid) -> FolderPublic {
    FolderPublic {
        name: "behavior_tree".to_string(),
        parent,
    }
}

/// The `behavior_tree` folder's id.
#[deprecated(
    since = "2.2.0",
    note = "`Status` and `TickId` live in the `arora` scope: `arora_types::record::scope::ARORA`"
)]
pub const BEHAVIOR_TREE_FOLDER_ID: Uuid = Uuid::from_bytes([
    0x12, 0x32, 0xd7, 0xc4, 0xd5, 0xaf, 0x4f, 0x91, 0x9a, 0x34, 0x8c, 0x70, 0x7b, 0x0c, 0x96, 0x93,
]);

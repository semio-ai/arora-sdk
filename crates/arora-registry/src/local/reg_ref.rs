use crate::{
  EnumerationFrozen, EnumerationPublic, FolderPublic, ModuleFrozen, ModulePublic, StructureFrozen,
  StructurePublic,
};
use std::rc::Rc;
use uuid::Uuid;

use super::ROOT_ID;

/// Common trait to inner local references to records.
pub trait LocalRegistryReference {
  /// The identifier of the record.
  fn id(&self) -> &Uuid;

  /// The name of the record. Root does not have a name.
  fn name(&self) -> Option<&String>;

  /// True if this record is the root.
  fn is_root(&self) -> bool;

  /// Returns the identifier of the parent if this record is not the root.
  /// The parent may be the [`ROOT_ID`] of this is a top-level record
  fn parent(&self) -> Option<&Uuid>;
}

/// A reference to a "latest" record stored in a [`LocalRegistry`].
#[derive(Clone)]
pub enum LatestRegistryReference {
  Enumeration {
    id: Uuid,
    record: Rc<EnumerationPublic>,
  },
  Variant {
    id: Uuid,
    parent_id: Uuid,
    parent_record: Rc<EnumerationPublic>,
  },
  Structure {
    id: Uuid,
    record: Rc<StructurePublic>,
  },
  Field {
    id: Uuid,
    parent_id: Uuid,
    parent_record: Rc<StructurePublic>,
  },
  Module {
    id: Uuid,
    record: Rc<ModulePublic>,
  },
  Function {
    id: Uuid,
    parent_id: Uuid,
    parent_record: Rc<ModulePublic>,
  },
  Folder {
    id: Uuid,
    record: Rc<FolderPublic>,
  },
  Root,
}

impl LocalRegistryReference for LatestRegistryReference {
  fn id(&self) -> &Uuid {
    match self {
      Self::Enumeration { id, .. } => id,
      Self::Variant { id, .. } => id,
      Self::Structure { id, .. } => id,
      Self::Field { id, .. } => id,
      Self::Module { id, .. } => id,
      Self::Function { id, .. } => id,
      Self::Folder { id, .. } => id,
      Self::Root => &ROOT_ID,
    }
  }

  fn is_root(&self) -> bool {
    match self {
      LatestRegistryReference::Root => true,
      _ => false,
    }
  }

  fn parent(&self) -> Option<&Uuid> {
    match self {
      Self::Enumeration { record, .. } => Some(&record.parent),
      Self::Variant { parent_id, .. } => Some(parent_id),
      Self::Structure { record, .. } => Some(&record.parent),
      Self::Field { parent_id, .. } => Some(parent_id),
      Self::Module { record, .. } => Some(&record.parent),
      Self::Function { parent_id, .. } => Some(parent_id),
      Self::Folder { record, .. } => Some(&record.parent),
      Self::Root => None,
    }
  }

  fn name(&self) -> Option<&String> {
    Some(match self {
      Self::Enumeration { record, .. } => &record.name,
      Self::Variant {
        id, parent_record, ..
      } => {
        &parent_record
          .variants
          .get(id)
          .expect("looking up a variant id not known to its enumeration")
          .name
      }
      Self::Structure { record, .. } => &record.name,
      Self::Field {
        id, parent_record, ..
      } => {
        &parent_record
          .fields
          .get(id)
          .expect("looking up a field id not known to its structure")
          .name
      }
      Self::Module { record, .. } => &record.name,
      Self::Function {
        id, parent_record, ..
      } => {
        &parent_record
          .exports
          .get(id)
          .expect("looking up an export id not known to its module")
          .name
      }
      Self::Folder { record, .. } => &record.name,
      Self::Root => return None,
    })
  }
}

/// A reference to a frozen record stored in a [`LocalRegistry`].
#[derive(Clone)]
pub enum FrozenRegistryReference {
  Enumeration {
    id: Uuid,
    record: Rc<EnumerationFrozen>,
  },
  Variant {
    id: Uuid,
    parent_id: Uuid,
    parent_record: Rc<EnumerationFrozen>,
  },
  Structure {
    id: Uuid,
    record: Rc<StructureFrozen>,
  },
  Field {
    id: Uuid,
    parent_id: Uuid,
    parent_record: Rc<StructureFrozen>,
  },
  Module {
    id: Uuid,
    record: Rc<ModuleFrozen>,
  },
  Function {
    id: Uuid,
    parent_id: Uuid,
    parent_record: Rc<ModuleFrozen>,
  },
  Folder {
    id: Uuid,
    record: Rc<FolderPublic>, // Folders cannot be frozen
  },
  Root,
}

impl LocalRegistryReference for FrozenRegistryReference {
  fn id(&self) -> &Uuid {
    todo!()
  }

  fn name(&self) -> Option<&String> {
    Some(match self {
      Self::Enumeration { record, .. } => &record.name,
      Self::Variant {
        id, parent_record, ..
      } => {
        &parent_record
          .variants
          .get(id)
          .expect("looking up a variant id not known to its enumeration")
          .name
      }
      Self::Structure { record, .. } => &record.name,
      Self::Field {
        id, parent_record, ..
      } => {
        &parent_record
          .fields
          .get(id)
          .expect("looking up a field id not known to its structure")
          .name
      }
      Self::Module { record, .. } => &record.name,
      Self::Function {
        id, parent_record, ..
      } => {
        &parent_record
          .exports
          .get(id)
          .expect("looking up an export id not known to its module")
          .name
      }
      Self::Folder { record, .. } => &record.name,
      Self::Root => return None,
    })
  }

  fn is_root(&self) -> bool {
    match self {
      FrozenRegistryReference::Root => true,
      _ => false,
    }
  }

  fn parent(&self) -> Option<&Uuid> {
    match self {
      FrozenRegistryReference::Enumeration { record, .. } => Some(&record.parent),
      FrozenRegistryReference::Variant { parent_id, .. } => Some(parent_id),
      FrozenRegistryReference::Structure { record, .. } => Some(&record.parent),
      FrozenRegistryReference::Field { parent_id, .. } => Some(parent_id),
      FrozenRegistryReference::Module { record, .. } => Some(&record.parent),
      FrozenRegistryReference::Function { parent_id, .. } => Some(parent_id),
      FrozenRegistryReference::Folder { record, .. } => Some(&record.parent),
      FrozenRegistryReference::Root => None,
    }
  }
}

use semio_record::{
  acl::Acl,
  enumeration::v0::unfrozen::{Enumeration, EnumerationVariant},
  ty::{Primitive, PrimitiveKind, UnfrozenTy},
};
use semver::Version;
use uuid::Uuid;

/// Produces the declaration of the `Status` enum,
/// so that it can be added to a registry,
/// with the given parent, and under the ID [`STATUS_ENUMERATION_ID`].
pub fn declare_status_enumeration(parent: Uuid) -> Enumeration {
  Enumeration {
    name: "Status".to_string(),
    parent,
    variants: [
      (
        STATUS_SUCCESS_VARIANT_ID,
        EnumerationVariant {
          name: "Success".to_string(),
          ty: UnfrozenTy::Primitive(Primitive {
            kind: PrimitiveKind::Unit,
          }),
        },
      ),
      (
        STATUS_FAILURE_VARIANT_ID,
        EnumerationVariant {
          name: "Failure".to_string(),
          ty: UnfrozenTy::Primitive(Primitive {
            kind: PrimitiveKind::Unit,
          }),
        },
      ),
      (
        STATUS_RUNNING_VARIANT_ID,
        EnumerationVariant {
          name: "Running".to_string(),
          ty: UnfrozenTy::Primitive(Primitive {
            kind: PrimitiveKind::Unit,
          }),
        },
      ),
    ]
    .into_iter()
    .collect(),
    acl: <Acl as Default>::default(),
  }
}

/// Use this ID to register the type to a registry.
pub const STATUS_ENUMERATION_ID: Uuid = Uuid::from_bytes([
  0x32, 0x5a, 0x57, 0x67, 0xe3, 0x44, 0x45, 0x32, 0x86, 0x0e, 0x07, 0x49, 0xbc, 0xf2, 0xe4, 0x28,
]);
pub const STATUS_SUCCESS_VARIANT_ID: Uuid = Uuid::from_bytes([
  0x76, 0x6e, 0x9e, 0x9a, 0x44, 0x6d, 0x4e, 0x46, 0x83, 0xe6, 0x14, 0xb7, 0xca, 0x10, 0x11, 0x69,
]);
pub const STATUS_FAILURE_VARIANT_ID: Uuid = Uuid::from_bytes([
  0x24, 0x68, 0xf4, 0x6c, 0xbb, 0x60, 0x42, 0x5c, 0x9a, 0x4d, 0x9a, 0xd3, 0x26, 0xcc, 0xc7, 0xe2,
]);
pub const STATUS_RUNNING_VARIANT_ID: Uuid = Uuid::from_bytes([
  0xac, 0xd7, 0x9e, 0xc6, 0x0c, 0x44, 0x40, 0x1a, 0x82, 0xf8, 0x5d, 0xa5, 0x42, 0x2d, 0x3e, 0xec,
]);
pub const STATUS_ENUMERATION_VERSION: Version = Version::new(1, 0, 0);

#[cfg(test)]
pub mod tests {

  use semio_record::enumeration::v0::unfrozen::Enumeration;

  use crate::{declare_status_enumeration, BEHAVIOR_TREE_FOLDER_ID};

  #[test]
  pub fn serialize_status() {
    let status_declaration = declare_status_enumeration(BEHAVIOR_TREE_FOLDER_ID);
    let status_declaration_yaml = serde_yaml::to_string(&status_declaration).unwrap();
    let parsed_status_declaration: Enumeration =
      serde_yaml::from_str(&status_declaration_yaml).unwrap();
    assert_eq!(status_declaration, parsed_status_declaration);
  }
}

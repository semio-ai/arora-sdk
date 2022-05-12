use crate::{
  resolve::{resolve_low_module, ModuleAndImports},
  ImportAsset, ModuleDeclarationError,
};
use arora_registry::{ModuleFrozen, ReadableRegistry};
use arora_schema::{
  module::low::{
    Executor, ExportFunction, ExportSymbol, Header, ImportFunction, ImportSymbol, Parameter,
    TypeRef,
  },
  ty::{
    BOOLEAN_ID, F32_ID, F64_ID, I16_ID, I32_ID, I64_ID, I8_ID, STRING_ID, U16_ID, U32_ID, U64_ID,
    U8_ID, UNIT_ID,
  },
  SemanticVersion,
};
use arora_vfs::{Directory, Entry, File};
use semio_record::{
  module::v0::frozen::ExportKind,
  record::Freezer,
  ty::{FrozenTy, PrimitiveKind},
};
use semver::Version;
use std::path::Path;
use tokio::fs::read_to_string;
use uuid::Uuid;

/// Creates a YAML header file named `module.yaml` describing the module.
pub fn generate_header_file(
  id: &Uuid,
  module: &ModuleFrozen,
  imports: &Vec<ImportAsset>,
  executor: &String,
) -> Result<Directory, ModuleDeclarationError> {
  let header = Header {
    id: id.to_owned(),
    name: module.name.to_owned(),
    author: String::new(),
    description: None,
    license: String::new(),
    version: SemanticVersion {
      major: 0u32,
      minor: 0u32,
      patch: 0u32,
    },
    executor: Executor {
      name: executor.to_owned(),
      min_version: None,
      max_version: None,
    },
    exports: module
      .exports
      .iter()
      .map(|(export_id, export)| {
        let ExportKind::Function(function) = &export.kind;
        ExportSymbol::Function(ExportFunction {
          id: export_id.to_owned(),
          name: export.name.to_owned(),
          parameters: function
            .parameter_ordering
            .iter()
            .map(|parameter_id| {
              let parameter = function.parameters.get(parameter_id).unwrap();
              Parameter {
                name: parameter.name.to_owned(),
                ty: low_type_ref_from_unfrozen_ty(&parameter.ty),
                mutable: parameter.mutable,
                id: parameter_id.to_owned(),
                default_value: None,
              }
            })
            .collect(),
          ret: low_type_ref_from_unfrozen_ty(&function.return_ty),
        })
      })
      .collect(),
    imports: imports
      .iter()
      .map(|import| {
        let ExportKind::Function(import_function) = &import.import.kind;
        ImportSymbol::Function(ImportFunction {
          id: import.id.to_owned(),
          name: import.import.name.to_owned(),
          module: import.module_id.to_owned(),
          parameters: import_function
            .parameter_ordering
            .iter()
            .map(|parameter_id| {
              let parameter = import_function.parameters.get(parameter_id).unwrap();
              Parameter {
                name: parameter.name.to_owned(),
                ty: low_type_ref_from_unfrozen_ty(&parameter.ty),
                mutable: parameter.mutable,
                id: parameter_id.to_owned(),
                default_value: None,
              }
            })
            .collect(),
          ret: low_type_ref_from_unfrozen_ty(&import_function.return_ty),
        })
      })
      .collect(),
    executable_mime: "".to_string(),
  };
  let mut result = Directory::new();
  let header_file = File::new(serde_yaml::to_string(&header).unwrap().as_bytes());
  result
    .insert("module.yaml", Entry::File(header_file))
    .map_err(ModuleDeclarationError::VfsError)?;
  Ok(result)
}

/// Reads the YAML header file at the given path
/// and returns a description compatible with the registry.
pub async fn module_frozen_from_header_file<P: AsRef<Path>, R: ReadableRegistry + Freezer>(
  header_path: P,
  registry: &mut R,
) -> Result<(Uuid, Version, ModuleAndImports), ModuleDeclarationError> {
  let header: Header = serde_yaml::from_str(
    &read_to_string(header_path.as_ref())
      .await
      .map_err(ModuleDeclarationError::IoError)?,
  )
  .map_err(|e| {
    ModuleDeclarationError::Generic(format!(
      "header file {} contains invalid yaml: {}",
      header_path.as_ref().display(),
      e
    ))
  })?;
  Ok((
    header.id.to_owned(),
    header.version.to_owned().into(),
    resolve_low_module(header, registry).await?,
  ))
}

fn low_type_ref_from_unfrozen_ty(unfrozen: &FrozenTy) -> TypeRef {
  match unfrozen {
    FrozenTy::Primitive(primitive) => match primitive.kind {
      PrimitiveKind::Unit => TypeRef::Scalar {
        id: UNIT_ID.to_owned(),
      },
      PrimitiveKind::Boolean => TypeRef::Scalar {
        id: BOOLEAN_ID.to_owned(),
      },
      PrimitiveKind::U8 => TypeRef::Scalar {
        id: U8_ID.to_owned(),
      },
      PrimitiveKind::U16 => TypeRef::Scalar {
        id: U16_ID.to_owned(),
      },
      PrimitiveKind::U32 => TypeRef::Scalar {
        id: U32_ID.to_owned(),
      },
      PrimitiveKind::U64 => TypeRef::Scalar {
        id: U64_ID.to_owned(),
      },
      PrimitiveKind::I8 => TypeRef::Scalar {
        id: I8_ID.to_owned(),
      },
      PrimitiveKind::I16 => TypeRef::Scalar {
        id: I16_ID.to_owned(),
      },
      PrimitiveKind::I32 => TypeRef::Scalar {
        id: I32_ID.to_owned(),
      },
      PrimitiveKind::I64 => TypeRef::Scalar {
        id: I64_ID.to_owned(),
      },
      PrimitiveKind::F32 => TypeRef::Scalar {
        id: F32_ID.to_owned(),
      },
      PrimitiveKind::F64 => TypeRef::Scalar {
        id: F64_ID.to_owned(),
      },
      PrimitiveKind::String => TypeRef::Scalar {
        id: STRING_ID.to_owned(),
      },
      PrimitiveKind::ArrayBoolean => TypeRef::Array {
        id: BOOLEAN_ID.to_owned(),
      },
      PrimitiveKind::ArrayU8 => TypeRef::Array {
        id: U8_ID.to_owned(),
      },
      PrimitiveKind::ArrayU16 => TypeRef::Array {
        id: U16_ID.to_owned(),
      },
      PrimitiveKind::ArrayU32 => TypeRef::Array {
        id: U32_ID.to_owned(),
      },
      PrimitiveKind::ArrayU64 => TypeRef::Array {
        id: U64_ID.to_owned(),
      },
      PrimitiveKind::ArrayI8 => TypeRef::Array {
        id: I8_ID.to_owned(),
      },
      PrimitiveKind::ArrayI16 => TypeRef::Array {
        id: I16_ID.to_owned(),
      },
      PrimitiveKind::ArrayI32 => TypeRef::Array {
        id: I32_ID.to_owned(),
      },
      PrimitiveKind::ArrayI64 => TypeRef::Array {
        id: I64_ID.to_owned(),
      },
      PrimitiveKind::ArrayF32 => TypeRef::Array {
        id: F32_ID.to_owned(),
      },
      PrimitiveKind::ArrayF64 => TypeRef::Array {
        id: F64_ID.to_owned(),
      },
      PrimitiveKind::ArrayString => TypeRef::Array {
        id: STRING_ID.to_owned(),
      },
    },
    FrozenTy::FrozenScalar(scalar) => TypeRef::Scalar {
      id: scalar.reference.id.to_owned(),
    },
    FrozenTy::FrozenArray(array) => TypeRef::Array {
      id: array.reference.id.to_owned(),
    },
  }
}

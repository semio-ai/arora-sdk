use crate::{ImportAsset, ModuleDeclarationError};
use arora_registry::local::ROOT_ID;
use arora_registry::{get_primitive, ModulePublic, ReadableRegistry};
use arora_schema::module::low::Header;
use arora_schema::module::{
  high::{
    ExportSymbol as HighExportSymbol, ImportSymbol as HighImportSymbol,
    ModuleDefinition as HighModuleDefinition, Parameter as HighParameter, TypeRef as HighTypeRef,
  },
  low::{
    ExportSymbol as LowExportSymbol, ImportSymbol as LowImportSymbol, Parameter as LowParameter,
    TypeRef as LowTypeRef,
  },
};
use convert_case::{Case, Casing};
use semio_client::common::Selector;
use semio_record::{
  module::v0::unfrozen::{Export, Function, Parameter},
  record::{UnfrozenReference, VersionReq},
  ty::{UnfrozenArray, UnfrozenScalar, UnfrozenTy},
};
use std::collections::HashSet;
use std::{collections::HashMap, str::FromStr};
use uuid::Uuid;

pub async fn resolve_type_id(
  name: &str,
  registry: &mut dyn ReadableRegistry,
) -> Result<Uuid, ModuleDeclarationError> {
  Ok(match Uuid::parse_str(name) {
    Ok(id) => id,
    Err(_) => registry
      .resolve_path(&name.to_string())
      .await
      .map_err(ModuleDeclarationError::RegistryError)?,
  })
}

pub async fn resolve_module_id(
  name: &str,
  registry: &mut dyn ReadableRegistry,
) -> Result<Uuid, ModuleDeclarationError> {
  Ok(match Uuid::parse_str(name) {
    Ok(id) => id,
    Err(_) => registry
      .resolve_path(&name.to_string())
      .await
      .map_err(ModuleDeclarationError::RegistryError)?,
  })
}

pub async fn resolve_high_type_ref(
  type_ref: &HighTypeRef,
  registry: &mut dyn ReadableRegistry,
) -> Result<UnfrozenTy, ModuleDeclarationError> {
  match type_ref {
    HighTypeRef::Scalar { id } => {
      let selector = Selector::from_str(id).map_err(ModuleDeclarationError::Generic)?;
      if let Some(primitive) = get_primitive(&selector) {
        Ok(UnfrozenTy::Primitive(primitive.into()))
      } else {
        Ok(UnfrozenTy::UnfrozenScalar(UnfrozenScalar {
          reference: unfrozen_reference_from_name(id, registry).await?,
        }))
      }
    }
    HighTypeRef::Array { id } => {
      let selector = Selector::from_str(id).map_err(ModuleDeclarationError::Generic)?;
      if let Some(primitive) = get_primitive(&selector) {
        Ok(UnfrozenTy::Primitive(primitive.into()))
      } else {
        Ok(UnfrozenTy::UnfrozenArray(UnfrozenArray {
          reference: unfrozen_reference_from_name(id, registry).await?,
        }))
      }
    }
    _ => Err(ModuleDeclarationError::Generic(format!(
      "Unsupported type ref: {:?}",
      type_ref
    ))),
  }
}

pub async fn resolve_low_type_ref(
  type_ref: &LowTypeRef,
) -> Result<UnfrozenTy, ModuleDeclarationError> {
  match type_ref {
    LowTypeRef::Scalar { id } => {
      let selector = Selector::Id(id.to_owned());
      if let Some(primitive) = get_primitive(&selector) {
        Ok(UnfrozenTy::Primitive(primitive.into()))
      } else {
        Ok(UnfrozenTy::UnfrozenScalar(UnfrozenScalar {
          reference: unfrozen_reference_from_id(id).await?,
        }))
      }
    }
    LowTypeRef::Array { id } => {
      let selector = Selector::Id(id.to_owned());
      if let Some(primitive) = get_primitive(&selector) {
        Ok(UnfrozenTy::Primitive(primitive.into()))
      } else {
        Ok(UnfrozenTy::UnfrozenArray(UnfrozenArray {
          reference: unfrozen_reference_from_id(id).await?,
        }))
      }
    }
    _ => Err(ModuleDeclarationError::Generic(format!(
      "Unsupported type ref: {:?}",
      type_ref
    ))),
  }
}

async fn unfrozen_reference_from_name(
  name_or_id: &str,
  registry: &mut dyn ReadableRegistry,
) -> Result<UnfrozenReference, ModuleDeclarationError> {
  Ok(UnfrozenReference {
    id: resolve_type_id(name_or_id, registry).await?,
    version_req: VersionReq::parse("*").unwrap(),
  })
}

async fn unfrozen_reference_from_id(
  id: &Uuid,
) -> Result<UnfrozenReference, ModuleDeclarationError> {
  Ok(UnfrozenReference {
    id: id.to_owned(),
    version_req: VersionReq::parse("*").unwrap(),
  })
}

pub async fn resolve_high_parameter(
  parameter: HighParameter,
  registry: &mut dyn ReadableRegistry,
) -> Result<Parameter, ModuleDeclarationError> {
  Ok(Parameter {
    name: parameter.name,
    ty: resolve_high_type_ref(&parameter.ty, registry).await?,
    mutable: parameter.mutable,
  })
}

pub async fn resolve_low_parameter(
  parameter: LowParameter,
) -> Result<Parameter, ModuleDeclarationError> {
  Ok(Parameter {
    name: parameter.name,
    ty: resolve_low_type_ref(&parameter.ty).await?,
    mutable: parameter.mutable,
  })
}

pub async fn resolve_high_import(
  symbol: HighImportSymbol,
  registry: &mut dyn ReadableRegistry,
) -> Result<Export, ModuleDeclarationError> {
  Ok(match symbol {
    HighImportSymbol::Function(function) => {
      let mut parameters = HashMap::new();
      let mut parameter_ordering = Vec::new();
      for parameter in function.parameters {
        let parameter_id = parameter.id.to_owned();
        let resolved_parameter = resolve_high_parameter(parameter, registry).await?;
        parameters.insert(parameter_id.to_owned(), resolved_parameter);
        parameter_ordering.push(parameter_id);
      }
      Export {
        name: function.name,
        kind: semio_record::module::v0::unfrozen::ExportKind::Function(Function {
          parameters,
          return_ty: resolve_high_type_ref(&function.ret, registry).await?,
          parameter_ordering,
        }),
      }
    }
  })
}

pub async fn resolve_low_import(symbol: LowImportSymbol) -> Result<Export, ModuleDeclarationError> {
  Ok(match symbol {
    LowImportSymbol::Function(function) => {
      let mut parameters = HashMap::new();
      let mut parameter_ordering = Vec::new();
      for parameter in function.parameters {
        let parameter_id = parameter.id.to_owned();
        let resolved_parameter = resolve_low_parameter(parameter).await?;
        parameters.insert(parameter_id.to_owned(), resolved_parameter);
        parameter_ordering.push(parameter_id);
      }
      Export {
        name: function.name,
        kind: semio_record::module::v0::unfrozen::ExportKind::Function(Function {
          parameters,
          return_ty: resolve_low_type_ref(&function.ret).await?,
          parameter_ordering,
        }),
      }
    }
  })
}

pub async fn resolve_high_export(
  symbol: HighExportSymbol,
  registry: &mut dyn ReadableRegistry,
) -> Result<Export, ModuleDeclarationError> {
  Ok(match symbol {
    HighExportSymbol::Function(function) => {
      let mut parameters = HashMap::new();
      let mut parameter_ordering = Vec::new();
      for parameter in function.parameters {
        let parameter_id = parameter.id.to_owned();
        let resolved_parameter = resolve_high_parameter(parameter, registry).await?;
        parameters.insert(parameter_id.to_owned(), resolved_parameter);
        parameter_ordering.push(parameter_id);
      }
      Export {
        name: function.name,
        kind: semio_record::module::v0::unfrozen::ExportKind::Function(Function {
          parameters,
          return_ty: resolve_high_type_ref(&function.ret, registry).await?,
          parameter_ordering,
        }),
      }
    }
  })
}

pub async fn resolve_low_export(symbol: LowExportSymbol) -> Result<Export, ModuleDeclarationError> {
  Ok(match symbol {
    LowExportSymbol::Function(function) => {
      let mut parameters = HashMap::new();
      let mut parameter_ordering = Vec::new();
      for parameter in function.parameters {
        let parameter_id = parameter.id.to_owned();
        let resolved_parameter = resolve_low_parameter(parameter).await?;
        parameters.insert(parameter_id.to_owned(), resolved_parameter);
        parameter_ordering.push(parameter_id);
      }
      Export {
        name: function.name,
        kind: semio_record::module::v0::unfrozen::ExportKind::Function(Function {
          parameters,
          return_ty: resolve_low_type_ref(&function.ret).await?,
          parameter_ordering,
        }),
      }
    }
  })
}

pub async fn resolve_high_module(
  module_definition: HighModuleDefinition,
  registry: &mut dyn ReadableRegistry,
) -> Result<ModuleAndImports, ModuleDeclarationError> {
  let mut dependencies = HashSet::new();

  let mut imports = Vec::new();
  for import in module_definition.imports {
    let HighImportSymbol::Function(import_function) = import.clone();
    let import_module_id = resolve_module_id(import_function.module.as_str(), registry).await?;
    let import_module = registry
      .get_module(&Selector::Id(import_module_id.clone()))
      .await
      .map_err(ModuleDeclarationError::RegistryError)?;
    dependencies.insert(UnfrozenReference {
      id: import_module_id,
      version_req: VersionReq::parse("*").unwrap(),
    });
    let import_id = import_function.id.to_owned();
    let resolved_import = resolve_high_import(import, registry).await?;
    let mut import_deps = HashSet::new();
    resolved_import.dependencies(&mut import_deps);
    dependencies.extend(import_deps.into_iter().cloned());
    imports.push(ImportAsset {
      module_id: import_module_id,
      module_name: import_module.name.clone(),
      id: import_id,
      import: resolved_import,
    });
  }

  let mut exports = HashMap::new();
  for export in module_definition.exports {
    let HighExportSymbol::Function(export_function) = export.clone();
    let resolved_export = resolve_high_export(export, registry).await?;
    let mut export_deps = HashSet::new();
    resolved_export.dependencies(&mut export_deps);
    dependencies.extend(export_deps.into_iter().cloned());
    exports.insert(export_function.id, resolved_export);
  }

  Ok(ModuleAndImports {
    module: ModulePublic {
      parent: ROOT_ID.to_owned(),
      name: module_definition.name,
      executable: None,
      exports,
      dependencies: dependencies.into_iter().collect(),
    },
    imports,
  })
}

pub async fn resolve_low_module(
  module_header: Header,
  registry: &mut dyn ReadableRegistry,
) -> Result<ModuleAndImports, ModuleDeclarationError> {
  let mut dependencies = HashSet::new();

  let mut imports = Vec::new();
  for import in module_header.imports {
    let LowImportSymbol::Function(import_function) = import.clone();
    let import_module_id = import_function.module;
    let import_module = registry
      .get_module(&Selector::Id(import_module_id.clone()))
      .await
      .map_err(ModuleDeclarationError::RegistryError)?;
    dependencies.insert(UnfrozenReference {
      id: import_module_id,
      version_req: VersionReq::parse("*").unwrap(),
    });
    let import_id = import_function.id.to_owned();
    let resolved_import = resolve_low_import(import).await?;
    let mut import_deps = HashSet::new();
    resolved_import.dependencies(&mut import_deps);
    dependencies.extend(import_deps.into_iter().cloned());
    imports.push(ImportAsset {
      module_id: import_module_id,
      module_name: import_module.name.clone(),
      id: import_id,
      import: resolved_import,
    });
  }

  let mut exports = HashMap::new();
  for export in module_header.exports {
    let LowExportSymbol::Function(export_function) = export.clone();
    let resolved_export = resolve_low_export(export).await?;
    let mut export_deps = HashSet::new();
    resolved_export.dependencies(&mut export_deps);
    dependencies.extend(export_deps.into_iter().cloned());
    exports.insert(export_function.id, resolved_export);
  }

  Ok(ModuleAndImports {
    module: ModulePublic {
      parent: ROOT_ID.to_owned(),
      name: module_header.name.to_case(Case::Snake),
      executable: None,
      exports,
      dependencies: dependencies.into_iter().collect(),
    },
    imports,
  })
}

pub struct ModuleAndImports {
  pub module: ModulePublic,
  pub imports: Vec<ImportAsset>,
}

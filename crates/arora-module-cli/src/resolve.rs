use arora_registry::Registry;
use arora_schema::module::{
  low::{
    Header as LowHeader,
    ImportSymbol as LowImportSymbol,
    ImportFunction as LowImportFunction,
    ExportSymbol as LowExportSymbol,
    ExportFunction as LowExportFunction,
    Parameter as LowParameter,
    Executor as LowExecutor,
    TypeRef as LowTypeRef,
  },
  high::{
    ModuleDefinition as HighModuleDefinition,
    ImportSymbol as HighImportSymbol,
    ImportFunction as HighImportFunction,
    ExportSymbol as HighExportSymbol,
    ExportFunction as HighExportFunction,
    Parameter as HighParameter,
    Executor as HighExecutor,
    TypeRef as HighTypeRef,
  }
};
use uuid::Uuid;

pub async fn resolve_type_id(name: &str, registry: &mut Registry) -> anyhow::Result<Uuid> {
  Ok(match Uuid::parse_str(name) {
    Ok(id) => id,
    Err(_) => registry.lookup_type(name).await?
  })
}

pub async fn resolve_module_id(name: &str, registry: &mut Registry) -> anyhow::Result<Uuid> {
  Ok(match Uuid::parse_str(name) {
    Ok(id) => id,
    Err(_) => registry.lookup_module(name).await?
  })
}

pub async fn resolve_type_ref(type_ref: &HighTypeRef, registry: &mut Registry) -> anyhow::Result<LowTypeRef> {
  match type_ref {
    HighTypeRef::Scalar { id } => Ok(LowTypeRef::Scalar { id: resolve_type_id(id, registry).await? }),
    HighTypeRef::Array { id } => Ok(LowTypeRef::Array { id: resolve_type_id(id, registry).await? }),
    _ => Err(anyhow::anyhow!("Unsupported type ref: {:?}", type_ref))
  }
}

pub async fn resolve_parameter(parameter: HighParameter, registry: &mut Registry) -> anyhow::Result<LowParameter> {
  Ok(LowParameter {
    id: parameter.id,
    name: parameter.name,
    ty: resolve_type_ref(&parameter.ty, registry).await?,
    mutable: parameter.mutable,
  })
}

pub async fn resolve_import_symbol(symbol: HighImportSymbol, registry: &mut Registry) -> anyhow::Result<LowImportSymbol> {
  Ok(match symbol {
    HighImportSymbol::Function(function) => {
      let mut parameters = Vec::new();
      for parameter in function.parameters {
        parameters.push(resolve_parameter(parameter, registry).await?);
      }
      
      LowImportSymbol::Function(LowImportFunction {
        module: resolve_module_id(&function.module, registry).await?,
        id: function.id,
        name: function.name,
        parameters,
        ret: resolve_type_ref(&function.ret, registry).await?,
      })
    }
  })
}

pub async fn resolve_export_symbol(symbol: HighExportSymbol, registry: &mut Registry) -> anyhow::Result<LowExportSymbol> {
  Ok(match symbol {
    HighExportSymbol::Function(function) => {
      let mut parameters = Vec::new();
      for parameter in function.parameters {
        parameters.push(resolve_parameter(parameter, registry).await?);
      }
      
      LowExportSymbol::Function(LowExportFunction {
        id: function.id,
        name: function.name,
        parameters,
        ret: resolve_type_ref(&function.ret, registry).await?,
      })
    }
  })
}

pub async fn resolve_module_header(module_definition: HighModuleDefinition, registry: &mut Registry) -> anyhow::Result<LowHeader> {
  let mut imports = Vec::new();
  for import in module_definition.imports {
    imports.push(resolve_import_symbol(import, registry).await?);
  }

  let mut exports = Vec::new();
  for export in module_definition.exports {
    exports.push(resolve_export_symbol(export, registry).await?);
  }
  
  Ok(LowHeader {
    name: module_definition.name,
    version: module_definition.version,
    author: module_definition.author,
    description: module_definition.description,
    executable_mime: module_definition.executable_mime,
    executor: LowExecutor {
      name: module_definition.executor.name,
      min_version: module_definition.executor.min_version,
      max_version: module_definition.executor.max_version,
    },
    imports,
    exports,
    id: module_definition.id,
    license: module_definition.license,
  })
}
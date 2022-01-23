use arora_registry::Registry;
use arora_schema::module::{
  low::{
    Header as LowHeader,
    ImportSymbol as LowImportSymbol,
    ImportFunction as LowImportFunction,
    ImportNode as LowImportNode,
    ExportSymbol as LowExportSymbol,
    ExportFunction as LowExportFunction,
    ExportNode as LowExportNode,
    Parameter as LowParameter,
    Executor as LowExecutor,
  },
  high::{
    ModuleDefinition as HighModuleDefinition,
    ImportSymbol as HighImportSymbol,
    ImportFunction as HighImportFunction,
    ImportNode as HighImportNode,
    ExportSymbol as HighExportSymbol,
    ExportFunction as HighExportFunction,
    ExportNode as HighExportNode,
    Parameter as HighParameter,
    Executor as HighExecutor,
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

pub async fn resolve_parameter(parameter: HighParameter, registry: &mut Registry) -> anyhow::Result<LowParameter> {
  Ok(LowParameter {
    id: parameter.id,
    name: parameter.name,
    ty_id: resolve_type_id(&parameter.ty, registry).await?,
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
        ret: resolve_type_id(&function.ret, registry).await?,
      })
    },
    HighImportSymbol::Node(node) => {
      let mut parameters = Vec::new();
      for parameter in node.parameters {
        parameters.push(resolve_parameter(parameter, registry).await?);
      }
      
      LowImportSymbol::Node(LowImportNode {
        module: resolve_module_id(&node.module, registry).await?,
        id: node.id,
        name: node.name,
        parameters
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
        ret: resolve_type_id(&function.ret, registry).await?,
      })
    },
    HighExportSymbol::Node(node) => {
      let mut parameters = Vec::new();
      for parameter in node.parameters {
        parameters.push(resolve_parameter(parameter, registry).await?);
      }
      
      LowExportSymbol::Node(LowExportNode {
        id: node.id,
        name: node.name,
        parameters
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
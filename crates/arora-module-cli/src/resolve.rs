use arora_registry::Registry;
use arora_schema::module::{
  low::{
    Header as LowHeader,
    Dependency as LowDependency,
    Symbol as LowSymbol,
    Function as LowFunction,
    Node as LowNode,
    Parameter as LowParameter,
    Executor as LowExecutor,
  },
  high::{
    ModuleDefinition as HighModuleDefinition,
    Dependency as HighDependency,
    Symbol as HighSymbol,
    Function as HighFunction,
    Node as HighNode,
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

pub async fn resolve_dependency(dependency: HighDependency, registry: &mut Registry) -> anyhow::Result<LowDependency> {
  Ok(LowDependency {
    id: resolve_module_id(&dependency.name, registry).await?,
    min_version: dependency.min_version,
    max_version: dependency.max_version,
  })
}

pub async fn resolve_symbol(symbol: HighSymbol, registry: &mut Registry) -> anyhow::Result<LowSymbol> {
  Ok(match symbol {
    HighSymbol::Function(function) => {
      let mut parameters = Vec::new();
      for parameter in function.parameters {
        parameters.push(resolve_parameter(parameter, registry).await?);
      }
      
      LowSymbol::Function(LowFunction {
        id: function.id,
        name: function.name,
        parameters,
        ret: resolve_type_id(&function.ret, registry).await?,
      })
    },
    HighSymbol::Node(node) => {
      let mut parameters = Vec::new();
      for parameter in node.parameters {
        parameters.push(resolve_parameter(parameter, registry).await?);
      }
      
      LowSymbol::Node(LowNode {
        id: node.id,
        name: node.name,
        parameters
      })
    }
  })
}

pub async fn resolve_module_header(module_definition: HighModuleDefinition, registry: &mut Registry) -> anyhow::Result<LowHeader> {
  let mut dependencies = Vec::new();
  for dependency in module_definition.dependencies {
    dependencies.push(resolve_dependency(dependency, registry).await?);
  }

  let mut imports = Vec::new();
  for import in module_definition.imports {
    imports.push(resolve_symbol(import, registry).await?);
  }

  let mut exports = Vec::new();
  for export in module_definition.exports {
    exports.push(resolve_symbol(export, registry).await?);
  }
  
  Ok(LowHeader {
    name: module_definition.name,
    version: module_definition.version,
    author: module_definition.author,
    description: module_definition.description,
    dependencies,
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
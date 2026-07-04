//! The `module` record: a named set of exported functions plus the executable
//! blob and record dependencies it needs. Unfrozen = dependencies carry
//! version requirements; frozen = pinned (the record wire format).

use async_trait::async_trait;
use std::collections::HashMap;

use super::freeze::{Freeze, Resolver};

/// Builder (unfrozen) forms: what a module declaration produces.
pub mod unfrozen {
  use super::super::reference::UnfrozenReference;
  use super::super::ty::UnfrozenTy;
  use serde::{Deserialize, Serialize};
  use std::collections::HashMap;
  use uuid::Uuid;

  #[derive(Debug, Serialize, Deserialize, Clone)]
  #[serde(rename = "module_V0_Parameter", rename_all = "camelCase")]
  pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: UnfrozenTy,
    pub mutable: bool,
  }

  #[derive(Debug, Serialize, Deserialize, Clone)]
  #[serde(rename = "module_V0_Function", rename_all = "camelCase")]
  pub struct Function {
    pub parameters: HashMap<Uuid, Parameter>,
    pub parameter_ordering: Vec<Uuid>,
    #[serde(rename = "returnType")]
    pub return_ty: UnfrozenTy,
  }

  impl Function {
    pub fn parameter_id(&self, name: &str) -> Option<&Uuid> {
      self
        .parameters
        .iter()
        .find(|(_, parameter)| parameter.name == *name)
        .map(|(id, _)| id)
    }

    pub fn parameter(&self, id: &Uuid) -> Option<&Parameter> {
      self.parameters.get(id)
    }

    pub fn parameter_named(&self, name: &str) -> Option<&Parameter> {
      self.parameter(self.parameter_id(name)?)
    }

    pub fn append_parameter(&mut self, id: Uuid, parameter: Parameter) {
      self.parameters.insert(id, parameter);
      self.parameter_ordering.push(id);
    }
  }

  /// What a module exports.
  ///
  /// `function` is serialized as `func` (a reserved keyword in JS/TS).
  #[derive(Debug, Serialize, Deserialize, Clone)]
  #[serde(
    rename = "module_V0_Export_Kind",
    tag = "type",
    rename_all = "camelCase",
    content = "value"
  )]
  pub enum ExportKind {
    #[serde(rename = "func")]
    Function(Function),
  }

  impl ExportKind {
    pub fn as_function(&self) -> Option<&Function> {
      match self {
        ExportKind::Function(function) => Some(function),
      }
    }

    pub fn to_function(self) -> Option<Function> {
      match self {
        ExportKind::Function(function) => Some(function),
      }
    }
  }

  #[derive(Debug, Serialize, Deserialize, Clone)]
  #[serde(rename = "module_V0_Export", rename_all = "camelCase")]
  pub struct Export {
    pub name: String,
    pub kind: ExportKind,
  }

  #[derive(Debug, Serialize, Deserialize, Clone, Default)]
  #[serde(rename = "module_V0_Private", rename_all = "camelCase")]
  pub struct Module {
    pub parent: Uuid,
    pub name: String,
    pub exports: HashMap<Uuid, Export>,
    pub executable: Option<Uuid>,
    pub dependencies: Vec<UnfrozenReference>,
  }

  impl Module {
    pub fn export_id(&self, name: &str) -> Option<&Uuid> {
      self
        .exports
        .iter()
        .find(|(_, export)| export.name == *name)
        .map(|(id, _)| id)
    }

    pub fn export(&self, id: &Uuid) -> Option<&Export> {
      self.exports.get(id)
    }

    pub fn export_named(&self, name: &str) -> Option<&Export> {
      self.exports.get(self.export_id(name)?)
    }

    pub fn add_export(&mut self, id: Uuid, export: Export) {
      self.exports.insert(id, export);
    }
  }
}

/// Frozen (version-pinned) forms — the record wire format.
pub mod frozen {
  use super::super::reference::FrozenReference;
  use super::super::ty::FrozenTy;
  use serde::{Deserialize, Serialize};
  use std::collections::HashMap;
  use uuid::Uuid;

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "module_V0_Frozen_Parameter", rename_all = "camelCase")]
  pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: FrozenTy,
    pub mutable: bool,
  }

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "module_V0_Frozen_Function", rename_all = "camelCase")]
  pub struct Function {
    pub parameters: HashMap<Uuid, Parameter>,
    pub parameter_ordering: Vec<Uuid>,
    #[serde(rename = "returnType")]
    pub return_ty: FrozenTy,
  }

  impl Function {
    pub fn parameter_id(&self, name: &str) -> Option<&Uuid> {
      self
        .parameters
        .iter()
        .find(|(_, parameter)| parameter.name == *name)
        .map(|(id, _)| id)
    }

    pub fn parameter(&self, id: &Uuid) -> Option<&Parameter> {
      self.parameters.get(id)
    }

    pub fn parameter_named(&self, name: &str) -> Option<&Parameter> {
      self.parameter(self.parameter_id(name)?)
    }

    /// Parameters in declared order.
    pub fn ordered_parameters(&self) -> impl Iterator<Item = &Parameter> {
      self
        .parameter_ordering
        .iter()
        .filter_map(|id| self.parameters.get(id))
    }
  }

  /// What a module exports.
  ///
  /// `function` is serialized as `func` (a reserved keyword in JS/TS).
  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(
    rename = "module_V0_Frozen_Export_Kind",
    tag = "type",
    rename_all = "camelCase",
    content = "value"
  )]
  pub enum ExportKind {
    #[serde(rename = "func")]
    Function(Function),
  }

  impl ExportKind {
    pub fn as_function(&self) -> Option<&Function> {
      match self {
        ExportKind::Function(function) => Some(function),
      }
    }
  }

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "module_V0_Frozen_Export", rename_all = "camelCase")]
  pub struct Export {
    pub name: String,
    pub kind: ExportKind,
  }

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "module_V0_Frozen", rename_all = "camelCase")]
  pub struct Module {
    pub parent: Uuid,
    pub name: String,
    pub exports: HashMap<Uuid, Export>,
    pub executable: Option<Uuid>,
    pub dependencies: Vec<FrozenReference>,
  }

  impl Module {
    pub fn export_id(&self, name: &str) -> Option<&Uuid> {
      self
        .exports
        .iter()
        .find(|(_, export)| export.name == *name)
        .map(|(id, _)| id)
    }

    pub fn export(&self, id: &Uuid) -> Option<&Export> {
      self.exports.get(id)
    }

    pub fn export_named(&self, name: &str) -> Option<&Export> {
      self.exports.get(self.export_id(name)?)
    }
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for unfrozen::Parameter {
  type Frozen = frozen::Parameter;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(frozen::Parameter {
      name: self.name.clone(),
      ty: self.ty.freeze(resolver).await?,
      mutable: self.mutable,
    })
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for unfrozen::Function {
  type Frozen = frozen::Function;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(frozen::Function {
      parameters: self.parameters.freeze(resolver).await?,
      parameter_ordering: self.parameter_ordering.clone(),
      return_ty: self.return_ty.freeze(resolver).await?,
    })
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for unfrozen::ExportKind {
  type Frozen = frozen::ExportKind;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    match self {
      unfrozen::ExportKind::Function(function) => Ok(frozen::ExportKind::Function(
        function.freeze(resolver).await?,
      )),
    }
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for unfrozen::Export {
  type Frozen = frozen::Export;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(frozen::Export {
      name: self.name.clone(),
      kind: self.kind.freeze(resolver).await?,
    })
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for unfrozen::Module {
  type Frozen = frozen::Module;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    let mut exports = HashMap::with_capacity(self.exports.len());
    for (id, export) in &self.exports {
      exports.insert(*id, export.freeze(resolver).await?);
    }
    Ok(frozen::Module {
      parent: self.parent,
      name: self.name.clone(),
      exports,
      executable: self.executable,
      dependencies: self.dependencies.freeze(resolver).await?,
    })
  }
}

/// Dependency discovery: what record references a declaration needs resolved
/// before it can be frozen.
mod dependencies {
  use super::super::reference::UnfrozenReference;
  use super::unfrozen::{Export, ExportKind, Function, Module, Parameter};
  use std::collections::HashSet;

  impl Parameter {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a UnfrozenReference>) {
      self.ty.dependencies(set);
    }
  }

  impl Function {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a UnfrozenReference>) {
      for parameter in self.parameters.values() {
        parameter.dependencies(set);
      }
      self.return_ty.dependencies(set);
    }
  }

  impl ExportKind {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a UnfrozenReference>) {
      match self {
        Self::Function(function) => function.dependencies(set),
      }
    }
  }

  impl Export {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a UnfrozenReference>) {
      self.kind.dependencies(set);
    }
  }

  impl Module {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a UnfrozenReference>) {
      for export in self.exports.values() {
        export.dependencies(set);
      }
      for dependency in &self.dependencies {
        set.insert(dependency);
      }
    }
  }
}

/// Frozen-side dependency discovery: which pinned records a frozen module
/// (transitively) references.
mod frozen_dependencies {
  use super::super::reference::FrozenReference;
  use super::frozen::{Export, ExportKind, Function, Module, Parameter};
  use std::collections::HashSet;

  impl Parameter {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a FrozenReference>) {
      self.ty.dependencies(set);
    }
  }

  impl Function {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a FrozenReference>) {
      for parameter in self.parameters.values() {
        parameter.dependencies(set);
      }
      self.return_ty.dependencies(set);
    }
  }

  impl ExportKind {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a FrozenReference>) {
      match self {
        Self::Function(function) => function.dependencies(set),
      }
    }
  }

  impl Export {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a FrozenReference>) {
      self.kind.dependencies(set);
    }
  }

  impl Module {
    pub fn dependencies<'a>(&'a self, set: &mut HashSet<&'a FrozenReference>) {
      for export in self.exports.values() {
        export.dependencies(set);
      }
      for dependency in &self.dependencies {
        set.insert(dependency);
      }
    }
  }
}

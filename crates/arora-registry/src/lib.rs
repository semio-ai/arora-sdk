use std::collections::HashMap;

use arora_schema::{
  module::low::{Header, ModuleDefinition},
  ty::{low::Type, PRIMITIVE_TYPES},
};
use derive_more::Display;
use tokio::{
  fs::{read_to_string, File},
  io::AsyncReadExt,
};
use url::Url;
use uuid::Uuid;

const BASE_URL: &'static str = "https://raw.githubusercontent.com/semio-ai/arora-registry/master/";

pub struct Registry {
  base_uri: Url,
  type_id_cache: HashMap<String, Uuid>,
}

impl Registry {
  pub fn new() -> Self {
    Registry {
      base_uri: Url::parse(BASE_URL).unwrap(),
      type_id_cache: Registry::new_type_id_cache_with_primitives(),
    }
  }

  pub fn new_with_base_uri(base_uri: Url) -> Self {
    Registry {
      base_uri,
      type_id_cache: Registry::new_type_id_cache_with_primitives(),
    }
  }

  fn new_type_id_cache_with_primitives() -> HashMap<String, Uuid> {
    let mut type_id_cache = HashMap::new();
    PRIMITIVE_TYPES.iter().for_each(|(id, ty)| {
      type_id_cache.insert(ty.name.clone(), id.clone());
      ();
    });
    type_id_cache
  }

  async fn get_bytes(url: Url) -> anyhow::Result<Box<[u8]>> {
    if url.scheme() == "file" {
      let mut file = File::open(url.path()).await?;
      let mut data = Vec::new();
      file.read_to_end(&mut data).await?;
      Ok(data.into_boxed_slice())
    } else {
      Ok(
        reqwest::get(url)
          .await?
          .bytes()
          .await?
          .to_vec()
          .into_boxed_slice(),
      )
    }
  }

  async fn get_text(url: Url) -> anyhow::Result<String> {
    if url.scheme() == "file" {
      let path = if cfg!(windows) {
        &url.path()[1..]
      } else {
        url.path()
      };
      eprintln!("FILE URI {}", path);
      Ok(read_to_string(path).await?)
    } else {
      Ok(reqwest::get(url).await?.text().await?)
    }
  }

  pub async fn get_type(&self, id: &Uuid) -> anyhow::Result<Type> {
    let uri = self.base_uri.join(&format!("types/by-uuid/{id}.yaml"))?;
    let ret: Type = serde_yaml::from_str(&Self::get_text(uri.clone()).await?).map_err(|e| {
      RegistryError::ParsingError {
        message: format!("error parsing type info from {}: {}", uri, e),
      }
    })?;
    Ok(ret)
  }

  pub async fn lookup_type(&mut self, name: &str) -> anyhow::Result<Uuid> {
    if let Some(id) = self.type_id_cache.get(name) {
      return Ok(id.clone());
    }

    let uri = self.base_uri.join(&format!("types/by-name/{name}"))?;
    let text = Self::get_text(uri).await?;
    let id = Uuid::parse_str(&text)?;
    self.type_id_cache.insert(name.to_string(), id.clone());
    Ok(id)
  }

  pub async fn get_module_header(&self, id: &Uuid) -> anyhow::Result<Header> {
    let uri = self
      .base_uri
      .join(&format!("modules/by-uuid/{id}/header.yaml"))?;
    let text = Self::get_text(uri.clone()).await?;
    let header: Header = serde_yaml::from_str(&text).map_err(|e| RegistryError::ParsingError {
      message: format!("error parsing module info from {}: {}", uri, e),
    })?;
    Ok(header)
  }

  pub async fn get_module(&self, id: &Uuid) -> anyhow::Result<ModuleDefinition> {
    let header = self.get_module_header(id).await?;

    let uri = self
      .base_uri
      .join(&format!("modules/by-uuid/{id}/executable.bin"))?;
    let executable = Self::get_bytes(uri).await?;

    Ok(ModuleDefinition {
      schema_version: 0,
      header,
      executable,
    })
  }

  pub async fn lookup_module(&self, name: &str) -> anyhow::Result<Uuid> {
    let uri = self
      .base_uri
      .join(&format!("{BASE_URL}/modules/by-name/{name}"))?;
    Ok(Uuid::parse_str(&Self::get_text(uri).await?)?)
  }
}

#[derive(Display, Debug)]
pub enum RegistryError {
  /// Error when parsing something, such as a behavior tree description.
  #[display(fmt = "parsing error: {}", message)]
  ParsingError { message: String },

  /// For any other error.
  #[display(fmt = "error: {}", message)]
  Generic { message: String },
}

impl std::error::Error for RegistryError {}

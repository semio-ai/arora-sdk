
use engine_schema::module::low::{Header, ModuleDefinition};
use tokio::{fs::{read_to_string, File}, io::AsyncReadExt};
use uuid::Uuid;
use url::Url;

const BASE_URL: &'static str = "https://github.com/semio-ai/engine-registry/blob/master/";

pub struct Registry {
  base_uri: Url,
}

impl Registry {
  pub fn new() -> Self {
    Registry {
      base_uri: Url::parse(BASE_URL).unwrap(),
    }
  }

  pub fn new_with_base_uri(base_uri: Url) -> Self {
    Registry {
      base_uri
    }
  }

  async fn get_bytes(url: Url) -> anyhow::Result<Box<[u8]>> {
    if url.scheme() == "file" {
      let mut file = File::open(url.path()).await?;
      let mut data = Vec::new();
      file.read_to_end(&mut data).await?;
      Ok(data.into_boxed_slice())
    } else {
      Ok(reqwest::get(url).await?.bytes().await?.to_vec().into_boxed_slice())
    }
  }

  async fn get_text(url: Url) -> anyhow::Result<String> {
    if url.scheme() == "file" {
      Ok(read_to_string(url.path()).await?)
    } else {
      Ok(reqwest::get(url).await?.text().await?)
    }
  }

  pub async fn get_type(&self, id: Uuid) -> anyhow::Result<String> {
    let uri = self.base_uri.join(&format!("types/by-uuid/{id}.yaml"))?;
    Ok(Self::get_text(uri).await?)
  }

  pub async fn lookup_type(&self, name: &str) -> anyhow::Result<Uuid> {
    let uri = self.base_uri.join(&format!("types/by-name/{name}"))?;
    println!("{} - {}", self.base_uri, uri);
    Ok(Uuid::parse_str(&Self::get_text(uri).await?)?)
  }

  pub async fn get_module_header(&self, id: Uuid) -> anyhow::Result<Header> {
    let uri = self.base_uri.join(&format!("modules/by-uuid/{id}/header.yaml"))?;
    let text = Self::get_text(uri).await?;
    let header: Header = serde_yaml::from_str(&text)?;
    Ok(header)
  }

  pub async fn get_module(&self, id: Uuid) -> anyhow::Result<ModuleDefinition> {
    let header = self.get_module_header(id).await?;

    let uri = self.base_uri.join(&format!("modules/by-uuid/{id}/executable.bin"))?;
    let executable = Self::get_bytes(uri).await?;

    Ok(ModuleDefinition {
      schema_version: 0,
      header,
      executable,
    })
  }

  pub async fn lookup_module(&self, name: &str) -> anyhow::Result<Uuid> {
    let uri = self.base_uri.join(&format!("{BASE_URL}/modules/by-name/{name}"))?;
    Ok(Uuid::parse_str(&Self::get_text(uri).await?)?)
  }
}

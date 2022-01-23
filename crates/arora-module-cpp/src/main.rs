use std::path::Path;

use arora_module_core::{AssetReader, Args, Parser, Asset};

use arora_schema::ty::low::Type;
use tokio::io::{stdin, AsyncWriteExt};

async fn write_file<P: AsRef<Path>>(path: P, content: &str) -> anyhow::Result<()> {
  let path = path.as_ref();
  tokio::fs::create_dir_all(path.parent().unwrap()).await?;
  let mut file = tokio::fs::File::create(path).await?;
  file.write_all(content.as_bytes()).await?;
  Ok(())
}

async fn write_type(ty: Type) -> anyhow::Result<()> {
  Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args = Args::parse();

  let mut stdin = stdin();
  let mut reader = AssetReader::new(&mut stdin);
  
  let mut assets = Vec::new();
  
  while let Ok(Some(asset)) = reader.read().await {
    assets.push(asset);
  }

  tokio::fs::create_dir_all(args.output_directory).await
    .map_err(|e| anyhow::anyhow!("Failed to create output directory: {}", e))?;

  for asset in assets {
    match asset {
      Asset::Type(ty) => {
        
      }, 
      Asset::ImportSymbol(symbol) => {

      },
      Asset::ExportSymbol(symbol) => {

      },
    }
  }

  

  Ok(())
}

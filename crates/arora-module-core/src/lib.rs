use arora_schema::{ty::low::Type, module::low::Symbol};
use serde::{Serialize, Deserialize};

use tokio::io::{AsyncWrite, AsyncWriteExt, AsyncRead, AsyncReadExt};
use bytes::{Buf, BufMut};
pub use clap::Parser;

#[derive(Debug, Serialize, Deserialize)]
pub enum Asset {
  Type(Type),
  ImportSymbol(Symbol),
  ExportSymbol(Symbol),
}

pub struct AssetWriter<'a, W: AsyncWrite + Unpin> {
  writer: &'a mut W
}

impl<'a, W: AsyncWrite + Unpin> AssetWriter<'a, W> {
  pub fn new(writer: &'a mut W) -> Self {
    Self {
      writer
    }
  }

  pub async fn write(&mut self, asset: Asset) -> tokio::io::Result<()> {
    let mut size = [0u8; 4];
    let serialized = serde_json::to_string(&asset).unwrap();
    (&mut size[..]).put_u32(serialized.len() as u32);
    self.writer.write_all(&size).await?;
    self.writer.write_all(serialized.as_bytes()).await?;
    println!("{:?}", asset);
    Ok(())
  }

  pub async fn end(mut self) -> tokio::io::Result<()> {
    let mut size = [0u8; 4];
    (&mut size[..]).put_u32(0);
    self.writer.write_all(&size).await?;
    Ok(())
  }
}

pub struct AssetReader<'a, R: AsyncRead + Unpin> {
  reader: &'a mut R
}

impl<'a, R: AsyncRead + Unpin> AssetReader<'a, R> {
  pub fn new(reader: &'a mut R) -> Self {
    Self {
      reader
    }
  }

  pub async fn read(&mut self) -> tokio::io::Result<Option<Asset>> {
    let mut size = [0u8; 4];
    self.reader.read_exact(&mut size).await?;
    let size = (&size[..]).get_u32() as usize;
    if size == 0 {
      return Ok(None);
    }

    let mut buf = vec![0u8; size];
    self.reader.read_exact(&mut buf).await?;
    let asset: Asset = serde_json::from_slice(&buf).unwrap();
    Ok(Some(asset))
  }
}

#[derive(Parser, Debug)]
#[clap(long_about = None)]
pub struct Args {
  #[clap(long)]
  pub output_directory: String,
}

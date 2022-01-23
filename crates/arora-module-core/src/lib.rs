use arora_schema::{ty::low::Type, module::low::{ImportSymbol, ExportSymbol, Header}};
use serde::{Serialize, Deserialize, de::DeserializeOwned};

use tokio::io::{AsyncWrite, AsyncWriteExt, AsyncRead, AsyncReadExt};
use bytes::{Buf, BufMut};

#[derive(Debug, Serialize, Deserialize)]
pub enum Asset {
  Type(Type),
  ImportSymbol(ImportSymbol),
  ExportSymbol(ExportSymbol),
  Header(Header),
}

pub struct Writer<'a, W: AsyncWrite + Unpin> {
  writer: &'a mut W,
}

impl<'a, W: AsyncWrite + Unpin> Writer<'a, W> {
  pub fn new(writer: &'a mut W) -> Self {
    Self {
      writer
    }
  }

  pub async fn write<T: Serialize>(&mut self, value: T) -> tokio::io::Result<()> {
    let mut size = [0u8; 4];
    let serialized = serde_json::to_string(&value).unwrap();
    (&mut size[..]).put_u32(serialized.len() as u32);
    self.writer.write_all(&size).await?;
    self.writer.write_all(serialized.as_bytes()).await?;
    Ok(())
  }

  pub async fn end(mut self) -> tokio::io::Result<()> {
    let mut size = [0u8; 4];
    (&mut size[..]).put_u32(0);
    self.writer.write_all(&size).await?;
    Ok(())
  }
}

pub struct Reader<'a, R: AsyncRead + Unpin> {
  reader: &'a mut R
}

impl<'a, R: AsyncRead + Unpin> Reader<'a, R> {
  pub fn new(reader: &'a mut R) -> Self {
    Self {
      reader
    }
  }

  pub async fn read<T: DeserializeOwned>(&mut self) -> tokio::io::Result<Option<T>> {
    let mut size = [0u8; 4];
    self.reader.read_exact(&mut size).await?;
    let size = (&size[..]).get_u32() as usize;
    if size == 0 {
      return Ok(None);
    }

    let mut buf = vec![0u8; size];
    self.reader.read_exact(&mut buf).await?;
    let value: T = serde_json::from_slice(&buf).unwrap();
    Ok(Some(value))
  }
}

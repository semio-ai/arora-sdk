use std::{collections::HashMap, sync::Arc, pin::Pin, future::Future, path::PathBuf};
use serde::{Serialize, Deserialize};
use tokio::{io, fs};
use io::{AsyncWriteExt, AsyncReadExt};
use sha2::{Sha256, Digest};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct File {
  pub data: Box<[u8]>,
}

impl File {
  pub fn new<T: AsRef<[u8]>>(data: T) -> Self {
    Self { data: data.as_ref().into() }
  }

  pub async fn sync(self: Arc<File>, path: PathBuf) -> io::Result<()> {
    let mut hasher = Sha256::new();
    hasher.update(&self.data);
    let hash = hasher.finalize();
    
    let real_hash = if path.exists() {
      let mut current = Vec::new();
      fs::File::open(&path).await?
        .read_to_end(&mut current).await?;
      let mut hasher = Sha256::new();
      hasher.update(&current);
      Some(hasher.finalize())
    } else {
      None
    };

    if real_hash == Some(hash) {
      return Ok(());
    }

    
    fs::File::create(&path).await?.write_all(&self.data).await?;
    Ok(())
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Directory {
  pub entries: HashMap<String, Arc<Entry>>,
}

impl Directory {
  pub fn new() -> Self {
    Self { entries: HashMap::new() }
  }

  pub fn get(&self, name: &str) -> Option<Arc<Entry>> {
    self.entries.get(name).cloned()
  }

  pub fn insert<N: AsRef<str>, E: Into<Entry>>(&mut self, name: N, entry: E) {
    self.entries.insert(name.as_ref().to_string(), entry.into().into());
  }

  pub fn remove(&mut self, name: &str) {
    self.entries.remove(name);
  }

  pub async fn sync(self: Arc<Directory>, path: PathBuf) -> io::Result<()> {
    if !path.exists() {
      fs::create_dir(&path).await?;
    }

    for (name, entry) in self.entries.iter() {
      let mut entry_path = path.clone();
      entry_path.push(name);
      entry.clone().sync(entry_path).await?;
    }
    Ok(())
  }

  pub fn merge_with(self: Arc<Directory>, other: Arc<Directory>) -> Arc<Directory> {
    // If they both have the same entry, recursively merge
    let mut entries = HashMap::new();
    for (name, entry) in self.entries.iter() {
      if let Some(other_entry) = other.entries.get(name) {
        match **entry {
          Entry::File(_) => {
            panic!("Merging files is unsupported");
          },
          Entry::Directory(ref directory) => {
            if let Entry::Directory(ref other_directory) = **other_entry {
              let directory = directory.clone();
              let other_directory = other_directory.clone();
              entries.insert(name.clone(), Entry::Directory(directory.merge_with(other_directory)).into());
            } else {
              panic!("Tried to merge a directory with a file");
            }
          },
        }
      } else {
        entries.insert(name.clone(), entry.clone());
      }
    }

    for (name, entry) in other.entries.iter() {
      if !self.entries.contains_key(name) {
        entries.insert(name.clone(), entry.clone());
      }
    }

    Arc::new(Directory { entries })
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Entry {
  File(Arc<File>),
  Directory(Arc<Directory>),
}

impl Entry {
  pub fn sync(self: &Arc<Entry>, path: PathBuf) -> Pin<Box<dyn Future<Output = io::Result<()>>>> {
    let this = self.clone();
    Box::pin(async move {
      match *this {
        Entry::File(ref file) => file.clone().sync(path).await,
        Entry::Directory(ref directory) => directory.clone().sync(path).await,
      }
    })
  }
}

impl From<File> for Entry {
  fn from(file: File) -> Self {
    Entry::File(Arc::new(file))
  }
}

impl From<Directory> for Entry {
  fn from(directory: Directory) -> Self {
    Entry::Directory(Arc::new(directory))
  }
}
use async_recursion::async_recursion;
use derive_more::Display;
use io::{AsyncReadExt, AsyncWriteExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{hash_map::Entry as HashMapEntry, HashMap},
    path::{Path, PathBuf},
};
use tokio::{fs, io};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct File {
    pub data: Box<[u8]>,
}

impl File {
    pub fn new<T: AsRef<[u8]>>(data: T) -> Self {
        Self {
            data: data.as_ref().into(),
        }
    }

    pub async fn sync(&self, path: PathBuf) -> io::Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(&self.data);
        let hash = hasher.finalize();

        let real_hash = if path.exists() {
            let mut current = Vec::new();
            fs::File::open(&path)
                .await?
                .read_to_end(&mut current)
                .await?;
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
    pub entries: HashMap<String, Entry>,
}

impl Default for Directory {
    fn default() -> Self {
        Self::new()
    }
}

impl Directory {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Entry> {
        self.entries.get_mut(name)
    }

    /// Looks up the entry at the given path.
    pub fn get_mut_at_path<P: AsRef<Path>>(&mut self, path: P) -> Option<&mut Entry> {
        let mut path_iter = path.as_ref().iter();
        let next_name = path_iter.next()?.to_str()?.to_string();
        let mut current_entry: &mut Entry = self.get_mut(&next_name)?;
        for entry_name in path_iter {
            let next_name = entry_name.to_str()?.to_string();
            current_entry = match current_entry {
                Entry::Directory(directory) => directory.get_mut(&next_name)?,
                Entry::File(_) => return None,
            };
        }
        Some(current_entry)
    }

    pub fn insert<N: AsRef<str>, E: Into<Entry>>(
        &mut self,
        name: N,
        entry: E,
    ) -> Result<&mut Entry, VfsError> {
        let name = name.as_ref().to_string();
        match self.entries.entry(name.clone()) {
            HashMapEntry::Occupied(_) => Err(VfsError::AlreadyExists(name)),
            HashMapEntry::Vacant(map_entry) => Ok(map_entry.insert(entry.into())),
        }
    }

    /// Inserts an entry at the given path.
    /// Entry at parent path must already exist and be a directory.
    pub fn insert_at_path<P: AsRef<Path>, E: Into<Entry>>(
        &mut self,
        path: P,
        entry: E,
    ) -> Result<&mut Entry, VfsError> {
        let path = path.as_ref();
        let file_name = Self::os_str_to_string(
            path.file_name()
                .ok_or(VfsError::Generic("no file name".to_string()))?,
        )?;
        if let Some(parent) = path.parent() {
            if let Entry::Directory(parent_dir) = self
                .get_mut_at_path(parent)
                .ok_or(VfsError::NotFound(path.display().to_string()))?
            {
                parent_dir.insert(file_name, entry)
            } else {
                Err(VfsError::Generic(format!(
                    "parent of path {} is not a directory",
                    path.display()
                )))
            }
        } else {
            self.insert(file_name, entry)
        }
    }

    /// Gets the entry with the given name, or creates a new one using the given function,
    /// inserts it and returns it.
    pub fn get_mut_or_insert<N: Into<String>, E: Into<Entry>, F: FnOnce() -> E>(
        &mut self,
        name: N,
        default: F,
    ) -> &mut Entry {
        self.entries.entry(name.into()).or_insert(default().into())
    }

    /// Removes the entry with the given name, if present.
    pub fn remove(&mut self, name: &str) {
        self.entries.remove(name);
    }

    /// Creates a directory at the given path,
    /// including all of its parent directories.
    /// Fails if the path is empty, invalid,
    /// or if any non-file entry exists with the same name.
    pub fn ensure_directories(&mut self, path: &Path) -> Result<&mut Directory, VfsError> {
        let mut path_iter = path.iter();
        let next_name = Self::os_str_to_string(path_iter.next().ok_or(VfsError::EmptyPath)?)?;
        let mut current_entry: &mut Entry = self.get_mut_or_insert(next_name, Entry::new_directory);
        for entry_name in path_iter {
            let next_name = Self::os_str_to_string(entry_name)?;
            current_entry = match current_entry {
                Entry::Directory(directory) => {
                    directory.get_mut_or_insert(next_name, Entry::new_directory)
                }
                Entry::File(_) => return Err(VfsError::AlreadyExists(next_name.clone())),
            };
        }
        Ok(current_entry
            .as_directory()
            .expect("inserted directory is not a directory after insertion!"))
    }

    /// Lists entries in the directory.
    pub fn list_mut(&mut self) -> Vec<(String, &mut Entry)> {
        self.entries
            .iter_mut()
            .map(|(name, entry)| (name.clone(), entry))
            .collect()
    }

    /// Lists every entry under this directory, recursively.
    pub fn list_all_mut(&mut self) -> Vec<(String, &mut Entry)> {
        self.list_all_recurse(PathBuf::new())
    }

    fn list_all_recurse(&mut self, parent_path: PathBuf) -> Vec<(String, &mut Entry)> {
        self.entries
            .iter_mut()
            .flat_map(|(name, entry)| {
                let path = parent_path.join(name);
                match entry {
                    Entry::Directory(directory) => directory.list_all_recurse(path),
                    _ => vec![(path.display().to_string(), entry)],
                }
            })
            .collect()
    }

    fn os_str_to_string(os_str: &std::ffi::OsStr) -> Result<String, VfsError> {
        os_str
            .to_str()
            .ok_or(VfsError::InvalidPath)
            .map(|s| s.to_string())
    }

    /// Writes the directory to the given path.
    pub async fn sync(&self, path: PathBuf) -> io::Result<()> {
        if !path.exists() {
            fs::create_dir_all(&path).await?;
        } else if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path {} is not a directory", path.display()),
            ));
        }

        for (name, entry) in self.entries.iter() {
            let mut entry_path = path.clone();
            entry_path.push(name);
            entry.sync(entry_path).await.map_err(|err| {
                io::Error::new(
                    err.kind(),
                    format!("failed to sync entry {}: {}", name, err),
                )
            })?;
        }
        Ok(())
    }

    pub fn merge_with(&self, other: &Directory) -> Directory {
        // If they both have the same entry, recursively merge
        let mut entries = HashMap::new();
        for (name, entry) in self.entries.iter() {
            if let Some(other_entry) = other.entries.get(name) {
                match *entry {
                    Entry::File(_) => {
                        panic!("Merging files is unsupported");
                    }
                    Entry::Directory(ref directory) => {
                        if let Entry::Directory(ref other_directory) = *other_entry {
                            let directory = directory.clone();
                            let other_directory = other_directory.clone();
                            entries.insert(
                                name.clone(),
                                Entry::Directory(directory.merge_with(&other_directory)),
                            );
                        } else {
                            panic!("Tried to merge a directory with a file");
                        }
                    }
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

        Directory { entries }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Entry {
    File(File),
    Directory(Directory),
}

impl Entry {
    pub fn new_file(data: Box<[u8]>) -> Self {
        Entry::File(File { data })
    }

    pub fn new_directory() -> Self {
        Entry::Directory(Directory::new())
    }

    #[async_recursion]
    pub async fn sync(&self, path: PathBuf) -> io::Result<()> {
        match self {
            Entry::File(ref file) => file.sync(path).await,
            Entry::Directory(ref directory) => directory.sync(path).await,
        }
    }

    pub fn as_file(&mut self) -> Option<&mut File> {
        match self {
            Entry::File(ref mut file) => Some(file),
            _ => None,
        }
    }

    pub fn as_directory(&mut self) -> Option<&mut Directory> {
        match self {
            Entry::Directory(ref mut directory) => Some(directory),
            _ => None,
        }
    }
}

impl From<File> for Entry {
    fn from(file: File) -> Self {
        Entry::File(file)
    }
}

impl From<Directory> for Entry {
    fn from(directory: Directory) -> Self {
        Entry::Directory(directory)
    }
}

#[derive(Display, Debug)]
pub enum VfsError {
    AlreadyExists(String),
    NotFound(String),
    InvalidPath,
    EmptyPath,
    Generic(String),
}
impl std::error::Error for VfsError {}

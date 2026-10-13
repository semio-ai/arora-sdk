use crate::{io, valid_hash, verify, Error, FetchedAsset, ResolvedRef};
use crypto_secretbox::aead::{Aead, AeadCore, KeyInit, OsRng};
use crypto_secretbox::{Key, Nonce, XSalsa20Poly1305};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Content-addressed bytes and the last resolution for each requested name.
#[derive(Clone, Debug)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let dir = dir.into();
        fs::create_dir_all(dir.join("blobs")).map_err(|e| io(&dir, e))?;
        Ok(Self { dir })
    }

    pub fn directory(&self) -> &Path {
        &self.dir
    }

    /// A public path for build scripts. A sealed blob is never exposed by this path.
    pub fn public_path(&self, hash: &str) -> Result<PathBuf, Error> {
        self.blob_path(hash, false)
    }

    fn blob_path(&self, hash: &str, private: bool) -> Result<PathBuf, Error> {
        if !valid_hash(hash) {
            return Err(Error::Cache(format!("invalid content hash {hash:?}")));
        }
        Ok(self.dir.join("blobs").join(if private {
            format!("{hash}.sealed")
        } else {
            hash.into()
        }))
    }

    // A separate inode stays locked while data files are atomically replaced.
    fn lock(&self) -> Result<File, Error> {
        let path = self.dir.join(".lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| io(&path, e))?;
        file.lock().map_err(|e| io(&path, e))?;
        Ok(file)
    }

    pub fn read(&self, hash: &str, private: bool) -> Result<Option<Vec<u8>>, Error> {
        let path = self.blob_path(hash, private)?;
        let _lock = self.lock()?;
        let Some(stored) = read_optional(&path)? else {
            return Ok(None);
        };
        let bytes = if private {
            match self.read_key()? {
                Some(key) if stored.len() >= 24 => XSalsa20Poly1305::new(&key)
                    .decrypt(Nonce::from_slice(&stored[..24]), &stored[24..])
                    .ok(),
                _ => None,
            }
        } else {
            Some(stored)
        };
        match bytes {
            Some(bytes) if verify(hash, hash, &bytes).is_ok() => Ok(Some(bytes)),
            _ => {
                fs::remove_file(&path).map_err(|e| io(&path, e))?;
                Ok(None)
            }
        }
    }

    pub fn fetch(&self, pin: &ResolvedRef) -> Result<Option<FetchedAsset>, Error> {
        Ok(self
            .read(&pin.content_hash, pin.private)?
            .map(|bytes| FetchedAsset {
                bytes,
                content_hash: pin.content_hash.clone(),
                resolved: Some(pin.clone()),
            }))
    }

    /// Verify before writing. Private bytes are encrypted entirely in memory.
    pub fn store(&self, hash: &str, bytes: &[u8], private: bool) -> Result<(), Error> {
        let path = self.blob_path(hash, private)?;
        verify(hash, hash, bytes)?;
        let _lock = self.lock()?;
        if private {
            let key = self.key()?;
            let nonce = XSalsa20Poly1305::generate_nonce(&mut OsRng);
            let ciphertext = XSalsa20Poly1305::new(&key)
                .encrypt(&nonce, bytes)
                .map_err(|_| Error::Cache("could not seal bytes".into()))?;
            atomic_write(&path, &[nonce.as_slice(), &ciphertext].concat())
        } else {
            atomic_write(&path, bytes)
        }
    }

    pub fn last_pin(&self, requested_name: &str) -> Result<Option<ResolvedRef>, Error> {
        let _lock = self.lock()?;
        Ok(self.pins()?.remove(requested_name))
    }

    pub fn save_pin(&self, requested_name: &str, pin: &ResolvedRef) -> Result<(), Error> {
        let _lock = self.lock()?;
        let mut pins = self.pins()?;
        pins.insert(requested_name.into(), pin.clone());
        let mut value = serde_json::to_value(pins).map_err(|e| Error::Cache(e.to_string()))?;
        for pin in value.as_object_mut().unwrap().values_mut() {
            pin.as_object_mut().unwrap().remove("storagePath");
        }
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| Error::Cache(e.to_string()))?;
        atomic_write(&self.dir.join("resolved.json"), &bytes)
    }

    fn pins(&self) -> Result<BTreeMap<String, ResolvedRef>, Error> {
        let path = self.dir.join("resolved.json");
        match read_optional(&path)? {
            None => Ok(BTreeMap::new()),
            Some(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| Error::Cache(format!("{}: {e}", path.display()))),
        }
    }

    fn read_key(&self) -> Result<Option<Key>, Error> {
        let path = self.dir.join("key");
        Ok(read_optional(&path)?
            .filter(|b| b.len() == 32)
            .map(|b| *Key::from_slice(&b)))
    }

    fn key(&self) -> Result<Key, Error> {
        let path = self.dir.join("key");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                let key = XSalsa20Poly1305::generate_key(&mut OsRng);
                file.write_all(&key)
                    .and_then(|_| file.sync_all())
                    .map_err(|e| io(&path, e))?;
                Ok(key)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => self
                .read_key()?
                .ok_or_else(|| Error::Cache(format!("{} must contain 32 bytes", path.display()))),
            Err(e) => Err(io(&path, e)),
        }
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(|e| io(path, e))?;
    temp.write_all(bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| io(path, e))?;
    temp.persist(path).map_err(|e| io(path, e.error))?;
    Ok(())
}

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
                if let Err(error) = fs::remove_file(&path) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        log::warn!(target: "arora-assets", "{}: could not remove corrupt blob: {error}", path.display());
                    }
                }
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
        Ok(self.pins().remove(requested_name))
    }

    pub fn save_pin(&self, requested_name: &str, pin: &ResolvedRef) -> Result<(), Error> {
        let _lock = self.lock()?;
        let mut pins = self.pins();
        pins.insert(requested_name.into(), pin.clone());
        self.write_pins(&pins)
    }

    /// Remove a revoked name and sealed blobs with no remaining pin.
    pub fn remove_pin(&self, requested_name: &str) -> Result<(), Error> {
        let _lock = self.lock()?;
        let mut pins = self.pins();
        pins.remove(requested_name);
        self.write_pins(&pins)?;
        let blobs = self.dir.join("blobs");
        for entry in fs::read_dir(&blobs).map_err(|e| io(&blobs, e))? {
            let entry = entry.map_err(|e| io(&blobs, e))?;
            let file = entry.file_name();
            if let Some(hash) = file.to_str().and_then(|s| s.strip_suffix(".sealed")) {
                if !pins.values().any(|pin| pin.content_hash == hash) {
                    fs::remove_file(entry.path()).map_err(|e| io(&entry.path(), e))?;
                }
            }
        }
        Ok(())
    }

    fn write_pins(&self, pins: &BTreeMap<String, ResolvedRef>) -> Result<(), Error> {
        let mut value = serde_json::to_value(pins).map_err(|e| Error::Cache(e.to_string()))?;
        for pin in value.as_object_mut().unwrap().values_mut() {
            pin.as_object_mut().unwrap().remove("storagePath");
        }
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| Error::Cache(e.to_string()))?;
        atomic_write(&self.dir.join("resolved.json"), &bytes)
    }

    fn pins(&self) -> BTreeMap<String, ResolvedRef> {
        let path = self.dir.join("resolved.json");
        let Some(entries) = fs::read(&path).ok().and_then(|bytes| {
            serde_json::from_slice::<BTreeMap<String, serde_json::Value>>(&bytes).ok()
        }) else {
            return BTreeMap::new();
        };
        entries
            .into_iter()
            .filter_map(|(name, value)| {
                let mut pin: ResolvedRef = serde_json::from_value(value).ok()?;
                pin.storage_path = None;
                Some((name, pin))
            })
            .collect()
    }

    pub(crate) async fn blocking<R: Send + 'static>(
        &self,
        work: impl FnOnce(Self) -> Result<R, Error> + Send + 'static,
    ) -> Result<R, Error> {
        let cache = self.clone();
        tokio::task::spawn_blocking(move || work(cache))
            .await
            .map_err(|e| Error::Cache(e.to_string()))?
    }

    pub async fn fetch_async(&self, pin: &ResolvedRef) -> Result<Option<FetchedAsset>, Error> {
        let pin = pin.clone();
        self.blocking(move |cache| cache.fetch(&pin)).await
    }

    pub async fn store_async(&self, asset: &FetchedAsset) -> Result<(), Error> {
        let hash = asset.content_hash.clone();
        let bytes = asset.bytes.clone();
        let private = asset.resolved.as_ref().is_some_and(|pin| pin.private);
        self.blocking(move |cache| cache.store(&hash, &bytes, private))
            .await
    }

    fn read_key(&self) -> Result<Option<Key>, Error> {
        let path = self.dir.join("key");
        Ok(read_optional(&path)?
            .filter(|b| b.len() == 32)
            .map(|b| *Key::from_slice(&b)))
    }

    fn key(&self) -> Result<Key, Error> {
        let path = self.dir.join("key");
        if let Some(key) = self.read_key()? {
            return Ok(key);
        }
        let key = XSalsa20Poly1305::generate_key(&mut OsRng);
        publish_key(&path, &key)
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

fn private_temp(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile, Error> {
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(|e| io(path, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| io(path, e))?;
    }
    temp.write_all(bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| io(path, e))?;
    Ok(temp)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    private_temp(path, bytes)?
        .persist(path)
        .map_err(|e| io(path, e.error))?;
    Ok(())
}

fn publish_key(path: &Path, key: &Key) -> Result<Key, Error> {
    let temp = private_temp(path, key)?;
    match temp.persist_noclobber(path) {
        Ok(_) => Ok(*key),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let bytes = fs::read(path).map_err(|e| io(path, e))?;
            if bytes.len() != 32 {
                log::warn!(target: "arora-assets",
                    "{}: replacing key with invalid length {}",
                    path.display(),
                    bytes.len()
                );
                atomic_write(path, key)?;
                return Ok(*key);
            }
            Ok(*Key::from_slice(&bytes))
        }
        Err(e) => Err(io(path, e.error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_publication_is_complete_and_never_clobbers_a_winner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        let first = Key::from([1; 32]);
        let second = Key::from([2; 32]);
        let temp = private_temp(&path, &first).unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read(temp.path()).unwrap(), first.as_slice());
        drop(temp);
        assert_eq!(publish_key(&path, &first).unwrap(), first);
        assert_eq!(publish_key(&path, &second).unwrap(), first);
        assert_eq!(fs::read(path).unwrap(), first.as_slice());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

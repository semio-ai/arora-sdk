use crate::{io, valid_hash, verify, Error, FetchedAsset, ResolvedRef};
use crypto_secretbox::aead::{Aead, AeadCore, KeyInit, OsRng};
use crypto_secretbox::{Key, Nonce, XSalsa20Poly1305};
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
        if let Some(bytes) = self.verified_bytes(hash, private, stored)? {
            return Ok(Some(bytes));
        }
        if let Err(error) = self.remove_corrupt(&path, hash, private) {
            log::warn!(target: "arora-assets", "{}: could not remove corrupt blob: {error}", path.display());
        }
        Ok(None)
    }

    fn verified_bytes(
        &self,
        hash: &str,
        private: bool,
        stored: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, Error> {
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
        Ok(bytes.filter(|bytes| verify(hash, hash, bytes).is_ok()))
    }

    fn remove_corrupt(&self, path: &Path, hash: &str, private: bool) -> Result<(), Error> {
        let _lock = self.lock()?;
        if let Some(stored) = read_optional(path)? {
            // A writer may have replaced the failed read before this lock was acquired.
            if self.verified_bytes(hash, private, stored)?.is_none() {
                remove_optional(path)?;
            }
        }
        Ok(())
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
        Ok(self.raw_pins().ok().and_then(|mut entries| {
            let entry = entries.remove(requested_name)?;
            let mut pin: ResolvedRef = serde_json::from_str(entry.get()).ok()?;
            pin.storage_path = None;
            Some(pin)
        }))
    }

    pub fn save_pin(&self, requested_name: &str, pin: &ResolvedRef) -> Result<(), Error> {
        let _lock = self.lock()?;
        let mut pins = match self.raw_pins() {
            Ok(pins) => pins,
            Err(error) => {
                log::warn!(target: "arora-assets", "{requested_name}: could not save resolved pin: {error}");
                return Ok(());
            }
        };
        let mut value = serde_json::to_value(pin).map_err(|e| Error::Cache(e.to_string()))?;
        value.as_object_mut().unwrap().remove("storagePath");
        let value =
            serde_json::value::to_raw_value(&value).map_err(|e| Error::Cache(e.to_string()))?;
        pins.insert(requested_name.into(), value);
        self.write_pins(&pins)
    }

    /// Clear only this name and its sealed blob, preserving all other raw entries.
    /// Cleanup failures warn without failing the call.
    pub fn remove_pin(&self, requested_name: &str) -> Result<(), Error> {
        if let Err(error) = self.clear_pin(requested_name) {
            log::warn!(target: "arora-assets", "{requested_name}: could not clear resolved pin: {error}");
        }
        Ok(())
    }

    fn clear_pin(&self, requested_name: &str) -> Result<(), Error> {
        let _lock = self.lock()?;
        let mut pins = self.raw_pins()?;
        let Some(pin) = pins
            .get(requested_name)
            .and_then(|raw| serde_json::from_str::<ResolvedRef>(raw.get()).ok())
        else {
            return Ok(());
        };
        pins.remove(requested_name);
        self.write_pins(&pins)?;
        let referenced = pins.values().any(|raw| {
            serde_json::from_str::<serde_json::Value>(raw.get())
                .ok()
                .is_some_and(|value| {
                    value.get("contentHash").and_then(serde_json::Value::as_str)
                        == Some(&pin.content_hash)
                })
        });
        if !referenced {
            remove_optional(&self.blob_path(&pin.content_hash, true)?)?;
        }
        Ok(())
    }

    fn write_pins(&self, pins: &BTreeMap<String, Box<RawValue>>) -> Result<(), Error> {
        let bytes = serde_json::to_vec_pretty(pins).map_err(|e| Error::Cache(e.to_string()))?;
        atomic_write(&self.dir.join("resolved.json"), &bytes)
    }

    fn raw_pins(&self) -> Result<BTreeMap<String, Box<RawValue>>, Error> {
        let path = self.dir.join("resolved.json");
        Ok(read_optional(&path)?
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default())
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

    /// Move owned or shared bytes to a blocking worker without copying the body.
    pub async fn store_async(&self, asset: impl Into<Arc<FetchedAsset>>) -> Result<(), Error> {
        let asset = asset.into();
        self.blocking(move |cache| {
            let private = asset.resolved.as_ref().is_some_and(|pin| pin.private);
            cache.store(&asset.content_hash, &asset.bytes, private)
        })
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

fn remove_optional(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(path, e)),
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
mod tests;

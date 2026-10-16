//! Exact public models for bespoke HAL build scripts.
//! Pass a Studio source from studio-bridge or a `DirSource`. The project tool
//! will pass the same `ARORA_MODEL_<NAME>` variables from project pins.

use crate::version::Version;
use crate::{env_value, io, valid_hash, Cache, Error, Kind, Resolver, Source};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const VARIABLES: &[&str] = &[
    "CARGO_MANIFEST_DIR",
    "ARORA_ASSETS_CACHE_DIR",
    "CARGO_HOME",
    "HOME",
    "ARORA_ASSETS_OFFLINE",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    scoped_name: String,
    version: String,
    content_hash: String,
}

/// Read package metadata, prepare the public blobs, and emit Cargo directives.
pub fn models_with(source: &(impl Source + ?Sized)) -> Result<(), Error> {
    models_with_env(
        source,
        |name| std::env::var(name).ok(),
        |line| println!("{line}"),
    )
}

/// The build helper with explicit environment lookup and directive output.
/// It downloads on a thread of its own, so it works inside or outside a Tokio runtime.
pub fn models_with_env(
    source: &(impl Source + ?Sized),
    mut lookup: impl FnMut(&str) -> Option<String>,
    mut emit: impl FnMut(&str),
) -> Result<(), Error> {
    emit("cargo:rerun-if-changed=Cargo.toml");
    let mut env = BTreeMap::new();
    for name in VARIABLES {
        emit(&format!("cargo:rerun-if-env-changed={name}"));
        if let Some(value) = env_value(lookup(name)) {
            env.insert(*name, value);
        }
    }
    let get = |name: &str| env.get(name).cloned();
    let manifest_dir = get("CARGO_MANIFEST_DIR")
        .ok_or_else(|| Error::Config("missing CARGO_MANIFEST_DIR".into()))?;
    let manifest = Path::new(&manifest_dir).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| io(&manifest, e))?;
    let document: toml::Value =
        toml::from_str(&text).map_err(|e| Error::Config(format!("{}: {e}", manifest.display())))?;
    let mut metadata = &document;
    for key in ["package", "metadata", "arora", "models"] {
        match metadata.get(key) {
            Some(value) => metadata = value,
            None => return Ok(()),
        }
    }
    let entries = metadata
        .as_table()
        .ok_or_else(|| Error::Config("package.metadata.arora.models must be a table".into()))?;
    let pins = validate_pins(entries)?;
    if pins.is_empty() {
        return Ok(());
    }
    let cache_dir = cache_directory(get)?;
    let cache = Cache::new(cache_dir)?;
    let offline = get("ARORA_ASSETS_OFFLINE").as_deref() == Some("1");
    for (name, variable, pin) in pins {
        let fail = |detail: String| Error::Build {
            entry: name.clone(),
            detail,
        };
        if cache.read(&pin.content_hash, false)?.is_none() {
            if offline {
                return Err(fail("offline cache miss; fill the cache by building once with ARORA_ASSETS_OFFLINE unset and the source available".into()));
            }
            fetch_into_cache(source, &cache, name, &pin)?;
        }
        let path = cache.public_path(&pin.content_hash)?;
        let absolute = path.canonicalize().map_err(|e| io(&path, e))?;
        let path_text = absolute
            .to_str()
            .filter(|p| !p.contains(['\n', '\r']))
            .ok_or_else(|| fail("cache path cannot be represented in a Cargo directive".into()))?;
        emit(&format!("cargo:rustc-env={variable}={path_text}"));
        emit(&format!("cargo:rerun-if-changed={path_text}"));
    }
    Ok(())
}

/// Choose the shared build cache without reading the process environment.
pub fn cache_directory(mut lookup: impl FnMut(&str) -> Option<String>) -> Result<PathBuf, Error> {
    if let Some(dir) = env_value(lookup("ARORA_ASSETS_CACHE_DIR")) {
        return Ok(PathBuf::from(dir));
    }
    if let Some(dir) = env_value(lookup("CARGO_HOME")) {
        return Ok(PathBuf::from(dir).join("arora-assets"));
    }
    Ok(PathBuf::from(env_value(lookup("HOME")).ok_or_else(|| {
        Error::Config("missing HOME (set ARORA_ASSETS_CACHE_DIR or CARGO_HOME)".into())
    })?)
    .join(".cargo/arora-assets"))
}

fn validate_pins(
    entries: &toml::map::Map<String, toml::Value>,
) -> Result<Vec<(&String, String, Pin)>, Error> {
    let mut pins = Vec::new();
    let mut names = BTreeSet::new();
    for (name, value) in entries {
        let fail = |detail: String| Error::Build {
            entry: name.clone(),
            detail,
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        {
            return Err(fail("name must match [a-z0-9_-]+".into()));
        }
        let variable = format!("ARORA_MODEL_{}", name.to_uppercase().replace('-', "_"));
        if !names.insert(variable.clone()) {
            return Err(fail(format!("duplicate output variable {variable}")));
        }
        let pin: Pin = value
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| fail(e.to_string()))?;
        crate::selection::validate_name(&pin.scoped_name).map_err(|e| fail(e.to_string()))?;
        if Version::parse(&pin.version).is_none_or(|v| v.to_string() != pin.version) {
            return Err(fail(format!(
                "version {:?} must be an exact canonical version at least 1.0.0",
                pin.version
            )));
        }
        if !valid_hash(&pin.content_hash) {
            return Err(fail("content_hash must be 64 lowercase hex digits".into()));
        }
        pins.push((name, variable, pin));
    }
    Ok(pins)
}

/// Resolve one pin and store its bytes in the cache. A build script may already
/// run inside a Tokio runtime, where starting another on the same thread panics,
/// so the work runs on a scoped thread with a current-thread runtime of its own.
fn fetch_into_cache(
    source: &(impl Source + ?Sized),
    cache: &Cache,
    name: &str,
    pin: &Pin,
) -> Result<(), Error> {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| Error::Config(format!("could not start async runtime: {e}")))?;
                let resolver = Resolver::new(source, cache.clone());
                runtime.block_on(async {
                    let resolved = resolver
                        .resolve(&pin.scoped_name, &pin.version, Kind::MODEL, "primary")
                        .await?;
                    validate_resolved(name, pin, &resolved)?;
                    resolver.fetch(&resolved).await.map(|_| ())
                })
            })
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

fn validate_resolved(name: &str, pin: &Pin, resolved: &crate::ResolvedRef) -> Result<(), Error> {
    let fail = |detail| Error::Build {
        entry: name.into(),
        detail,
    };
    if resolved.private {
        return Err(fail(format!(
            "{}: build models must be public",
            pin.scoped_name
        )));
    }
    if resolved.version != pin.version {
        return Err(fail("resolved version differs from the build pin".into()));
    }
    if resolved.content_hash != pin.content_hash {
        return Err(Error::HashMismatch {
            asset: format!("entry {name}: {}", pin.scoped_name),
            expected: pin.content_hash.clone(),
            actual: resolved.content_hash.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;

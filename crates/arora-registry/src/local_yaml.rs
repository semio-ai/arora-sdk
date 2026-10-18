use crate::local::LocalRegistry;
use crate::{
    EditableRegistry, EnumerationFrozen, FolderPublic, ModuleFrozen, RegistryError, StructureFrozen,
};
use semver::Version;
use std::{ffi::OsStr, path::Path, str::FromStr};
use uuid::Uuid;

/// Reads a directory describing registry records in YAML format,
/// and loads them into the given registry.
/// The directory may contain a subdirectory for each record type:
/// `folder`, `enumeration`, `structure`, `module`.
/// Each subdirectory may contain a list of records serialized in YAML,
/// into files named `<uuid>{@<tag>}.yaml`,
/// where `<uuid>` is the UUID to give to the record when adding it to the registry,
/// and `<tag>` is the version tag of the record. Folders are added each after
/// its parent, then enumerations, structures and modules.
pub async fn load_records_from_yaml_dir<P: AsRef<Path>>(
    path: P,
    registry: &mut dyn EditableRegistry,
) -> Result<(), RegistryError> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(RegistryError::Generic {
            message: format!("Path does not exist: {}", path.display()),
        });
    }
    if !path.is_dir() {
        return Err(RegistryError::Generic {
            message: format!("Path is not a directory: {}", path.display()),
        });
    }

    let mut folders = Vec::new();
    for_each_yaml_record(&path.join("folder"), &mut |id, _, yaml: String| {
        folders.push((
            id,
            serde_yaml::from_str::<FolderPublic>(yaml.as_str()).map_err(|err| {
                RegistryError::ParsingError {
                    message: format!(
                        "YAML folder description in {} is invalid: {}",
                        path.display(),
                        err
                    ),
                }
            })?,
        ));
        Ok(())
    })
    .await?;
    add_folders(folders, registry).await?;

    let mut enumerations = Vec::new();
    for_each_yaml_record(
        &path.join("enumeration"),
        &mut |id, tag: Option<Version>, yaml: String| {
            enumerations.push((
                id,
                tag.ok_or(RegistryError::ParsingError {
                    message: format!(
                        "YAML file name was missing version information: {}",
                        path.display()
                    ),
                })?,
                serde_yaml::from_str::<EnumerationFrozen>(yaml.as_str()).map_err(|err| {
                    RegistryError::ParsingError {
                        message: format!(
                            "YAML enumeration description in {} is invalid: {}",
                            path.display(),
                            err
                        ),
                    }
                })?,
            ));
            Ok(())
        },
    )
    .await?;
    for (id, tag, enumeration) in enumerations {
        registry.add_enumeration(id, tag, enumeration).await?;
    }

    let mut structures = Vec::new();
    for_each_yaml_record(
        &path.join("structure"),
        &mut |id, tag: Option<Version>, yaml: String| {
            structures.push((
                id,
                tag.ok_or(RegistryError::ParsingError {
                    message: format!(
                        "YAML file name was missing version information: {}",
                        path.display()
                    ),
                })?,
                serde_yaml::from_str::<StructureFrozen>(yaml.as_str()).map_err(|err| {
                    RegistryError::ParsingError {
                        message: format!(
                            "YAML structure description in {} is invalid: {}",
                            path.display(),
                            err
                        ),
                    }
                })?,
            ));
            Ok(())
        },
    )
    .await?;
    for (id, tag, structure) in structures {
        registry.add_structure(id, tag, structure).await?;
    }

    let mut modules = Vec::new();
    for_each_yaml_record(
        &path.join("module"),
        &mut |id, tag: Option<Version>, yaml: String| {
            modules.push((
                id,
                tag.ok_or(RegistryError::ParsingError {
                    message: format!(
                        "YAML file name was missing version information: {}",
                        path.display()
                    ),
                })?,
                serde_yaml::from_str::<ModuleFrozen>(yaml.as_str()).map_err(|err| {
                    RegistryError::ParsingError {
                        message: format!(
                            "YAML module description in {} is invalid: {}",
                            path.display(),
                            err
                        ),
                    }
                })?,
            ));
            Ok(())
        },
    )
    .await?;
    for (id, tag, module) in modules {
        registry.add_module(id, tag, module).await?;
    }
    Ok(())
}

/// Add `folders`, each after its parent: a folder's path names its parent's,
/// so a parent must be held first, whatever order the folders were read in.
/// Fails with the error of a folder whose parent is neither the root, nor
/// held, nor among `folders`.
pub(crate) async fn add_folders(
    mut folders: Vec<(Uuid, FolderPublic)>,
    registry: &mut dyn EditableRegistry,
) -> Result<(), RegistryError> {
    while !folders.is_empty() {
        let mut waiting = Vec::new();
        let mut unknown_parent = None;
        let count = folders.len();
        for (id, folder) in folders {
            match registry.add_folder(id, folder.clone()).await {
                Ok(()) => {}
                Err(error @ RegistryError::UnknownParent { .. }) => {
                    unknown_parent = Some(error);
                    waiting.push((id, folder));
                }
                Err(error) => return Err(error),
            }
        }
        if waiting.len() == count {
            return Err(unknown_parent.expect("a folder waits on its parent"));
        }
        folders = waiting;
    }
    Ok(())
}

async fn for_each_yaml_record<F>(path: &Path, mut f: F) -> Result<(), RegistryError>
where
    F: FnMut(Uuid, Option<Version>, String) -> Result<(), RegistryError>,
{
    let dir = match std::fs::read_dir(path) {
        Ok(dir) => dir,
        _ => return Ok(()),
    };
    for entry in dir {
        let entry = entry.map_err(|err| RegistryError::Generic {
            message: format!("failed to read directory {}: {}", path.display(), err),
        })?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("yaml")) {
            continue;
        }
        let stem = match path.file_stem().map(OsStr::to_str) {
            Some(Some(stem)) => stem,
            _ => continue,
        };
        let (id, tag) = if let Some(n) = stem.find("@") {
            let (id_str, tag_str) = stem.split_at(n);
            let id = match Uuid::from_str(id_str) {
                Ok(id) => id,
                _ => continue,
            };
            let tag = match Version::parse(&tag_str[1..]) {
                Ok(tag) => Some(tag),
                Err(_) => continue,
            };
            (id, tag)
        } else {
            let id = match Uuid::from_str(stem) {
                Ok(id) => id,
                _ => continue,
            };
            (id, None)
        };
        let yaml = std::fs::read_to_string(&path).map_err(|err| RegistryError::Generic {
            message: format!(
                "failed to read record description in {}: {}",
                path.display(),
                err
            ),
        })?;
        f(id, tag, yaml)?;
    }
    Ok(())
}

/// The kind of registry record, identifying how an entry passed to
/// [`load_records`] should be parsed and added. Kinds order as they load:
/// folders, enumerations, structures, then modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordKind {
    Folder,
    Enumeration,
    Structure,
    Module,
}

impl RecordKind {
    /// The subdirectory of a record directory holding records of this kind.
    pub fn directory(self) -> &'static str {
        match self {
            RecordKind::Folder => "folder",
            RecordKind::Enumeration => "enumeration",
            RecordKind::Structure => "structure",
            RecordKind::Module => "module",
        }
    }

    /// The kind of the records a record directory's subdirectory `name`
    /// holds, or `None` for a name that is not one.
    pub fn from_directory(name: &str) -> Option<RecordKind> {
        [
            RecordKind::Folder,
            RecordKind::Enumeration,
            RecordKind::Structure,
            RecordKind::Module,
        ]
        .into_iter()
        .find(|kind| kind.directory() == name)
    }
}

/// The records `registry` holds, as [`load_records`] reads them: each one's
/// kind, its file stem (`<uuid>[@<version>]`) and its YAML, ordered by kind
/// (folders, enumerations, structures, modules) then by stem. The same
/// records write the same YAML each time.
pub fn records_as_yaml(
    registry: &LocalRegistry,
) -> Result<Vec<(RecordKind, String, String)>, RegistryError> {
    use crate::local::FrozenRegistryReference as Held;
    let yaml = |what: &str, result: Result<String, serde_yaml::Error>| {
        result.map_err(|err| RegistryError::generic(&format!("{what} does not serialize: {err}")))
    };
    let mut records = Vec::new();
    for (id, version, held) in registry.held() {
        let stem = format!("{id}@{version}");
        records.push(match held {
            Held::Folder { record, .. } => (
                RecordKind::Folder,
                id.to_string(),
                yaml("a folder", serde_yaml::to_string(record.as_ref()))?,
            ),
            Held::Enumeration { record, .. } => (
                RecordKind::Enumeration,
                stem,
                yaml("an enumeration", serde_yaml::to_string(record.as_ref()))?,
            ),
            Held::Structure { record, .. } => (
                RecordKind::Structure,
                stem,
                yaml("a structure", serde_yaml::to_string(record.as_ref()))?,
            ),
            Held::Module { record, .. } => (
                RecordKind::Module,
                stem,
                yaml(
                    "a module",
                    serde_yaml::to_value(record.as_ref())
                        .map(sort_id_keyed_mappings)
                        .and_then(|value| serde_yaml::to_string(&value)),
                )?,
            ),
            _ => continue,
        });
    }
    records.sort_by(|(a, a_stem, _), (b, b_stem, _)| (a, a_stem).cmp(&(b, b_stem)));
    Ok(records)
}

/// `value` with every mapping keyed by ids ordered by id, so a module
/// record's exports and parameters — unordered maps, a function's
/// parameters ordered by its `parameterOrdering` — write the same YAML each
/// time. Not for a structure's fields or an enumeration's variants, whose
/// order is their declared one.
fn sort_id_keyed_mappings(value: serde_yaml::Value) -> serde_yaml::Value {
    use serde_yaml::{Mapping, Value};
    match value {
        Value::Mapping(mapping) => {
            let mut entries: Vec<(Value, Value)> = mapping
                .into_iter()
                .map(|(key, value)| (key, sort_id_keyed_mappings(value)))
                .collect();
            let id_keyed = entries
                .iter()
                .all(|(key, _)| key.as_str().is_some_and(|key| Uuid::from_str(key).is_ok()));
            if id_keyed {
                entries.sort_by(|(a, _), (b, _)| a.as_str().cmp(&b.as_str()));
            }
            Value::Mapping(entries.into_iter().collect::<Mapping>())
        }
        Value::Sequence(sequence) => {
            Value::Sequence(sequence.into_iter().map(sort_id_keyed_mappings).collect())
        }
        other => other,
    }
}

/// Write the records `registry` holds into the directory `path`, in the
/// layout [`load_records_from_yaml_dir`] reads —
/// `<path>/<kind>/<uuid>[@<version>].yaml` — creating the directories as
/// needed.
pub fn write_records_to_yaml_dir<P: AsRef<Path>>(
    registry: &LocalRegistry,
    path: P,
) -> Result<(), RegistryError> {
    let path = path.as_ref();
    for (kind, stem, yaml) in records_as_yaml(registry)? {
        let dir = path.join(kind.directory());
        std::fs::create_dir_all(&dir).map_err(|err| {
            RegistryError::generic(&format!("could not create {}: {err}", dir.display()))
        })?;
        let file = dir.join(format!("{stem}.yaml"));
        std::fs::write(&file, yaml).map_err(|err| {
            RegistryError::generic(&format!("could not write {}: {err}", file.display()))
        })?;
    }
    Ok(())
}

/// Parse a record file stem `<uuid>[@<version>]` into its id and optional
/// version tag. Returns `None` if the uuid (or, when present, the version) is
/// not parseable.
pub fn parse_record_stem(stem: &str) -> Option<(Uuid, Option<Version>)> {
    if let Some(at) = stem.find('@') {
        let (id_str, tag_str) = stem.split_at(at);
        let id = Uuid::from_str(id_str).ok()?;
        let tag = Version::parse(&tag_str[1..]).ok()?;
        Some((id, Some(tag)))
    } else {
        Some((Uuid::from_str(stem).ok()?, None))
    }
}

/// Load registry records from in-memory YAML — the portable, filesystem-free
/// counterpart to [`load_records_from_yaml_dir`].
///
/// Each item is `(kind, file_stem, yaml)`, where `file_stem` is the record
/// file's name without extension (`<uuid>[@<version>]`). This lets a caller
/// embed the records (e.g. via `include_dir!`) and load them on targets without
/// a filesystem, such as `wasm32`. Records are added in dependency order
/// (folders, each after its parent, then enumerations, structures and
/// modules) regardless of input order.
pub async fn load_records<'a, I>(
    records: I,
    registry: &mut dyn EditableRegistry,
) -> Result<(), RegistryError>
where
    I: IntoIterator<Item = (RecordKind, &'a str, String)>,
{
    let mut folders = Vec::new();
    let mut enumerations = Vec::new();
    let mut structures = Vec::new();
    let mut modules = Vec::new();

    for (kind, stem, yaml) in records {
        let (id, tag) = parse_record_stem(stem).ok_or_else(|| RegistryError::ParsingError {
            message: format!("record file name is not a uuid[@version]: {stem}"),
        })?;
        match kind {
            RecordKind::Folder => folders.push((id, yaml)),
            RecordKind::Enumeration => enumerations.push((id, tag, yaml)),
            RecordKind::Structure => structures.push((id, tag, yaml)),
            RecordKind::Module => modules.push((id, tag, yaml)),
        }
    }

    let parse_err = |what: &str, err: serde_yaml::Error| RegistryError::ParsingError {
        message: format!("YAML {what} description is invalid: {err}"),
    };
    let need_version = || RegistryError::ParsingError {
        message: "record file name was missing version information".to_string(),
    };

    let folders = folders
        .into_iter()
        .map(|(id, yaml)| {
            serde_yaml::from_str::<FolderPublic>(&yaml)
                .map(|folder| (id, folder))
                .map_err(|e| parse_err("folder", e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    add_folders(folders, registry).await?;
    for (id, tag, yaml) in enumerations {
        let enumeration = serde_yaml::from_str::<EnumerationFrozen>(&yaml)
            .map_err(|e| parse_err("enumeration", e))?;
        registry
            .add_enumeration(id, tag.ok_or_else(need_version)?, enumeration)
            .await?;
    }
    for (id, tag, yaml) in structures {
        let structure = serde_yaml::from_str::<StructureFrozen>(&yaml)
            .map_err(|e| parse_err("structure", e))?;
        registry
            .add_structure(id, tag.ok_or_else(need_version)?, structure)
            .await?;
    }
    for (id, tag, yaml) in modules {
        let module =
            serde_yaml::from_str::<ModuleFrozen>(&yaml).map_err(|e| parse_err("module", e))?;
        registry
            .add_module(id, tag.ok_or_else(need_version)?, module)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::LocalRegistry;
    use crate::local_yaml::load_records_from_yaml_dir;
    use crate::ReadableRegistry;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_load_records_from_yaml_dir() {
        let mut registry = LocalRegistry::new();
        let path = PathBuf::from("test_data/behavior_tree_types");
        load_records_from_yaml_dir(path, &mut registry)
            .await
            .unwrap();
        assert_eq!(
            registry
                .resolve_id(&Uuid::from_str("325a5767-e344-4532-860e-0749bcf2e428").unwrap())
                .await
                .unwrap(),
            "behavior_tree.Status"
        );
    }
}

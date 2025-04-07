use crate::{
  EditableRegistry, EnumerationFrozen, FolderPublic, ModuleFrozen, RegistryError, StructureFrozen,
};
use semver::Version;
use std::{ffi::OsStr, path::Path, str::FromStr};
use tokio::fs::read_to_string;
use uuid::Uuid;

/// Reads a directory describing registry records in YAML format,
/// and loads them into the given registry.
/// The directory may contain a subdirectory for each record type:
/// `folder`, `enumeration`, `structure`, `module`.
/// Each subdirectory may contain a list of records serialized in YAML,
/// into files named `<uuid>{@<tag>}.yaml`,
/// where `<uuid>` is the UUID to give to the record when adding it to the registry,
/// and `<tag>` is the version tag of the record.
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
  for (id, folder) in folders {
    registry.add_folder(id, folder).await?;
  }

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

async fn for_each_yaml_record<F>(path: &Path, mut f: F) -> Result<(), RegistryError>
where
  F: FnMut(Uuid, Option<Version>, String) -> Result<(), RegistryError>,
{
  let mut dir = match tokio::fs::read_dir(path).await {
    Ok(dir) => dir,
    _ => return Ok(()),
  };
  while let Some(entry) = dir
    .next_entry()
    .await
    .map_err(|err| RegistryError::Generic {
      message: format!("failed to read directory {}: {}", path.display(), err),
    })?
  {
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
    let yaml = read_to_string(&path)
      .await
      .map_err(|err| RegistryError::Generic {
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

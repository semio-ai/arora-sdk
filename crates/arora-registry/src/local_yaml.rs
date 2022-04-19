use tokio::fs::read_to_string;
use uuid::Uuid;

use crate::{
  EditableRegistry, EnumerationPublic, FolderPublic, ModulePublic, RegistryError, StructurePublic,
};
use std::{ffi::OsStr, path::Path, str::FromStr};

/// Reads a directory describing registry records in YAML format,
/// and loads them into the given registry.
/// The directory may contain a subdirectory for each record type:
/// `folder`, `enumeration`, `structure`, `module`.
/// Each subdirectory may contain a list of records serialized in YAML,
/// into files named `<uuid>.yaml`,
/// where `<uuid>` is the UUID to give to the record when adding it to the registry.
pub async fn load_records_from_yaml_dir<P: AsRef<Path>>(
  path: P,
  registry: &mut dyn EditableRegistry,
) -> Result<(), RegistryError> {
  let path = path.as_ref();

  let mut folders = Vec::new();
  for_each_uuid_yaml(&path.join("folder"), &mut |id, yaml: String| {
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
  for_each_uuid_yaml(&path.join("enumeration"), &mut |id, yaml: String| {
    enumerations.push((
      id,
      serde_yaml::from_str::<EnumerationPublic>(yaml.as_str()).map_err(|err| {
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
  })
  .await?;
  for (id, enumeration) in enumerations {
    registry.add_enumeration(id, enumeration).await?;
  }

  let mut structures = Vec::new();
  for_each_uuid_yaml(&path.join("structure"), &mut |id, yaml: String| {
    structures.push((
      id,
      serde_yaml::from_str::<StructurePublic>(yaml.as_str()).map_err(|err| {
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
  })
  .await?;
  for (id, structure) in structures {
    registry.add_structure(id, structure).await?;
  }

  let mut modules = Vec::new();
  for_each_uuid_yaml(&path.join("module"), &mut |id, yaml: String| {
    modules.push((
      id,
      serde_yaml::from_str::<ModulePublic>(yaml.as_str()).map_err(|err| {
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
  })
  .await?;
  for (id, module) in modules {
    registry.add_module(id, module).await?;
  }
  Ok(())
}

async fn for_each_uuid_yaml<F>(path: &Path, mut f: F) -> Result<(), RegistryError>
where
  F: FnMut(Uuid, String) -> Result<(), RegistryError>,
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
    let id = match Uuid::from_str(stem) {
      Ok(id) => id,
      _ => continue,
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
    f(id, yaml)?;
  }
  Ok(())
}

use std::path;

use arora_module_core::Asset2;
use arora_vfs::{Directory, File, VfsError};
use convert_case::{Case, Casing};
use quote::{__private::TokenStream, quote};
use semio_record::enumeration::v0::public::Public as EnumerationPublic;
use uuid::Uuid;

/// Generates a set of sources organized in a virtual directory
/// from a set of assets as produced by [`arora_module_core::analyze_module`].
/// First, the types, then the modules, then the imports.
pub fn generate_sources(assets: Vec<Asset2>) -> Directory {
  Directory::new()
}

pub fn generate_enumeration_source(
  id: &Uuid,
  enumeration: &EnumerationPublic,
  parent_path: &String,
) -> Result<Directory, VfsError> {
  let tokens = quote! {};
  token_stream_to_file(
    format!(
      "{}/{}.rs",
      parent_path.replace('.', "/"),
      enumeration.name.to_case(Case::Snake)
    ),
    &tokens,
  )
}

pub fn token_stream_to_file(
  file_path: String,
  tokens: &TokenStream,
) -> Result<Directory, VfsError> {
  let file_path = path::Path::new(&file_path);
  let file_name = file_path.file_name().unwrap().to_str().unwrap();
  let parent_path = file_path.parent().unwrap();
  let mut output = Directory::new();
  let parent_dir = match output.ensure_directories(&parent_path.to_path_buf()) {
    Ok(dir) => dir,
    Err(VfsError::EmptyPath) => &mut output,
    Err(err) => return Err(err),
  };
  parent_dir.insert(file_name, File::new(tokens.to_string()))?;
  Ok(output)
}

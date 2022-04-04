use std::{fmt::Display, path};

use arora_module_core::Asset2;
use arora_vfs::{Directory, Entry, File, VfsError};
use convert_case::{Case, Casing};
use quote::{
  __private::{Ident, TokenStream},
  format_ident, quote, ToTokens,
};
use semio_record::enumeration::v0::public::Public as EnumerationPublic;
use uuid::Uuid;

/// Generates a set of sources organized in a virtual directory
/// from a set of assets as produced by [`arora_module_core::analyze_module`].
/// First, the types, then the modules, then the imports.
pub fn generate_sources(assets: Vec<Asset2>) -> Directory {
  Directory::new()
}

/// Generates `mod.rs` files and adds them at every level of the directory hierarchy
/// where `.rs` files can be found. Returns true if it was generated.
pub fn generate_mods_in_directories(dir: &mut Directory) -> Result<bool, VfsError> {
  let mut mods = Vec::new();
  for (path, entry) in dir.list_mut() {
    if let Entry::Directory(ref mut dir) = entry {
      if generate_mods_in_directories(dir)? {
        mods.push(path);
      }
    } else {
      if path.ends_with(".rs") {
        mods.push(path[..path.len() - 3].to_string());
      }
    }
  }
  if !mods.is_empty() {
    let mods = mods
      .into_iter()
      .map(|mod_name| format_ident!("{}", mod_name));
    let tokens = quote! {
      #(pub mod #mods;)*
    };
    dir.insert("mod.rs", File::new(tokens.to_string()))?;
    Ok(true)
  } else {
    Ok(false)
  }
}

/// Generates sources that are common dependencies to
/// other generated sources.
/// Always call this function before generating sources.
pub fn generate_common_sources() -> Result<Directory, VfsError> {
  let source = quote! {
    use derive_more::Display;

    #[derive(Display, Debug)]
    pub struct DeserializationError {
      #[display(fmt = "deserialization error: {}", message)]
      pub message: String,
    }

    impl std::error::Error for DeserializationError {}
  };
  token_stream_to_file("error.rs".to_string(), &source)
}

/// Generates a Rust source file for the given enumeration.
/// It contains the type declaration and some functions
/// to serialize and deserialixe values.
/// It depends on `arora_buffers` and the
pub fn generate_enumeration_source(
  id: &Uuid,
  enumeration: &EnumerationPublic,
  parent_path: &String,
) -> Result<Directory, VfsError> {
  let uses = quote! {
    use crate::arora_generated::error::DeserializationError;
    use arora_buffers::*;
    use arora_schema::value::{ConversionError, Enumeration, Value};
    use uuid::Uuid;
  };

  // Enum declaration.
  let name = &enumeration.name;
  let enum_name = name.to_case(Case::UpperCamel);
  let enum_ident = type_ident(&enum_name);
  let variants = enumeration.variants.iter();
  let enum_contents = variants.clone().map(|(_, variant)| {
    let variant_ident = format_ident!("{}", variant.name.to_case(Case::UpperCamel));
    quote! { #variant_ident, }
  });
  let enum_declaration = quote! {
    #[derive(Debug, PartialEq)]
    pub enum #enum_ident {
      #(#enum_contents)*
    }
  };

  // Enum IDs.
  let enum_id = id.to_string();
  let enum_id_bytes = RawUuidValue(id);
  let enum_upper_name = format_ident!("{}", enum_name.to_case(Case::ScreamingSnake));
  let enum_const_id_ident = format_ident!("{}_ENUM_RAW_ID", enum_upper_name);
  let enum_const_id_doc = format!("{}: {}", enum_name, enum_id);
  let enum_id_declaration = quote! {
    #[doc = #enum_const_id_doc]
    pub const #enum_const_id_ident: [u8; 16] = #enum_id_bytes;
  };

  let variant_id_declarations = variants.clone().map(|(id, variant)| {
    let id_string = id.to_string();
    let id_bytes = RawUuidValue(id);
    let variant_const_id_ident = enum_variant_const_id_ident(&enum_name, &variant.name);
    let variant_doc = format!(
      "{}: {}",
      enum_variant_ident(&enum_name, &variant.name).to_string(),
      id_string
    );
    quote! {
      #[doc = #variant_doc]
      pub const #variant_const_id_ident: [u8; 16] = #id_bytes;
    }
  });

  // Enum Serialization.
  let serialization_match_branches = variants.clone().map(|(_, variant)| {
    let variant_ident = enum_variant_ident(&enum_name, &variant.name);
    let id_ident = enum_variant_const_id_ident(&enum_name, &variant.name);
    quote! {
      #variant_ident => #id_ident.as_slice(),
    }
  });

  let into_impl = generate_into_impl(&enum_ident);
  let serialization = quote! {
    #into_impl

    pub fn serialize_to_writer(value: &#enum_ident, writer: &mut BufferWriter) {
      let enumeration_id = #enum_const_id_ident.as_slice();
      let variant_id = match value {
        #(#serialization_match_branches)*
      };
      writer.add_enumeration_value(enumeration_id, variant_id);
      writer.add_unit();
    }
  };

  // Enum Deserialization.
  let deserialization_cases = variants.clone().map(|(_, variant)| {
    let variant_const_id_ident = enum_variant_const_id_ident(&enum_name, &variant.name);
    let variant_ident = enum_variant_ident(&enum_name, &variant.name);
    quote! {
      #variant_const_id_ident => Ok(#variant_ident),
    }
  });

  let deserialization = quote! {
    impl TryFrom<&[u8]> for #enum_ident {
      type Error = DeserializationError;

      fn try_from(buffer: &[u8]) -> Result<Self, Self::Error> {
        let mut reader = BufferReader::new(buffer);
        return deserialize_from_reader(&mut reader, true)
      }
    }

    pub fn deserialize_from_reader(reader: &mut BufferReader, check_type: bool) -> Result<#enum_ident, DeserializationError> {
      if check_type {
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
          return Err(DeserializationError{ message: "missing next type information".to_string() })
        }
        if type_raw_id_opt.unwrap() != TYPE_ENUMERATION {
          return Err(DeserializationError{ message: "next type is not an enumeration".to_string() })
        }
      }

      if #enum_const_id_ident != reader.get_structure_field() {
        return Err(DeserializationError{ message: "missing variant information".to_string() })
      }

      let variant_raw_id = reader.get_enumeration_value_raw();
      match variant_raw_id.try_into().expect("enum id is of unexpected length") {
        #(#deserialization_cases)*
        _ => Err(DeserializationError{ message: "unexpected variant".to_string() })
      }
    }
  };

  // Conversion to generic `Value`.
  let to_value_cases = variants.clone().map(|(_, variant)| {
    let variant_ident = enum_variant_ident(&enum_name, &variant.name);
    let id_ident = enum_variant_const_id_ident(&enum_name, &variant.name);
    quote! {
      #variant_ident => Value::Enumeration(Enumeration {
        id: Uuid::from_bytes(#enum_const_id_ident),
        variant_id: Uuid::from_bytes(#id_ident),
        value: Box::new(Value::Unit),
      }),
    }
  });

  let to_value = quote! {
    impl Into<Value> for #enum_ident {
      fn into(self) -> Value {
        match self {
          #(#to_value_cases)*
        }
      }
    }
  };

  // Conversion from generic `Value`.
  let from_value_cases = variants.map(|(_, variant)| {
    let variant_const_id_ident = enum_variant_const_id_ident(&enum_name, &variant.name);
    let variant_ident = enum_variant_ident(&enum_name, &variant.name);
    quote! {
      #variant_const_id_ident => Ok(#variant_ident),
    }
  });

  let from_value = quote! {
    impl TryFrom<Value> for #enum_ident {
      type Error = ConversionError;
      fn try_from(value: Value) -> Result<Self, Self::Error> {
        if let Value::Enumeration(as_enum) = value {
          if *as_enum.id.as_bytes() == #enum_const_id_ident {
            match *as_enum.variant_id.as_bytes() {
              #(#from_value_cases)*
              _ => Err(Self::Error { message: "unexpected variant".to_string() }),
            }
          } else {
            Err(Self::Error {
              message: "unexpected enum type ID".to_string(),
            })
          }
        } else {
          Err(Self::Error {
            message: "unexpected kind".to_string(),
          })
        }
      }
    }
  };

  // Putting it all together.
  let type_source = quote! {
    #uses
    #enum_declaration
    #serialization
    #deserialization
    #to_value
    #from_value
    #enum_id_declaration
    #(#variant_id_declarations)*
  };

  token_stream_to_file(
    format!(
      "{}/{}.rs",
      parent_path.replace('.', "/"),
      enumeration.name.to_case(Case::Snake)
    ),
    &type_source,
  )
}

pub fn generate_into_impl(type_ident: &Ident) -> TokenStream {
  quote! {
    impl Into<Box<[u8]>> for #type_ident {
      fn into(self) -> Box<[u8]> {
        let mut writer = BufferWriter::new();
        serialize_to_writer(&self, &mut writer);
        writer.finalize()
      }
    }
  }
}

pub fn generate_try_from_impl(type_ident: &Ident) -> TokenStream {
  quote! {
    impl TryFrom<&[u8]> for #type_ident {
      type Error = DeserializationError;

      fn try_from(buffer: &[u8]) -> Result<Self, Self::Error> {
        let mut reader = BufferReader::new(buffer);
        return deserialize_from_reader(&mut reader, true)
      }
    }
  }
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

pub fn type_ident(type_name: &String) -> Ident {
  format_ident!("{}", type_name.to_case(Case::UpperCamel))
}

pub fn struct_field_const_id_ident(struct_name: &String, field_name: &String) -> Ident {
  format_ident!(
    "{}_{}_FIELD_RAW_ID",
    struct_name.to_case(Case::ScreamingSnake),
    field_name.to_case(Case::ScreamingSnake)
  )
}

pub fn struct_field_ident(struct_name: &String, field_name: &String) -> TokenStream {
  format!(
    "{}::{}",
    struct_name.to_case(Case::UpperCamel),
    field_name.to_case(Case::UpperCamel)
  )
  .parse()
  .unwrap()
}

pub fn struct_field_intermediate_variable_ident(
  struct_name: &String,
  field_name: &String,
) -> Ident {
  format_ident!(
    "{}_{}",
    struct_name.to_case(Case::Snake),
    field_name.to_case(Case::Snake),
  )
}

pub fn enum_variant_ident(enum_name: &String, variant_name: &String) -> TokenStream {
  format!(
    "{}::{}",
    enum_name.to_case(Case::UpperCamel),
    variant_name.to_case(Case::UpperCamel),
  )
  .parse()
  .unwrap()
}

pub fn enum_variant_const_id_ident(enum_name: &String, variant_name: &String) -> Ident {
  format_ident!(
    "{}_{}_VARIANT_RAW_ID",
    enum_name.to_case(Case::ScreamingSnake),
    variant_name.to_case(Case::ScreamingSnake),
  )
}

pub fn function_const_id_ident(function_name: &String) -> Ident {
  format_ident!(
    "{}_FUNCTION_RAW_ID",
    function_name.to_case(Case::ScreamingSnake),
  )
}

pub fn function_param_const_id_ident(function_name: &String, param_name: &String) -> Ident {
  format_ident!(
    "{}_{}_PARAMETER_RAW_ID",
    function_name.to_case(Case::ScreamingSnake),
    param_name.to_case(Case::ScreamingSnake),
  )
}

pub fn variable_ident(name: &String) -> Ident {
  format_ident!("{}", name.to_case(Case::Snake))
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum CheckType {
  Yes,
  No,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum PrefixWithMod {
  Yes,
  No,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Public {
  Yes,
  No,
}

/// A helper to format a Uuid into an inlined byte array.
pub struct RawUuidValue<'a>(pub &'a Uuid);

impl<'a> Display for RawUuidValue<'a> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{:#04x?}", self.0.as_bytes())
  }
}

impl<'a> ToTokens for RawUuidValue<'a> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let new_tokens: TokenStream = self.to_string().parse().unwrap();
    tokens.extend(new_tokens);
  }
}

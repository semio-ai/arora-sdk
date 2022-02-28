
use std::{sync::Arc, fmt::Display};

use arora_index::Index;
use arora_module_core::{Reader, Asset, Writer};
use arora_schema::{ty::{low::{Type, Enumeration}, UNIT_ID, PRIMITIVE_IDS}, module::low::{ExportSymbol, TypeRef}};
use arora_vfs::{Entry, Directory, File};
use clap::Parser;
use convert_case::{Casing, Case};
use quote::{quote, __private::{TokenStream, Ident}, format_ident, ToTokens};
use tokio::io::{stdin, stdout, AsyncWriteExt};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[clap(long_about = None)]
pub struct Args {
  #[clap(short, long, name = "self-id")]
  pub self_id: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let mut index = Index::new();
  let mut types = Vec::new();
  let mut exports = Vec::new();
  let mut imports = Vec::new();

  let mut stdin = stdin();
  let mut reader = Reader::new(&mut stdin);
  while let Ok(Some(asset)) = reader.read().await {
    match asset {
      Asset::Type(symbol) => {
        index.add_type(symbol.clone());
        types.push(symbol);
      },
      Asset::ExportSymbol(symbol) => exports.push(symbol),
      Asset::ImportSymbol(symbol) => imports.push(symbol),
      _ => (),
    };
  }

  let mut out_dir = Arc::new(Directory::new());
  out_dir = out_dir.merge_with(generate_common_sources());
  for ty in &types {
    out_dir = out_dir.merge_with(generate_type_source(&ty));
  }
  out_dir = out_dir.merge_with(generate_function_exports_source(&exports, &index));
  out_dir = out_dir.merge_with(generate_mod_source(&types));

  let mut stdout = stdout();
  let mut writer = Writer::new(&mut stdout);
  writer
    .write::<arora_vfs::Entry>(Entry::Directory(out_dir))
    .await?;
  writer.end().await?;
  stdout.flush().await?;

  Ok(())
}

fn generate_common_sources() -> Arc<Directory> {
  let source = quote! {
    use derive_more::Display;

    #[derive(Display, Debug)]
    pub struct DeserializationError {}

    impl std::error::Error for DeserializationError {}
  };
  token_stream_to_file("error.rs".to_string(), &source)
}

fn generate_type_source(ty: &Type) -> Arc<Directory> {
  let tokens = match &ty.kind {
    arora_schema::ty::low::TypeKind::Structure(_) => {
        let struct_or_enum = quote! { struct };
        struct_or_enum
      },
    arora_schema::ty::low::TypeKind::Enumeration(enumeration) => {
      generate_enumeration_source_contents(&ty.id, &ty.name, &enumeration)
    },
    arora_schema::ty::low::TypeKind::Primitive(_) => return Arc::new(Directory::new())
  };
  token_stream_to_file(format!("{}.rs", ty.name.to_lowercase()), &tokens)
}

fn generate_enumeration_source_contents(id: &Uuid, name: &String, enumeration: &Enumeration) -> TokenStream {
  let uses = quote! {
    use arora_buffers::{BufferReader, TYPE_ENUMERATION, BufferWriter};
    use crate::arora_generated::error::DeserializationError;
  };

  // Actual enum declaration.
  let enum_name = name.to_case(Case::UpperCamel);
  let enum_ident = type_ident(&enum_name);
  let variants = enumeration.values.iter();
  let enum_contents = variants.clone()
    .map(|(_, variant)| {
      let variant_ident = format_ident!("{}", variant.name.to_case(Case::UpperCamel));
      quote! { #variant_ident, }
    });
  let enum_declaration = quote! {
    #[derive(Debug, PartialEq)]
    pub enum #enum_ident {
      #(#enum_contents)*
    }
  };

  // ID declarations.
  let enum_id = id.to_string();
  let enum_id_bytes = RawUuidValue(id);
  let enum_upper_name = format_ident!("{}", enum_name.to_uppercase());
  let enum_const_id_ident = format_ident!("{}_ENUM_RAW_ID", enum_upper_name);
  let enum_const_id_doc = format!("{}: {}", enum_name, enum_id);
  let enum_id_declaration = quote! {
    #[doc = #enum_const_id_doc]
    pub const #enum_const_id_ident: [u8; 16] = #enum_id_bytes;
  };

  let variant_id_declarations = variants.clone()
    .map(|(id, variant)| {
      let id_string = id.to_string();
      let id_bytes = RawUuidValue(id);
      let variant_const_id_ident = enum_variant_const_id_ident(&enum_name, &variant.name);
      let variant_doc = format!("{}: {}", enum_variant_ident(&enum_name, &variant.name).to_string(), id_string);
      quote! {
        #[doc = #variant_doc]
        pub const #variant_const_id_ident: [u8; 16] = #id_bytes;
      }
    });

  // Serialization.
  let serialization_match_branches = variants.clone()
    .map(|(_, variant)| {
      let variant_ident = enum_variant_ident(&enum_name, &variant.name);
      let id_ident = enum_variant_const_id_ident(&enum_name, &variant.name);
      quote! { 
        #variant_ident => #id_ident.as_slice(),
      }
    });

  let serialization = quote! {
    impl Into<Box<[u8]>> for #enum_ident {
      fn into(self) -> Box<[u8]> {
        let mut writer = BufferWriter::new();
        serialize_to_writer(&self, &mut writer);
        writer.finalize()
      }
    }

    pub fn serialize_to_writer(value: &#enum_ident, writer: &mut BufferWriter) {
      let enumeration_id = #enum_const_id_ident.as_slice();
      let variant_id = match value {
        #(#serialization_match_branches)*
      };
      writer.add_enumeration_value(enumeration_id, variant_id);
      writer.add_unit();
    }
  };

  // Deserialization.
  let deserialization_cases = variants
    .map(|(_, variant)| {
      let variant_const_id_ident = enum_variant_const_id_ident(&enum_name, &variant.name);
      let variant_ident = enum_variant_ident(&enum_name, &variant.name);
      quote! {
        if variant_raw_id == #variant_const_id_ident {
          Ok(#variant_ident)
        }
      }
    });

  let deserialization = quote! {
    impl TryFrom<&[u8]> for #enum_ident {
      type Error = DeserializationError;
    
      fn try_from(buffer: &[u8]) -> Result<Self, Self::Error> {
        let mut reader = BufferReader::new(buffer);
        return deserialize_from_reader(&mut reader)
      }
    }

    fn deserialize_from_reader(reader: &mut BufferReader) -> Result<#enum_ident, DeserializationError> {
      let type_raw_id_opt = reader.next_type();
      if type_raw_id_opt.is_none() {
        return Err(DeserializationError{})
      }
      if type_raw_id_opt.unwrap() != TYPE_ENUMERATION {
        return Err(DeserializationError{})
      }
      if #enum_const_id_ident != reader.get_structure_field() {
        return Err(DeserializationError{})
      }
  
      let variant_raw_id = reader.get_enumeration_value_raw();
      return #(#deserialization_cases) else* else {
        Err(DeserializationError{})
      }
    }
  };
  
  // Putting it all together.
  let type_source = quote! {
    #uses
    #enum_declaration
    #serialization
    #deserialization
    #enum_id_declaration
    #(#variant_id_declarations)*
  };
  return type_source;
}

fn generate_function_exports_source(exports: &Vec<ExportSymbol>, index: &Index) -> Arc<Directory> {
  // Types used by function exports.
  // May differ from the full list of type dependencies,
  // because some uses of types are internal to the module.
  let use_type_mods = exports.iter()
    .flat_map(|ExportSymbol::Function(function_symbol)| {
      function_symbol.type_dependencies()
    })
    .filter_map(|type_id| {
      if PRIMITIVE_IDS.contains(&type_id) {
        None
      } else {
        let ty = index.find_type(&type_id).unwrap();
        let type_mod_ident = type_mod_ident(&ty.name);
        Some(quote! { #type_mod_ident })
      }
    });

  // Using the function implementations we expect to find at the root of the crate.
  let use_functions = exports.iter()
    .map(|export| {
      let ExportSymbol::Function(function_symbol) = export;
      format_ident!("{}", function_symbol.name)
    });
  
  // The declarations of the functions we export as an Arora module.
  let function_declarations = exports.iter()
    .map(|export| {
      let function_ident = format_ident!("{}", export.name());
      let ExportSymbol::Function(function_symbol) = export;
      let call_and_write_result = match function_symbol.ret {
        TypeRef::Scalar { id } => {
          if id == *UNIT_ID {
            quote! {
              #function_ident ();
              writer.add_unit();
            }
          } else {
            let return_type = index.find_type(&id).unwrap();
            let return_type_mod_ident = type_mod_ident(&return_type.name);
            quote! {
              let result = #function_ident ();
              #return_type_mod_ident :: serialize_to_writer(&result, &mut writer);
            }
          }
        },
        _ => quote!{}
      };
      let uuid_suffix = export.id().to_string().replace("-", "_");
      let arora_function_ident = format_ident!("arora_function_{}", uuid_suffix);
      quote! {
        #[no_mangle]
        pub extern "C" fn #arora_function_ident (_: i32) -> i32 {
          let mut writer = BufferWriter::new();
          #call_and_write_result
          let result_buffer = writer.finalize();
          result_buffer.as_ptr() as i32
        }
      }
    });

  // Putting it all together.
  let source = quote! {
    use arora_buffers::BufferWriter;
    use crate::{arora_generated::{#(#use_type_mods),*}, #(#use_functions),*};
    #(#function_declarations)*
  };
  token_stream_to_file("export.rs".to_string(), &source)
}

fn generate_mod_source(types: &Vec<Type>) -> Arc<Directory> {
  let type_mods = types.iter()
    .map(|ty| {
      let type_mod_ident = type_mod_ident(&ty.name);
      quote! { #type_mod_ident }
    });
  let source = quote! {
    pub mod error;
    #(pub mod #type_mods;)*
    pub mod export;
  };
  token_stream_to_file("mod.rs".to_string(), &source)
}

fn token_stream_to_file(file_name: String, tokens: &TokenStream) -> Arc<Directory> {
  let mut output = Directory::new();
  output.insert(file_name, File::new(tokens.to_string()));
  Arc::new(output)
}

fn type_mod_ident(type_name: &String) -> Ident {
  format_ident!("{}", type_name.to_lowercase())
}

fn type_ident(type_name: &String) -> Ident {
  format_ident!("{}", type_name.to_case(Case::UpperCamel))
}

fn enum_variant_ident(enum_name: &String, variant_name: &String) -> TokenStream {
  let enum_camel_name = enum_name.to_case(Case::UpperCamel);
  let variant_camel_name = variant_name.to_case(Case::UpperCamel);
  format!("{}::{}", enum_camel_name, variant_camel_name).parse().unwrap()
}

fn enum_variant_const_id_ident(enum_name: &String, variant_name: &String) -> Ident {
  let enum_upper_name = enum_name.to_uppercase();
  let variant_upper_name = variant_name.to_uppercase();
  format_ident!("{}_{}_VARIANT_RAW_ID", enum_upper_name, variant_upper_name)
}

struct RawUuidValue<'a>(&'a Uuid);

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

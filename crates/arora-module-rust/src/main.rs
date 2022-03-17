use std::{
  collections::HashSet,
  fmt::{Debug, Display},
  sync::Arc,
};

use arora_index::Index;
use arora_module_core::{Asset, Reader, Writer};
use arora_schema::{
  module::low::{ExportSymbol, ImportSymbol, TypeRef, Parameter},
  ty::{
    low::{Enumeration, Structure, Type, TypeKind},
    BOOLEAN_ID, F32_ID, F64_ID, I16_ID, I32_ID, I64_ID, I8_ID, PRIMITIVE_IDS, STRING_ID, U16_ID,
    U32_ID, U64_ID, U8_ID, UNIT_ID,
  },
};
use arora_vfs::{Directory, Entry, File};
use clap::Parser;
use convert_case::{Case, Casing};
use itertools::Itertools;
use quote::{
  __private::{Ident, TokenStream},
  format_ident, quote, ToTokens,
};
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
      }
      Asset::ExportSymbol(symbol) => exports.push(symbol),
      Asset::ImportSymbol(symbol) => imports.push(symbol),
      Asset::Header(header) => index.add_module(&header)?,
    };
  }

  let mut out_dir = Arc::new(Directory::new());
  out_dir = out_dir.merge_with(generate_common_sources());
  for ty in &types {
    out_dir = out_dir.merge_with(generate_type_source(&ty, &index));
  }
  out_dir = out_dir.merge_with(generate_imports_source(&imports, &index));
  out_dir = out_dir.merge_with(generate_exports_source(&exports, &index));
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
    pub struct DeserializationError {
      #[display(fmt = "deserialization error: {}", message)]
      pub message: String,
    }

    impl std::error::Error for DeserializationError {}
  };
  token_stream_to_file("error.rs".to_string(), &source)
}

fn generate_type_source(ty: &Type, index: &Index) -> Arc<Directory> {
  let tokens = match &ty.kind {
    arora_schema::ty::low::TypeKind::Structure(structure) => {
      generate_struct_source_contents(&ty.id, &ty.name, &structure, &index)
    }
    arora_schema::ty::low::TypeKind::Enumeration(enumeration) => {
      generate_enumeration_source_contents(&ty.id, &ty.name, &enumeration)
    }
    arora_schema::ty::low::TypeKind::Primitive(_) => return Arc::new(Directory::new()),
  };
  token_stream_to_file(format!("{}.rs", ty.name.to_case(Case::Snake)), &tokens)
}

fn generate_struct_source_contents(
  id: &Uuid,
  name: &String,
  structure: &Structure,
  index: &Index,
) -> TokenStream {
  // Struct uses, based on dependencies.
  let deps = structure.type_dependencies();
  let uses = deps.iter().filter_map(|dep_id| {
    let ty = index.find_type(&dep_id).unwrap();
    if let TypeKind::Primitive(_) = ty.kind {
      None
    } else {
      let mod_ident = type_mod_ident(&ty.name);
      let type_ident = type_ident(&ty.name);
      Some(quote! { use #mod_ident :: #type_ident })
    }
  });

  // Struct declaration.
  let struct_ident = type_ident(&name);
  let field_declarations = structure.fields.iter().map(|(_, field)| {
    let field_ident = variable_ident(&field.name);
    let field_type_ident = type_ident_from_ref(&field.type_ref, &index, PrefixWithMod::Yes);
    quote! { pub #field_ident: #field_type_ident }
  });
  let struct_declaration = quote! {
    pub struct #struct_ident {
      #(#field_declarations),*
    }
  };

  // Struct IDs.
  let id_str = id.to_string();
  let id_bytes = RawUuidValue(id);
  let upper_name = format_ident!("{}", name.to_case(Case::ScreamingSnake));
  let const_id_ident = format_ident!("{}_STRUCT_RAW_ID", upper_name);
  let const_id_doc = format!("{}: {}", name, id_str);
  let id_declaration = quote! {
    #[doc = #const_id_doc]
    pub const #const_id_ident: [u8; 16] = #id_bytes;
  };

  let field_id_declarations = structure.fields.iter().map(|(field_id, field)| {
    let field_id_bytes = RawUuidValue(field_id);
    let field_const_id_ident = struct_field_const_id_ident(&name, &field.name);
    let field_doc = format!(
      "{}: {}",
      struct_field_ident(&name, &field.name).to_string(),
      field_id.to_string(),
    );
    quote! {
      #[doc = #field_doc]
      pub const #field_const_id_ident: [u8; 16] = #field_id_bytes;
    }
  });

  // Struct Serialization.
  let fields_serialization = structure.fields.iter().map(|(_, field)| {
    let field_const_id_ident = struct_field_const_id_ident(&name, &field.name);
    let field_ident = variable_ident(&field.name);
    let value_expression = quote! { value.#field_ident };
    let serialize = generate_serialize_from_type_ref(&field.type_ref, value_expression, &index);
    quote! {
      writer.add_structure_field(&#field_const_id_ident);
      #serialize
    }
  });
  let field_count = fields_serialization.len() as u32;

  let into_impl = generate_into_impl(&struct_ident);
  let serialization = quote! {
    #into_impl

    pub fn serialize_to_writer(value: &#struct_ident, writer: &mut BufferWriter) {
      let structure_id = #const_id_ident.as_slice();
      writer.begin_structure(structure_id, #field_count);
      #(#fields_serialization)*
    }
  };

  // Struct Deserialization.
  // We convert each field we read into an optional,
  // then we move all of them into the result structure.
  let field_variable_declarations = structure.fields.iter().map(|(_, field)| {
    let variable_ident = struct_field_intermediate_variable_ident(&name, &field.name);
    let type_ident = type_ident_from_ref(&field.type_ref, &index, PrefixWithMod::No);
    quote! { let mut #variable_ident: Option<#type_ident> = None; }
  });

  let deserialization_cases = structure.fields.iter().map(|(_, field)| {
    let field_const_id_ident = struct_field_const_id_ident(&name, &field.name);
    let field_variable_ident = struct_field_intermediate_variable_ident(&name, &field.name);
    let deserialize = generate_deserialize_from_type_ref(
      &field.type_ref,
      &index,
      PrefixWithMod::No,
      CheckType::Yes,
    );
    quote! {
      if field_raw_id == #field_const_id_ident {
        #field_variable_ident = Some(#deserialize);
      }
    }
  });

  let struct_field_assignment = structure.fields.iter().map(|(_, field)| {
    let field_ident = variable_ident(&field.name);
    let variable_ident = struct_field_intermediate_variable_ident(&name, &field.name);
    quote! { #field_ident: #variable_ident.unwrap() }
  });

  let try_from_impl = generate_try_from_impl(&struct_ident);
  let expected_field_count = structure.fields.len();
  let deserialization = quote! {
    #try_from_impl

    pub fn deserialize_from_reader(reader: &mut BufferReader, check_type: bool) -> Result<#struct_ident, DeserializationError> {
      let field_count = if check_type {
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
          return Err(DeserializationError{ message: "missing next type information".to_string() })
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
          return Err(DeserializationError{ message: "next type is not a structure".to_string() })
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if #const_id_ident != structure_raw_id {
          return Err(DeserializationError{ message: "structure id does not match".to_string() })
        }
        field_count
      } else {
        reader.get_structure_raw()
      };
      if #expected_field_count != field_count as usize {
        return Err(DeserializationError{
          message: format!("expected {} fields, found {}", #expected_field_count, field_count)
        })
      }

      #(#field_variable_declarations)*
      for _ in 0..field_count {
        let field_raw_id = reader.get_structure_field();
        #(#deserialization_cases) else* else {
          return Err(DeserializationError {
            message: format!("unexpected struct field {}", Uuid::from_slice(field_raw_id).unwrap().to_string())
          })
        }
      }

      Ok(#struct_ident {
        #(#struct_field_assignment,)*
      })
    }
  };

  quote! {
    use arora_buffers::*;
    use uuid::Uuid;
    use crate::arora_generated::error::DeserializationError;
    #(#uses:)*
    #struct_declaration
    #serialization
    #deserialization
    #id_declaration
    #(#field_id_declarations)*
  }
}

fn generate_enumeration_source_contents(
  id: &Uuid,
  name: &String,
  enumeration: &Enumeration,
) -> TokenStream {
  // Enum uses.
  let uses = quote! {
    use arora_buffers::*;
    use crate::arora_generated::error::DeserializationError;
  };

  // Enum declaration.
  let enum_name = name.to_case(Case::UpperCamel);
  let enum_ident = type_ident(&enum_name);
  let variants = enumeration.values.iter();
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
  let deserialization_cases = variants.map(|(_, variant)| {
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
      return #(#deserialization_cases) else* else {
        Err(DeserializationError{ message: "unexpected variant".to_string() })
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

fn generate_exports_source(exports: &Vec<ExportSymbol>, index: &Index) -> Arc<Directory> {
  // Function Uses.
  let use_functions = exports.iter().map(|export| {
    let ExportSymbol::Function(function_symbol) = export;
    format_ident!("{}", function_symbol.name)
  });

  // Function and param IDs.
  let function_ids = exports.iter().flat_map(|export| {
    let ExportSymbol::Function(function_symbol) = export;
    let mut id_declarations = Vec::with_capacity(function_symbol.parameters.len() + 1);
    id_declarations.push(generate_const_id_declaration(
      &function_symbol.name,
      &function_const_id_ident(&function_symbol.name),
      &function_symbol.id,
      Public::Yes,
    ));
    for param in &function_symbol.parameters {
      id_declarations.push(generate_const_id_declaration(
        &format!("{}.{}", &function_symbol.name, &param.name),
        &function_param_const_id_ident(&function_symbol.name, &param.name),
        &param.id,
        Public::Yes,
      ));
    }
    id_declarations
  });

  // Functions declarations exported for Arora.
  let function_declarations = exports.iter().map(|export| {
    let function_ident = format_ident!("{}", export.name());
    let ExportSymbol::Function(function_symbol) = export;
    let const_id_ident = function_const_id_ident(&function_symbol.name);

    let call_check = quote! {
      let mut reader = BufferReader::new(&input);
      let type_raw_id_opt = reader.next_type();
      assert!(!type_raw_id_opt.is_none());
      assert_eq!(type_raw_id_opt.unwrap(), TYPE_STRUCTURE);
      let (structure_raw_id, field_count) = reader.get_structure();
      assert_eq!(#const_id_ident, structure_raw_id);
    };

    let param_declarations = function_symbol.parameters.iter().map(|param| {
      let param_var_ident = param_ident(&param);
      let param_type_ident = type_ident_from_ref(&param.ty, &index, PrefixWithMod::Yes);
      quote! { let mut #param_var_ident: Option<#param_type_ident> = None; }
    });

    let deserialization_cases = function_symbol.parameters.iter().map(|param| {
      let param_const_id_ident = function_param_const_id_ident(&function_symbol.name, &param.name);
      let param_var_ident = param_ident(&param);
      let deserialize =
        generate_deserialize_from_type_ref(&param.ty, &index, PrefixWithMod::Yes, CheckType::Yes);
      quote! {
        if field_raw_id == #param_const_id_ident {
          #param_var_ident = Some(#deserialize);
        }
      }
    });

    let deserialize_params = if function_symbol.parameters.is_empty() {
      quote! {
        assert_eq!(0, field_count);
      }
    } else {
      quote! {
        #(#param_declarations)*
        for _ in 0..field_count {
          let field_raw_id = reader.get_structure_field();
          #(#deserialization_cases else)* {
            panic!("buffer contains an unexpected parameter: {:?}", field_raw_id);
          }
        }
      }
    };

    let param_args = function_symbol.parameters.iter().map(|param| {
      let param_var_ident = param_ident(&param);
      if param.mutable {
        quote! { &mut #param_var_ident }
      } else {
        quote! { #param_var_ident }
      }
    });

    let call_and_write_result = (|| {
      let result_ident = match function_symbol.ret {
        TypeRef::Scalar { id } if id == *UNIT_ID => quote! { _ },
        _ => quote! { result },
      };
      let serialize_result =
        generate_serialize_from_type_ref(&function_symbol.ret, result_ident.clone(), &index);
      quote! {
        let #result_ident = #function_ident (#(#param_args),*);
        #serialize_result;
      }
    })();

    let write_mutated_params: Vec<TokenStream> = function_symbol
      .parameters
      .iter()
      .filter_map(|param| {
        if param.mutable {
          let param_var_ident = param_ident(&param);
          let param_const_id_ident =
            function_param_const_id_ident(&function_symbol.name, &param.name);
          let serialize_param =
            generate_serialize_from_type_ref(&param.ty, quote! {#param_var_ident.unwrap()}, &index);
          Some(quote! {
            writer.add_structure_field(&#param_const_id_ident);
            #serialize_param;
          })
        } else {
          None
        }
      })
      .collect();
    let nof_mutated_params = write_mutated_params.len();

    let uuid_suffix = export.id().to_string().replace("-", "_");
    let arora_function_ident = format_ident!("arora_function_{}", uuid_suffix);
    let doc = format!("{}", function_symbol.name);
    quote! {
      #[doc = #doc]
      #[no_mangle]
      pub extern "C" fn #arora_function_ident (input_addr: i32) -> i32 {
        let input_ptr = input_addr as *const u8;
        const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
        let input_size_bytes: &[u8; 4] = unsafe {
          std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE)
        }.try_into().expect("input is too small");
        let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
        let input = unsafe {
          std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size)
        };
        #call_check
        #deserialize_params
        let mut writer = BufferWriter::new();
        writer.begin_structure(&#const_id_ident, (#nof_mutated_params + 1) as u32);
        writer.add_structure_field(&#const_id_ident);
        #call_and_write_result
        #(#write_mutated_params)*
        let result_buffer = writer.finalize();
        Box::leak(result_buffer).as_ptr() as i32
      }
    }
  });

  // Putting it all together.
  let source = quote! {
    use arora_buffers::*;
    use crate::{arora_generated, arora_generated::error::DeserializationError, #(#use_functions),*};
    #(#function_declarations)*
    #(#function_ids)*
  };
  token_stream_to_file("export.rs".to_string(), &source)
}

/// Generates a virtual source file with wrappers for every symbol imported by the module.
/// It contains human-readable public functions that can be used by the module implementation,
/// under the module `import`, as `<module>::<function>`.
fn generate_imports_source(imports: &Vec<ImportSymbol>, index: &Index) -> Arc<Directory> {
  // Using dependent types.
  let type_dependencies = HashSet::<Uuid>::from_iter(
    imports
      .into_iter()
      .flat_map(|import| import.type_dependencies()),
  );
  let uses = type_dependencies.into_iter().filter_map(|ref type_id| {
    if PRIMITIVE_IDS.contains(type_id) {
      None
    } else {
      let type_ident = type_ident_from_id(type_id, index, PrefixWithMod::Yes);
      Some(quote! {
        use crate::arora_generated::#type_ident;
      })
    }
  });

  // Sort imports by module, so that to declare them together.
  let imports_by_module = imports.into_iter().group_by(|import| import.module());

  // For each module, declare a mod.
  let mod_declarations = imports_by_module
    .into_iter()
    .map(|(module_id, module_imports)| {
      let module = index
        .modules
        .get(module_id)
        .expect(format!("importing symbol from unknown module {}", module_id).as_str());
      let module_ident = format_ident!("{}", module.name.to_case(Case::Snake));

      // Declare the ID of the module to use it locally.
      let module_const_id_ident =
        format_ident!("{}_MODULE_ID", module.name.to_case(Case::ScreamingSnake),);
      let module_id_declaration =
        generate_const_id_declaration(&module.name, &module_const_id_ident, &module.id, Public::No);

      // For each import, declare a function.
      let functions_declarations = module_imports.map(|import_symbol| {
        let ImportSymbol::Function(function_symbol) = import_symbol;
        let function_ident = format_ident!("{}", function_symbol.name);
        let parameters_declarations = function_symbol.parameters.iter().map(|param| {
          let maybe_mut = if param.mutable {
            quote! { mut }
          } else {
            quote! {}
          };
          let param_name_ident = format_ident!("{}", param.name);
          let param_type_ident = type_ident_from_ref(&param.ty, index, PrefixWithMod::Yes);
          quote! { #maybe_mut #param_name_ident: #param_type_ident }
        });
        let ret_type_ident = type_ident_from_ref(&function_symbol.ret, index, PrefixWithMod::Yes);

        // And implement the call.
        // First declare the const ids.
        let function_const_id_ident = function_const_id_ident(&function_symbol.name);
        let function_id_declaration = generate_const_id_declaration(
          &function_symbol.name,
          &function_const_id_ident,
          &function_symbol.id,
          Public::No,
        );
        let param_ids_declarations = function_symbol.parameters.iter().map(|param| {
          generate_const_id_declaration(
            &format!("{}.{}", &function_symbol.name, &param.name),
            &function_param_const_id_ident(&function_symbol.name, &param.name),
            &param.id,
            Public::No,
          )
        });
        let ids_declaration = quote! {
          #function_id_declaration
          #(#param_ids_declarations)*
        };

        // Then prepare a call argument structure.
        // It consists in a struct with the function id as id,
        // and with one field for each param.
        let add_args = function_symbol.parameters.iter().map(|param| {
          let function_param_const_id_ident =
            function_param_const_id_ident(&function_symbol.name, &param.name);
          let param_name_ident = format_ident!("{}", param.name);
          let serialize_arg = generate_serialize_from_type_ref(
            &param.ty,
            param_name_ident.into_token_stream(),
            &index,
          );
          quote! {
            writer.add_structure_field(#function_param_const_id_ident.as_slice());
            #serialize_arg;
          }
        });
        let nof_args = add_args.len() as u32;
        let prepare_call_structure = quote! {
          let mut writer = BufferWriter::new();
          writer.begin_structure(#function_const_id_ident.as_slice(), #nof_args);
          #(#add_args)*
          let arg = writer.finalize();
        };

        // Then perform the call.
        let perform_call = quote! {
          let result_buffer_addr = unsafe {
            arora_dispatch(
              #module_const_id_ident.as_ptr() as i32,
              #function_const_id_ident.as_ptr() as i32,
              arg.as_ptr() as i32,
            )
          };
        };

        // Then parse the result.
        let prepare_parsing = quote! {
          let result_buffer_ptr = result_buffer_addr as *const u8;
          const BUFFER_SIZE_SIZE: usize = std::mem::size_of::<u32>();
          let input_size_bytes: &[u8; 4] =
            unsafe { std::slice::from_raw_parts(result_buffer_ptr, BUFFER_SIZE_SIZE) }
              .try_into()
              .expect("input is too small");
          let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
          let input =
            unsafe { std::slice::from_raw_parts(result_buffer_ptr, BUFFER_SIZE_SIZE + input_size) };
          let mut reader = BufferReader::new(&input);
        };

        // It consists in a struct with the function id as id,
        let check_result_struct = quote! {
          let type_raw_id_opt = reader.next_type();
          assert!(!type_raw_id_opt.is_none());
          assert_eq!(type_raw_id_opt.unwrap(), TYPE_STRUCTURE);
          let (result_struct_id, result_field_count) = reader.get_structure();
          assert_eq!(result_struct_id, #function_const_id_ident);
        };

        // with one field for the return value,
        // plus one field for each param.
        // Mutate the mutable parameters
        // and return.
        let deserialize_ret = generate_deserialize_from_type_ref(
          &function_symbol.ret,
          index,
          PrefixWithMod::Yes,
          CheckType::Yes,
        );

        let process_params = if nof_args > 1 {
          let declare_mutable_params = function_symbol.parameters.iter().filter_map(|param| {
            if param.mutable {
              let param_name_ident = format_ident!("{}", param.name);
              Some(quote! {
                let mut #param_name_ident = None;
              })
            } else {
              None
            }
          });
          
          let deserialize_params = function_symbol.parameters.iter().filter_map(|param| {
            if param.mutable {
              let param_name_ident = format_ident!("{}", param.name);
              let function_param_const_id_ident =
                function_param_const_id_ident(&function_symbol.name, &param.name);
              let deserialize_param = generate_deserialize_from_type_ref(
                &param.ty,
                index,
                PrefixWithMod::Yes,
                CheckType::Yes,
              );
              Some(quote! {
                x if *x == #function_param_const_id_ident => *#param_name_ident = #deserialize_param,
              })
            } else {
              None
            }
          });

          quote! {
            #(#declare_mutable_params)*
            for _i in 1u32..#nof_args {
              let next_field_id = reader.get_structure_field();
              match next_field_id {
                #(#deserialize_params)*
                x => panic!("found unexpected mutated argument id: {:#?}", x),
              }
            }
          }
        } else {
          quote! {}
        };

        let process_result = quote! {
          assert_eq!(result_field_count, #nof_args);
          let first_field_id = reader.get_structure_field();
          assert_eq!(first_field_id, #function_const_id_ident);
          let ret = #deserialize_ret;
          #process_params
          ret
        };

        // This makes an import function.
        quote! {
          pub fn #function_ident (#(#parameters_declarations),*) -> #ret_type_ident {
            #ids_declaration
            #prepare_call_structure
            #perform_call
            #prepare_parsing
            #check_result_struct
            #process_result
          }
        }
      });

      // Also declare arora engine functions.
      let engine_functions_declarations = quote! {
        #[link(wasm_import_module = "env")]
        extern "C" {
          pub fn arora_dispatch(module_id: i32, method_id: i32, arg: i32) -> i32;
          pub fn arora_dispatch_indirect(callable_id: u64) -> i32;
        }
      };

      // This makes a module import.
      quote! {
        pub mod #module_ident {
          use arora_buffers::*;
          use crate::{arora_generated, arora_generated::error::DeserializationError};
          use super::arora_dispatch;
          #module_id_declaration
          #(#functions_declarations)*
        }
        #engine_functions_declarations
      }
    });

  // This makes the import source file.
  let source = quote! {
    #(#uses)*
    #(#mod_declarations)*
  };
  token_stream_to_file("import.rs".to_string(), &source)
}

fn generate_mod_source(types: &Vec<Type>) -> Arc<Directory> {
  let type_mods = types.iter().map(|ty| {
    let type_mod_ident = type_mod_ident(&ty.name);
    quote! { #type_mod_ident }
  });
  let source = quote! {
    pub mod error;
    #(pub mod #type_mods;)*
    pub mod import;
    pub mod export;
  };
  token_stream_to_file("mod.rs".to_string(), &source)
}

fn generate_into_impl(type_ident: &Ident) -> TokenStream {
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

fn generate_try_from_impl(type_ident: &Ident) -> TokenStream {
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

fn generate_serialize_from_id(
  id: &Uuid,
  value_expression: TokenStream,
  index: &Index,
) -> TokenStream {
  match id {
    x if *x == *UNIT_ID => quote! { writer.add_unit() },
    x if *x == *BOOLEAN_ID => quote! { writer.add_boolean(#value_expression) },
    x if *x == *U8_ID => quote! { writer.add_u8(#value_expression) },
    x if *x == *U16_ID => quote! { writer.add_u16(#value_expression) },
    x if *x == *U32_ID => quote! { writer.add_u32(#value_expression) },
    x if *x == *U64_ID => quote! { writer.add_u64(#value_expression) },
    x if *x == *I8_ID => quote! { writer.add_i8(#value_expression) },
    x if *x == *I16_ID => quote! { writer.add_i16(#value_expression) },
    x if *x == *I32_ID => quote! { writer.add_i32(#value_expression) },
    x if *x == *I64_ID => quote! { writer.add_i64(#value_expression) },
    x if *x == *F32_ID => quote! { writer.add_f32(#value_expression) },
    x if *x == *F64_ID => quote! { writer.add_f64(#value_expression) },
    x if *x == *STRING_ID => quote! { writer.add_string(#value_expression) },
    x => {
      let ty = index.find_type(&x).unwrap();
      let type_mod_ident = type_mod_ident(&ty.name);
      quote! { arora_generated::#type_mod_ident ::serialize_to_writer(&#value_expression, &mut writer) }
    }
  }
}

fn generate_serialize_from_type_ref(
  type_ref: &TypeRef,
  value_expression: TokenStream,
  index: &Index,
) -> TokenStream {
  match type_ref {
    TypeRef::Scalar { id } => generate_serialize_from_id(&id, value_expression, &index),
    TypeRef::Array { id } => {
      let id_bytes = RawUuidValue(id);
      let add_array_args = quote! { #id_bytes, #value_expression.len() };
      let serialize_array = match id {
        x if *x == *UNIT_ID => panic!("arrays of unit are not supported"),
        x if *x == *BOOLEAN_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_boolean_bulk(#value_expression);
        },
        x if *x == *U8_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_u8_bulk(#value_expression);
        },
        x if *x == *U16_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_u16_bulk(#value_expression);
        },
        x if *x == *U32_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_u32_bulk(#value_expression);
        },
        x if *x == *U64_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_u64_bulk(#value_expression);
        },
        x if *x == *I8_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_i8_bulk(#value_expression);
        },
        x if *x == *I16_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_i16_bulk(#value_expression);
        },
        x if *x == *I32_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_i32_bulk(#value_expression);
        },
        x if *x == *I64_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_i64_bulk(#value_expression);
        },
        x if *x == *F32_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_f32_bulk(#value_expression);
        },
        x if *x == *F64_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          writer.add_f64_bulk(#value_expression);
        },
        x if *x == *STRING_ID => quote! {
          writer.add_array_primitive(#add_array_args);
          for s in #value_expression {
            writer.add_string(s);
          }
        },
        x => {
          let prepare_array = match index.find_type(&x).unwrap().kind {
            TypeKind::Structure(_) => {
              quote! { writer.add_array_structure(#add_array_args); }
            }
            TypeKind::Enumeration(_) => {
              quote! { writer.add_array_enumeration(#add_array_args); }
            }
            TypeKind::Primitive(_) => {
              panic!("primitive case has already been dismissed")
            }
          };
          let serialize_element = generate_serialize_from_id(x, quote! { element }, &index);
          quote! {
            #prepare_array
            for element in #value_expression {
              #serialize_element;
            }
          }
        }
      };
      quote! {
        (|| {
          #serialize_array
        })()
      }
    }
    TypeRef::Map {
      key_id: _,
      value_id: _,
    } => {
      todo!("not implemented");
    }
  }
}

fn generate_deserialize_from_id(id: &Uuid, index: &Index, check_type: CheckType) -> TokenStream {
  let type_kind_ident = type_kind_ident(&id, &index);
  let type_check = match check_type {
    CheckType::Yes => quote! {
      assert_eq!(reader.next_type(), Some(#type_kind_ident));
    },
    CheckType::No => quote! {},
  };
  let deserialization = match id {
    x if *x == *UNIT_ID => quote! { Result::<(), DeserializationError>::Ok(reader.get_unit()) },
    x if *x == *BOOLEAN_ID => {
      quote! {
        #type_check
        Result::<bool, DeserializationError>::Ok(reader.get_boolean())
      }
    }
    x if *x == *U8_ID => quote! {
      #type_check
      Result::<u8, DeserializationError>::Ok(reader.get_u8())
    },
    x if *x == *U16_ID => quote! {
      #type_check
      Result::<u16, DeserializationError>::Ok(reader.get_u16())
    },
    x if *x == *U32_ID => quote! {
      #type_check
      Result::<u32, DeserializationError>::Ok(reader.get_u32())
    },
    x if *x == *U64_ID => quote! {
      #type_check
      Result::<u64, DeserializationError>::Ok(reader.get_u64())
    },
    x if *x == *I8_ID => quote! {
      #type_check
      Result::<i8, DeserializationError>::Ok(reader.get_i8())
    },
    x if *x == *I16_ID => quote! {
      #type_check
      Result::<i16, DeserializationError>::Ok(reader.get_i16())
    },
    x if *x == *I32_ID => quote! {
      #type_check
      Result::<i32, DeserializationError>::Ok(reader.get_i32())
    },
    x if *x == *I64_ID => quote! {
      #type_check
      Result::<i64, DeserializationError>::Ok(reader.get_i64())
    },
    x if *x == *F32_ID => quote! {
      #type_check
      Result::<f32, DeserializationError>::Ok(reader.get_f32())
    },
    x if *x == *F64_ID => quote! {
      #type_check
      Result::<f64, DeserializationError>::Ok(reader.get_f64())
    },
    x if *x == *STRING_ID => {
      quote! {
        #type_check
        Result::<String, DeserializationError>::Ok(reader.get_string().to_string())
      }
    }
    x => {
      let ty = index.find_type(&x).unwrap();
      let type_mod_ident = type_mod_ident(&ty.name);
      let check_type = check_type == CheckType::Yes;
      quote! { arora_generated::#type_mod_ident ::deserialize_from_reader(&mut reader, #check_type) }
    }
  };
  quote! {
    (|| {
      #deserialization
    })()
  }
}

fn generate_deserialize_from_type_ref(
  type_ref: &TypeRef,
  index: &Index,
  with_mod: PrefixWithMod,
  check_type: CheckType,
) -> TokenStream {
  match type_ref {
    TypeRef::Scalar { id } => {
      let deserialize = generate_deserialize_from_id(&id, &index, check_type);
      let type_ident = type_ident_from_id(id, &index, with_mod);
      let type_str = type_ident.to_string();
      quote! {
        #deserialize
          .expect(format!("failed to deserialize value of type {}", #type_str).as_str())
      }
    }
    TypeRef::Array { id } => {
      let type_kind_ident = type_kind_ident(&id, &index);
      let array_check = quote! {
        assert_eq!(reader.next_type(), Some(TYPE_ARRAY));
        let (ty, count) = reader.get_array();
        assert_eq!(ty, #type_kind_ident);
      };
      let deserialize_array = match id {
        x if *x == *UNIT_ID => panic!("arrays of unit are not supported"),
        x if *x == *BOOLEAN_ID => quote! { reader.get_boolean_bulk(count) },
        x if *x == *U8_ID => quote! { reader.get_u8_bulk(count) },
        x if *x == *U16_ID => quote! { reader.get_u16_bulk(count) },
        x if *x == *U32_ID => quote! { reader.get_u32_bulk(count) },
        x if *x == *U64_ID => quote! { reader.get_u64_bulk(count) },
        x if *x == *I8_ID => quote! { reader.get_i8_bulk(count) },
        x if *x == *I16_ID => quote! { reader.get_i16_bulk(count) },
        x if *x == *I32_ID => quote! { reader.get_i32_bulk(count) },
        x if *x == *I64_ID => quote! { reader.get_i64_bulk(count) },
        x if *x == *F32_ID => quote! { reader.get_f32_bulk(count) },
        x if *x == *F64_ID => quote! { reader.get_f64_bulk(count) },
        x => {
          // STRING_ID case is almost the same
          let maybe_get_structure_field = if *x == *STRING_ID {
            quote! {}
          } else {
            let raw_id = RawUuidValue(x);
            quote! { assert_eq!(reader.get_structure_field(), #raw_id); }
          };
          let type_ident = type_ident_from_id(x, &index, with_mod);
          let type_str = type_ident.to_string();
          let deserialize_element = generate_deserialize_from_id(x, &index, CheckType::No);
          quote! {
            #maybe_get_structure_field
            let mut res = Vec::<#type_ident>::with_capacity(count as usize);
            for i in 0..count {
              res.push(
                #deserialize_element
                  .expect(format!("failed to deserialize item #{} of an array of {}", i, #type_str).as_str())
              );
            }
            res
          }
        }
      };
      quote! {
        (|| {
          #array_check
          #deserialize_array
        })()
      }
    }
    TypeRef::Map {
      key_id: _,
      value_id: _,
    } => {
      todo!("not implemented");
    }
  }
}

/// Generates
fn generate_const_id_declaration(
  name: &String,
  ident: &Ident,
  id: &Uuid,
  public: Public,
) -> TokenStream {
  let id_str = id.to_string();
  let id_bytes = RawUuidValue(id);
  let const_id_doc = format!("{}: {}", name, id_str);
  let maybe_pub = match public {
    Public::Yes => quote! { pub },
    Public::No => quote! {},
  };
  quote! {
    #[doc = #const_id_doc]
    #maybe_pub const #ident: [u8; 16] = #id_bytes;
  }
}

fn token_stream_to_file(file_name: String, tokens: &TokenStream) -> Arc<Directory> {
  let mut output = Directory::new();
  output.insert(file_name, File::new(tokens.to_string()));
  Arc::new(output)
}

fn type_mod_ident(type_name: &String) -> Ident {
  format_ident!("{}", type_name.to_case(Case::Snake))
}

fn type_ident_from_id(id: &Uuid, index: &Index, with_mod: PrefixWithMod) -> TokenStream {
  match id {
    x if *x == *UNIT_ID => quote! { () },
    x if *x == *BOOLEAN_ID => quote! { bool },
    x if *x == *U8_ID => quote! { u8 },
    x if *x == *U16_ID => quote! { u16 },
    x if *x == *U32_ID => quote! { u32 },
    x if *x == *U64_ID => quote! { u64 },
    x if *x == *I8_ID => quote! { i8 },
    x if *x == *I16_ID => quote! { i16 },
    x if *x == *I32_ID => quote! { i32 },
    x if *x == *I64_ID => quote! { i64 },
    x if *x == *F32_ID => quote! { f32 },
    x if *x == *F64_ID => quote! { f64 },
    x if *x == *STRING_ID => quote! { String },
    x => {
      let ty = index.find_type(&x).unwrap();
      let mod_prefix = match with_mod {
        PrefixWithMod::Yes => {
          let mod_ident = type_mod_ident(&ty.name);
          quote! { arora_generated::#mod_ident :: }
        }
        PrefixWithMod::No => quote! {},
      };
      let type_ident = type_ident(&ty.name);
      quote! { #mod_prefix #type_ident }
    }
  }
}

fn type_ident_from_ref(type_ref: &TypeRef, index: &Index, with_mod: PrefixWithMod) -> TokenStream {
  match type_ref {
    TypeRef::Scalar { id } => type_ident_from_id(&id, &index, with_mod),
    TypeRef::Array { id } => {
      let ty_ident = type_ident_from_id(&id, &index, with_mod);
      quote! { Vec<#ty_ident> }
    }
    TypeRef::Map { key_id, value_id } => {
      let key_ty_ident = type_ident_from_id(&key_id, &index, with_mod);
      let value_ty_ident = type_ident_from_id(&value_id, &index, with_mod);
      quote! { HashMap<#key_ty_ident, #value_ty_ident> }
    }
  }
}

fn type_ident(type_name: &String) -> Ident {
  format_ident!("{}", type_name.to_case(Case::UpperCamel))
}

fn type_kind_ident(id: &Uuid, index: &Index) -> TokenStream {
  match id {
    x if *x == *UNIT_ID => quote! { TYPE_UNIT },
    x if *x == *BOOLEAN_ID => quote! { TYPE_BOOLEAN },
    x if *x == *U8_ID => quote! { TYPE_U8 },
    x if *x == *U16_ID => quote! { TYPE_U16 },
    x if *x == *U32_ID => quote! { TYPE_U32 },
    x if *x == *U64_ID => quote! { TYPE_U64 },
    x if *x == *I8_ID => quote! { TYPE_I8 },
    x if *x == *I16_ID => quote! { TYPE_I16 },
    x if *x == *I32_ID => quote! { TYPE_I32 },
    x if *x == *I64_ID => quote! { TYPE_I64 },
    x if *x == *F32_ID => quote! { TYPE_F32 },
    x if *x == *F64_ID => quote! { TYPE_F64 },
    x if *x == *STRING_ID => quote! { TYPE_STRING },
    x => {
      let ty = index.find_type(&x).unwrap();
      match ty.kind {
        TypeKind::Structure(_) => quote! { TYPE_STRUCTURE },
        TypeKind::Enumeration(_) => quote! { TYPE_ENUMERATION },
        TypeKind::Primitive(_) => panic!("encountered unknown primitive type {}", x.to_string()),
      }
    }
  }
}

fn struct_field_const_id_ident(struct_name: &String, field_name: &String) -> Ident {
  format_ident!(
    "{}_{}_FIELD_RAW_ID",
    struct_name.to_case(Case::ScreamingSnake),
    field_name.to_case(Case::ScreamingSnake)
  )
}

fn struct_field_ident(struct_name: &String, field_name: &String) -> TokenStream {
  format!(
    "{}::{}",
    struct_name.to_case(Case::UpperCamel),
    field_name.to_case(Case::UpperCamel)
  )
  .parse()
  .unwrap()
}

fn struct_field_intermediate_variable_ident(struct_name: &String, field_name: &String) -> Ident {
  format_ident!(
    "{}_{}",
    struct_name.to_case(Case::Snake),
    field_name.to_case(Case::Snake),
  )
}

fn enum_variant_ident(enum_name: &String, variant_name: &String) -> TokenStream {
  format!(
    "{}::{}",
    enum_name.to_case(Case::UpperCamel),
    variant_name.to_case(Case::UpperCamel),
  )
  .parse()
  .unwrap()
}

fn enum_variant_const_id_ident(enum_name: &String, variant_name: &String) -> Ident {
  format_ident!(
    "{}_{}_VARIANT_RAW_ID",
    enum_name.to_case(Case::ScreamingSnake),
    variant_name.to_case(Case::ScreamingSnake),
  )
}

fn function_const_id_ident(function_name: &String) -> Ident {
  format_ident!(
    "{}_FUNCTION_RAW_ID",
    function_name.to_case(Case::ScreamingSnake),
  )
}

fn function_param_const_id_ident(function_name: &String, param_name: &String) -> Ident {
  format_ident!(
    "{}_{}_PARAMETER_RAW_ID",
    function_name.to_case(Case::ScreamingSnake),
    param_name.to_case(Case::ScreamingSnake),
  )
}

fn param_ident(param: &Parameter) -> Ident {
  let param_id_sanitized = param.id.to_string().replace("-", "");
  format_ident!("param_{}_{}", param.name.to_case(Case::Snake), param_id_sanitized)
}

fn variable_ident(name: &String) -> Ident {
  format_ident!("{}", name.to_case(Case::Snake))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CheckType {
  Yes,
  No,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PrefixWithMod {
  Yes,
  No,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Public {
  Yes,
  No,
}

/// A helper to format a Uuid into an inlined byte array.
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

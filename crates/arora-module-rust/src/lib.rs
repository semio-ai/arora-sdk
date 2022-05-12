pub mod rustfmt;

use arora_module_core::{
  header::generate_header_file, ImportAsset, ModuleAsset, ModuleDeclarationError,
};
use arora_registry::{
  EnumerationFrozen, ModuleFrozen, ReadableRegistry, RegistryError, StructureFrozen,
  TypeDefinitionFrozen,
};
use arora_schema::ty::{
  BOOLEAN_ID, F32_ID, F64_ID, I16_ID, I32_ID, I64_ID, I8_ID, STRING_ID, U16_ID, U32_ID, U64_ID,
  U8_ID,
};
use arora_vfs::{Directory, Entry as VfsEntry, File, VfsError};
use async_recursion::async_recursion;
use convert_case::{Case, Casing};
use derive_more::Display;
use quote::{
  __private::{Ident, TokenStream},
  format_ident, quote, ToTokens,
};
use semio_client::common::{RecordType, Selector};
use semio_record::ty::PrimitiveKind;
use semio_record::{
  module::v0::frozen::{ExportKind, Parameter},
  record::FrozenReference,
  ty::{FrozenScalar, FrozenTy, Primitive},
};
use semver::VersionReq;
use std::{
  collections::{hash_map::Entry, HashMap, HashSet},
  fmt::Display,
  path,
};
use uuid::Uuid;

/// Generates a set of sources organized in a virtual directory
/// from a set of assets as produced by [`arora_module_core::analyze_module`].
/// First, the types, then the modules, then the imports.
pub async fn generate_sources(
  assets: Vec<ModuleAsset>,
  registry: &mut dyn ReadableRegistry,
) -> Result<Directory, GenerationError> {
  let mut result = generate_common_sources()?;
  let mut imports_by_module: HashMap<Uuid, Vec<ImportAsset>> = HashMap::new();
  let mut current_module = Option::<(Uuid, ModuleFrozen, String)>::None;
  for asset in assets {
    match asset {
      ModuleAsset::Type(id, _, ty) => match ty {
        TypeDefinitionFrozen::Primitive(_) => (),
        TypeDefinitionFrozen::Enumeration(enumeration) => {
          let parent_path = registry
            .resolve_id(&enumeration.parent)
            .await
            .map_err(GenerationError::RegistryError)?;
          let enum_sources = generate_enumeration_source(&id, &enumeration, &parent_path)
            .map_err(GenerationError::VfsError)?;
          result = result.merge_with(&enum_sources);
        }
        TypeDefinitionFrozen::Structure(structure) => {
          let parent_path = registry
            .resolve_id(&structure.parent)
            .await
            .map_err(GenerationError::RegistryError)?;
          let struct_sources =
            generate_structure_source(&id, &structure, registry, &parent_path).await?;
          result = result.merge_with(&struct_sources);
        }
      },
      ModuleAsset::Import(import) => match imports_by_module.entry(import.module_id.to_owned()) {
        Entry::Occupied(mut entry) => entry.get_mut().push(import),
        Entry::Vacant(entry) => {
          entry.insert(vec![import]);
        }
      },
      ModuleAsset::Module(ref module_id, _, ref module, ref executor) => {
        let module_sources = generate_module_source(&module, registry).await?;
        result = result.merge_with(&module_sources);
        assert!(current_module.is_none()); // Only one module to generate at a time.
        current_module = Some((module_id.to_owned(), module.to_owned(), executor.to_owned()));
      }
    }
  }

  let mut all_imports = Vec::new();
  for (module_id, ref mut imports) in imports_by_module {
    // Generate bindings for imported functions.
    let module_path = registry
      .resolve_id(&module_id)
      .await
      .map_err(GenerationError::RegistryError)?;
    let imports_sources =
      generate_imports_from_module_source(&module_id, &module_path, imports, registry).await?;
    result = result.merge_with(&imports_sources);
    all_imports.append(imports);
  }

  // Produce the stripped `module.yaml` file.
  let current_module = current_module.unwrap();
  result = result.merge_with(
    &generate_header_file(&current_module.0, &current_module.1, &all_imports, &current_module.2)
      .map_err(GenerationError::ModuleDeclarationError)?,
  );

  // Also declare arora engine functions.
  let engine_functions_declarations = quote! {
    #[link(wasm_import_module = "env")]
    extern "C" {
      pub fn arora_dispatch(module_id: i32, method_id: i32, arg: i32) -> i32;
      pub fn arora_dispatch_indirect(callable_id: u64) -> i32;
    }
  };
  result = result.merge_with(
    &token_stream_to_file("arora.rs", &engine_functions_declarations)
      .map_err(GenerationError::VfsError)?,
  );

  // Add the `mod.rs` files.
  generate_mods_in_directories(&mut result)?;

  Ok(result)
}

/// Generates `mod.rs` files and adds them at every level of the directory hierarchy
/// where `.rs` files can be found. Returns true if it was generated.
pub fn generate_mods_in_directories(dir: &mut Directory) -> Result<bool, GenerationError> {
  let mut mods = Vec::new();
  for (path, entry) in dir.list_mut() {
    if let VfsEntry::Directory(ref mut dir) = entry {
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
      .map(|mod_name| format_ident!("{}", mod_name.to_case(Case::Snake)));
    let tokens = quote! {
      #(pub mod #mods;)*
    };
    dir
      .insert("mod.rs", File::new(tokens.to_string()))
      .map_err(GenerationError::VfsError)?;
    Ok(true)
  } else {
    Ok(false)
  }
}

/// Generates sources that are common dependencies to
/// other generated sources.
/// Always call this function before generating sources.
pub fn generate_common_sources() -> Result<Directory, GenerationError> {
  let source = quote! {
    use derive_more::Display;

    #[derive(Display, Debug)]
    pub struct DeserializationError {
      #[display(fmt = "deserialization error: {}", message)]
      pub message: String,
    }

    impl std::error::Error for DeserializationError {}
  };
  token_stream_to_file("error.rs".to_string(), &source).map_err(GenerationError::VfsError)
}

/// Generates a Rust source file for the given enumeration.
/// It contains the type declaration and some functions
/// to serialize and deserialize values.
/// It depends on `arora_buffers`, `arora_schema` and `uuid`.
pub fn generate_enumeration_source(
  id: &Uuid,
  enumeration: &EnumerationFrozen,
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
    #[derive(Debug, PartialEq, Clone)]
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

  let base_file_name = enumeration.name.to_case(Case::Snake);
  let file_path = if parent_path.is_empty() {
    format!("{}.rs", base_file_name)
  } else {
    format!("{}/{}.rs", parent_path.replace('.', "/"), base_file_name)
  };
  token_stream_to_file(file_path, &type_source)
}

/// Generates a Rust source file for the given structure.
/// It contains the type declaration and some functions
/// to serialize and deserialize values.
/// It depends on `arora-buffers`, `arora-schema`, `arora-registry` and `uuid`.
pub async fn generate_structure_source(
  id: &Uuid,
  structure: &StructureFrozen,
  registry: &mut dyn ReadableRegistry,
  parent_path: &String,
) -> Result<Directory, GenerationError> {
  // Struct declaration.
  let name = &structure.name;
  let struct_ident = type_ident(name);
  let mut field_declarations = Vec::new();
  for (_, field) in &structure.fields {
    let field_ident = variable_ident(&field.name);
    let field_type_ident = type_ident_from_frozen(&field.ty, registry, PrefixWithMod::Yes).await?;
    field_declarations.push(quote! { pub #field_ident: #field_type_ident });
  }
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
  let mut fields_serialization = Vec::new();
  for (_, field) in &structure.fields {
    let field_const_id_ident = struct_field_const_id_ident(&name, &field.name);
    let field_ident = variable_ident(&field.name);
    let value_expression = quote! { value.#field_ident };
    let serialize = generate_serialize_from_frozen(&field.ty, value_expression, registry).await?;
    fields_serialization.push(quote! {
      writer.add_structure_field(&#field_const_id_ident);
      #serialize
    });
  }
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
  let mut field_variable_declarations = Vec::new();
  for (_, field) in &structure.fields {
    let variable_ident = struct_field_intermediate_variable_ident(&name, &field.name);
    let type_ident = type_ident_from_frozen(&field.ty, registry, PrefixWithMod::Yes).await?;
    field_variable_declarations
      .push(quote! { let mut #variable_ident: Option<#type_ident> = None; });
  }

  let mut deserialization_cases = Vec::new();
  for (_, field) in &structure.fields {
    let field_const_id_ident = struct_field_const_id_ident(&name, &field.name);
    let field_variable_ident = struct_field_intermediate_variable_ident(&name, &field.name);
    let deserialize = generate_deserialize_from_frozen(&field.ty, registry, CheckType::Yes).await?;
    deserialization_cases.push(quote! {
      if field_raw_id == #field_const_id_ident {
        #field_variable_ident = Some(#deserialize);
      }
    });
  }

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

  let type_source = quote! {
    use arora_buffers::*;
    use uuid::Uuid;
    use crate::arora_generated::error::DeserializationError;
    #struct_declaration
    #serialization
    #deserialization
    #id_declaration
    #(#field_id_declarations)*
  };

  let base_file_name = structure.name.to_case(Case::Snake);
  let file_path = if parent_path.is_empty() {
    format!("{}.rs", base_file_name)
  } else {
    format!("{}/{}.rs", parent_path.replace('.', "/"), base_file_name)
  };
  token_stream_to_file(file_path, &type_source).map_err(GenerationError::VfsError)
}

/// Generates a virtual source file with wrappers for every symbol imported by the module.
/// It contains human-readable public functions that can be used by the module implementation,
/// under the path of the module, as `path::to::module::import`.
async fn generate_imports_from_module_source(
  module_id: &Uuid,
  module_path: &String,
  imports: &Vec<ImportAsset>,
  registry: &mut dyn ReadableRegistry,
) -> Result<Directory, GenerationError> {
  // Using dependent types.
  let uses = {
    let mut dependencies = HashSet::<&FrozenReference>::new();
    for import in imports {
      import.import.dependencies(&mut dependencies);
    }
    let mut uses = Vec::new();
    for dep in dependencies {
      let dep_selector = Selector::Id(dep.id);
      let type_def = match registry
        .get_type(
          &dep_selector,
          &VersionReq::parse(dep.version.to_string().as_str()).unwrap(),
        )
        .await
      {
        Ok(TypeDefinitionFrozen::Primitive(_)) => continue,
        Ok(type_definition) => type_definition,
        Err(RegistryError::NotAType { selector: _ }) => continue,
        Err(err) => return Err(GenerationError::RegistryError(err)),
      };
      let type_ident =
        type_ident_from_definition(&type_def, &dep.id, registry, PrefixWithMod::Yes).await?;
      uses.push(type_ident);
    }
    uses
  };

  let splitted_module_path: Vec<&str> = module_path.split(".").collect();
  let module_name = splitted_module_path.last().unwrap();

  // Declare the ID of the module to use it locally.
  let module_const_id_ident =
    format_ident!("{}_MODULE_ID", module_name.to_case(Case::ScreamingSnake),);
  let module_id_declaration = generate_const_id_declaration(
    &module_name.to_string(),
    &module_const_id_ident,
    &module_id,
    Public::No,
  );

  // Declare each imported function.
  let mut functions_declarations = Vec::<TokenStream>::new();
  for import in imports {
    let ExportKind::Function(function_symbol) = &import.import.kind;
    let function_name = &import.import.name;
    let function_ident = format_ident!("{}", function_name);
    let mut parameters_declarations = Vec::new();
    for (_, param) in &function_symbol.parameters {
      let maybe_mut = if param.mutable {
        quote! { mut }
      } else {
        quote! {}
      };
      let param_name_ident = format_ident!("{}", param.name);
      let param_type_ident =
        type_ident_from_frozen(&param.ty, registry, PrefixWithMod::Yes).await?;
      parameters_declarations.push(quote! {
        #maybe_mut #param_name_ident: #param_type_ident
      });
    }
    let ret_type_ident =
      type_ident_from_frozen(&function_symbol.return_ty, registry, PrefixWithMod::Yes).await?;

    // And implement the call.
    // First declare the const ids.
    let function_const_id_ident = function_const_id_ident(&function_name);
    let function_id_declaration = generate_const_id_declaration(
      &function_name,
      &function_const_id_ident,
      &import.id,
      Public::No,
    );
    let param_ids_declarations = {
      let mut param_ids_declarations = Vec::new();
      for (id, param) in &function_symbol.parameters {
        param_ids_declarations.push(generate_const_id_declaration(
          &format!("{}.{}", &function_name, &param.name),
          &function_param_const_id_ident(&function_name, &param.name),
          &id,
          Public::No,
        ));
      }
      param_ids_declarations
    };

    let ids_declaration = quote! {
      #function_id_declaration
      #(#param_ids_declarations)*
    };

    // Then prepare a call argument structure.
    // It consists in a struct with the function id as id,
    // and with one field for each param.
    let add_args = {
      let mut add_args = Vec::new();
      for (_, param) in &function_symbol.parameters {
        let function_param_const_id_ident =
          function_param_const_id_ident(&function_name, &param.name);
        let param_name_ident = format_ident!("{}", param.name);
        let serialize_arg =
          generate_serialize_from_frozen(&param.ty, param_name_ident.into_token_stream(), registry)
            .await?;
        add_args.push(quote! {
          writer.add_structure_field(#function_param_const_id_ident.as_slice());
          #serialize_arg;
        });
      }
      add_args
    };
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
    let deserialize_ret =
      generate_deserialize_from_frozen(&function_symbol.return_ty, registry, CheckType::Yes)
        .await?;

    let process_params = if nof_args > 1 {
      let declare_mutable_params = {
        let mut declare_mutable_params = Vec::new();
        for (_, param) in &function_symbol.parameters {
          if param.mutable {
            let param_name_ident = format_ident!("{}", param.name);
            declare_mutable_params.push(quote! {
              let mut #param_name_ident = None;
            })
          }
        }
        declare_mutable_params
      };

      let deserialize_params = {
        let mut deserialize_params = Vec::new();
        for (_, param) in &function_symbol.parameters {
          if param.mutable {
            let param_name_ident = format_ident!("{}", param.name);
            let function_param_const_id_ident =
              function_param_const_id_ident(&function_name, &param.name);
            let deserialize_param =
              generate_deserialize_from_frozen(&param.ty, registry, CheckType::Yes).await?;
            deserialize_params.push(quote! {
              x if *x == #function_param_const_id_ident => *#param_name_ident = #deserialize_param,
            });
          }
        }
        deserialize_params
      };

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
    functions_declarations.push(quote! {
      pub fn #function_ident (#(#parameters_declarations),*) -> #ret_type_ident {
        #ids_declaration
        #prepare_call_structure
        #perform_call
        #prepare_parsing
        #check_result_struct
        #process_result
      }
    });
  }

  // This makes a module import.
  let source = quote! {
    #(#uses)*
    use arora_buffers::*;
    use crate::arora_generated::arora::arora_dispatch;
    #module_id_declaration
    #(#functions_declarations)*
  };

  let file_path = splitted_module_path
    .iter()
    .map(|name| name.to_case(Case::Snake))
    .fold(String::new(), |acc, name| {
      if acc.is_empty() {
        name
      } else {
        format!("{}/{}", acc, name)
      }
    })
    + ".rs";
  token_stream_to_file(file_path, &source).map_err(GenerationError::VfsError)
}

/// Generates the interface of a module, i.e. the declarations of its exported functions.
async fn generate_module_source(
  module: &ModuleFrozen,
  registry: &mut dyn ReadableRegistry,
) -> Result<Directory, GenerationError> {
  // Function Uses.
  let exports = &module.exports;
  let use_functions = exports
    .iter()
    .map(|(_, export)| format_ident!("{}", export.name));

  // Function and param IDs.
  let function_ids = exports.iter().flat_map(|(function_id, export)| {
    let ExportKind::Function(function_symbol) = &export.kind;
    let mut id_declarations = Vec::with_capacity(function_symbol.parameters.len() + 1);
    id_declarations.push(generate_const_id_declaration(
      &export.name,
      &function_const_id_ident(&export.name),
      &function_id,
      Public::Yes,
    ));
    for (param_id, param) in &function_symbol.parameters {
      id_declarations.push(generate_const_id_declaration(
        &format!("{}.{}", &export.name, &param.name),
        &function_param_const_id_ident(&export.name, &param.name),
        &param_id,
        Public::Yes,
      ));
    }
    id_declarations
  });

  // Functions declarations exported for Arora.
  let function_declarations = {
    let mut function_declarations = Vec::new();
    for (export_id, export) in exports {
      let function_ident = format_ident!("{}", export.name);
      let ExportKind::Function(function_symbol) = &export.kind;
      let const_id_ident = function_const_id_ident(&export.name);

      let call_check = quote! {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        assert!(!type_raw_id_opt.is_none());
        assert_eq!(type_raw_id_opt.unwrap(), TYPE_STRUCTURE);
        let (structure_raw_id, field_count) = reader.get_structure();
        assert_eq!(#const_id_ident, structure_raw_id);
      };

      let param_declarations = {
        let mut param_declarations = Vec::new();
        for (param_id, param) in &function_symbol.parameters {
          let param_var_ident = param_ident(param_id, param);
          let param_type_ident =
            type_ident_from_frozen(&param.ty, registry, PrefixWithMod::Yes).await?;
          param_declarations
            .push(quote! { let mut #param_var_ident: Option<#param_type_ident> = None; });
        }
        param_declarations
      };

      let deserialization_cases = {
        let mut deserialization_cases = Vec::new();
        for (param_id, param) in &function_symbol.parameters {
          let param_const_id_ident = function_param_const_id_ident(&export.name, &param.name);
          let param_var_ident = param_ident(param_id, param);
          let deserialize =
            generate_deserialize_from_frozen(&param.ty, registry, CheckType::Yes).await?;
          deserialization_cases.push(quote! {
            if field_raw_id == #param_const_id_ident {
              #param_var_ident = Some(#deserialize);
            }
          });
        }
        deserialization_cases
      };

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

      let param_args = function_symbol.parameter_ordering.iter().map(|param_id| {
        let param = function_symbol.parameters.get(param_id).unwrap();
        let param_var_ident = param_ident(param_id, param);
        if param.mutable {
          quote! { &mut #param_var_ident }
        } else {
          quote! { #param_var_ident }
        }
      });

      let call_and_write_result = {
        let result_ident = match &function_symbol.return_ty {
          FrozenTy::Primitive(Primitive { kind }) if *kind == PrimitiveKind::Unit => quote! { _ },
          _ => quote! { result },
        };
        let serialize_result = generate_serialize_from_frozen(
          &function_symbol.return_ty,
          result_ident.clone(),
          registry,
        )
        .await?;
        quote! {
          let #result_ident = #function_ident (#(#param_args),*);
          #serialize_result;
        }
      };

      let write_mutated_params: Vec<TokenStream> = {
        let mut write_mutated_params = Vec::new();
        for (param_id, param) in &function_symbol.parameters {
          if param.mutable {
            let param_var_ident = param_ident(param_id, param);
            let param_const_id_ident = function_param_const_id_ident(&export.name, &param.name);
            let serialize_param = generate_serialize_from_frozen(
              &param.ty,
              quote! {#param_var_ident.unwrap()},
              registry,
            )
            .await?;
            write_mutated_params.push(quote! {
              writer.add_structure_field(&#param_const_id_ident);
              #serialize_param;
            });
          }
        }
        write_mutated_params
      };
      let nof_mutated_params = write_mutated_params.len();

      let uuid_suffix = export_id.to_string().replace("-", "_");
      let arora_function_ident = format_ident!("arora_function_{}", uuid_suffix);
      let doc = format!("{}", export.name);
      function_declarations.push(quote! {
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
      });
    }
    function_declarations
  };

  // Putting it all together.
  let source = quote! {
    use arora_buffers::*;
    use crate::{arora_generated, #(#use_functions),*};
    #(#function_declarations)*
    #(#function_ids)*
  };
  token_stream_to_file("export.rs".to_string(), &source).map_err(GenerationError::VfsError)
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

async fn generate_serialize_from_frozen(
  ty: &FrozenTy,
  value_expression: TokenStream,
  registry: &mut dyn ReadableRegistry,
) -> Result<TokenStream, GenerationError> {
  match ty {
    FrozenTy::Primitive(primitive) => {
      let generate_serialize_primitive_array =
        |primitive_type_id: &Uuid, write_function: TokenStream| {
          let id_bytes = RawUuidValue(primitive_type_id);
          quote! {
            writer.add_array_primitive(#id_bytes, #value_expression.len() as u32);
            #write_function (#value_expression);
          }
        };
      Ok(match primitive.kind {
        PrimitiveKind::Unit => quote! { writer.add_unit() },
        PrimitiveKind::Boolean => quote! { writer.add_boolean(#value_expression) },
        PrimitiveKind::U8 => quote! { writer.add_u8(#value_expression) },
        PrimitiveKind::U16 => quote! { writer.add_u16(#value_expression) },
        PrimitiveKind::U32 => quote! { writer.add_u32(#value_expression) },
        PrimitiveKind::U64 => quote! { writer.add_u64(#value_expression) },
        PrimitiveKind::I8 => quote! { writer.add_i8(#value_expression) },
        PrimitiveKind::I16 => quote! { writer.add_i16(#value_expression) },
        PrimitiveKind::I32 => quote! { writer.add_i32(#value_expression) },
        PrimitiveKind::I64 => quote! { writer.add_i64(#value_expression) },
        PrimitiveKind::F32 => quote! { writer.add_f32(#value_expression) },
        PrimitiveKind::F64 => quote! { writer.add_f64(#value_expression) },
        PrimitiveKind::String => quote! { writer.add_string(#value_expression) },
        PrimitiveKind::ArrayBoolean => {
          generate_serialize_primitive_array(&BOOLEAN_ID, quote! { writer.add_boolean_bulk })
        }
        PrimitiveKind::ArrayU8 => {
          generate_serialize_primitive_array(&U8_ID, quote! { writer.add_u8_bulk })
        }
        PrimitiveKind::ArrayU16 => {
          generate_serialize_primitive_array(&U16_ID, quote! { writer.add_u16_bulk })
        }
        PrimitiveKind::ArrayU32 => {
          generate_serialize_primitive_array(&U32_ID, quote! { writer.add_u32_bulk })
        }
        PrimitiveKind::ArrayU64 => {
          generate_serialize_primitive_array(&U64_ID, quote! { writer.add_u64_bulk })
        }
        PrimitiveKind::ArrayI8 => {
          generate_serialize_primitive_array(&I8_ID, quote! { writer.add_i8_bulk })
        }
        PrimitiveKind::ArrayI16 => {
          generate_serialize_primitive_array(&I16_ID, quote! { writer.add_i16_bulk })
        }
        PrimitiveKind::ArrayI32 => {
          generate_serialize_primitive_array(&I32_ID, quote! { writer.add_i32_bulk })
        }
        PrimitiveKind::ArrayI64 => {
          generate_serialize_primitive_array(&I64_ID, quote! { writer.add_i64_bulk })
        }
        PrimitiveKind::ArrayF32 => {
          generate_serialize_primitive_array(&F32_ID, quote! { writer.add_f32_bulk })
        }
        PrimitiveKind::ArrayF64 => {
          generate_serialize_primitive_array(&F64_ID, quote! { writer.add_f64_bulk })
        }
        PrimitiveKind::ArrayString => {
          let id_bytes = RawUuidValue(&STRING_ID);
          quote! {
            writer.add_array_primitive(#id_bytes, #value_expression.len() as u32);
            for s in #value_expression {
              writer.add_string(s);
            }
          }
        }
      })
    }
    FrozenTy::FrozenScalar(scalar) => {
      let mod_prefix = generated_mod_ident_from_id(&scalar.reference.id, registry)
        .await
        .map_err(GenerationError::RegistryError)?;
      Ok(quote! { #mod_prefix serialize_to_writer(&#value_expression, &mut writer) })
    }
    FrozenTy::FrozenArray(array) => {
      let type_def = registry
        .get_type(
          &Selector::Id(array.reference.id),
          &VersionReq::parse(array.reference.version.0.to_string().as_str()).unwrap(),
        )
        .await
        .map_err(GenerationError::RegistryError)?;
      let id_bytes = RawUuidValue(&array.reference.id);
      let add_array_args = quote! { #id_bytes, #value_expression.len() };
      let prepare_array = match type_def {
        TypeDefinitionFrozen::Primitive(_) => {
          unreachable!("got an array of primitive type instead of a primitive array type")
        }
        TypeDefinitionFrozen::Enumeration(_) => {
          quote! { writer.add_array_enumeration(#add_array_args); }
        }
        TypeDefinitionFrozen::Structure(_) => {
          quote! { writer.add_array_structure(#add_array_args); }
        }
      };
      let mod_prefix = generated_mod_ident_from_id(&array.reference.id, registry)
        .await
        .map_err(GenerationError::RegistryError)?;
      let serialize_element =
        quote! { #mod_prefix serialize_to_writer(&#value_expression, &mut writer) };
      Ok(quote! {
        #prepare_array
        for element in #value_expression {
          #serialize_element;
        }
      })
    }
  }
}

#[async_recursion(?Send)]
async fn generate_deserialize_from_frozen(
  ty: &FrozenTy,
  registry: &mut dyn ReadableRegistry,
  check_type: CheckType,
) -> Result<TokenStream, GenerationError> {
  match ty {
    FrozenTy::Primitive(primitive) => {
      let type_kind_ident = type_kind_ident_from_primitive(&primitive.kind);

      let generate_deserialize = |deserialize: TokenStream| {
        let type_check = match check_type {
          CheckType::Yes => quote! {
            assert_eq!(reader.next_type(), Some(#type_kind_ident));
          },
          CheckType::No => quote! {},
        };
        quote! {(|| {
            #type_check
            #deserialize
          })()
        }
      };

      let generate_deserialize_base_type = |type_ident: TokenStream| {
        let getter = format_ident!("get_{}", type_ident.to_string());
        generate_deserialize(quote! { reader.#getter() })
      };

      let generate_deserialize_array = |deserialize_array: TokenStream| {
        let array_type_check = quote! {
          assert_eq!(reader.next_type(), Some(TYPE_ARRAY));
          let (ty, count) = reader.get_array();
          assert_eq!(ty, #type_kind_ident);
        };
        quote! {(|| {
            #array_type_check
            #deserialize_array
          })()
        }
      };
      Ok(match primitive.kind {
        PrimitiveKind::Unit => quote! { Result::<(), DeserializationError>::Ok(reader.get_unit()) },
        PrimitiveKind::Boolean => generate_deserialize(quote! { reader.get_boolean() }),
        PrimitiveKind::U8 => generate_deserialize_base_type(quote! {u8}),
        PrimitiveKind::U16 => generate_deserialize_base_type(quote! {u16}),
        PrimitiveKind::U32 => generate_deserialize_base_type(quote! {u32}),
        PrimitiveKind::U64 => generate_deserialize_base_type(quote! {u64}),
        PrimitiveKind::I8 => generate_deserialize_base_type(quote! {i8}),
        PrimitiveKind::I16 => generate_deserialize_base_type(quote! {i16}),
        PrimitiveKind::I32 => generate_deserialize_base_type(quote! {i32}),
        PrimitiveKind::I64 => generate_deserialize_base_type(quote! {i64}),
        PrimitiveKind::F32 => generate_deserialize_base_type(quote! {f32}),
        PrimitiveKind::F64 => generate_deserialize_base_type(quote! {f64}),
        PrimitiveKind::String => generate_deserialize(quote! {
          reader.get_string().to_string()
        }),
        PrimitiveKind::ArrayBoolean => generate_deserialize_array(quote! {
          reader.get_boolean_bulk(count)
        }),
        PrimitiveKind::ArrayU8 => generate_deserialize_array(quote! {
          reader.get_u8_bulk(count)
        }),
        PrimitiveKind::ArrayU16 => generate_deserialize_array(quote! {
          reader.get_u16_bulk(count)
        }),
        PrimitiveKind::ArrayU32 => generate_deserialize_array(quote! {
          reader.get_u32_bulk(count)
        }),
        PrimitiveKind::ArrayU64 => generate_deserialize_array(quote! {
          reader.get_u64_bulk(count)
        }),
        PrimitiveKind::ArrayI8 => generate_deserialize_array(quote! {
          reader.get_i8_bulk(count)
        }),
        PrimitiveKind::ArrayI16 => generate_deserialize_array(quote! {
          reader.get_i16_bulk(count)
        }),
        PrimitiveKind::ArrayI32 => generate_deserialize_array(quote! {
          reader.get_i32_bulk(count)
        }),
        PrimitiveKind::ArrayI64 => generate_deserialize_array(quote! {
          reader.get_i64_bulk(count)
        }),
        PrimitiveKind::ArrayF32 => generate_deserialize_array(quote! {
          reader.get_f32_bulk(count)
        }),
        PrimitiveKind::ArrayF64 => generate_deserialize_array(quote! {
          reader.get_f64_bulk(count)
        }),
        PrimitiveKind::ArrayString => {
          let deserialize_element = generate_deserialize(quote! {
            Result::<String, DeserializationError>::Ok(reader.get_string().to_string())
          });
          generate_deserialize_array(quote! {
            let mut res = Vec::<String>::with_capacity(count as usize);
            for _i in 0..count {
              res.push(#deserialize_element);
            }
            res
          })
        }
      })
    }
    FrozenTy::FrozenScalar(scalar) => {
      let mod_prefix = generated_mod_ident_from_id(&scalar.reference.id, registry)
        .await
        .map_err(GenerationError::RegistryError)?;
      let check_type = check_type == CheckType::Yes;
      let type_ident =
        type_ident_from_id(&scalar.reference.id, registry, PrefixWithMod::Yes).await?;
      let type_str = type_ident.to_string();
      Ok(
        quote! { #mod_prefix deserialize_from_reader(&mut reader, #check_type)
        .expect(format!("failed to deserialize value of type {}", #type_str).as_str()) },
      )
    }
    FrozenTy::FrozenArray(array) => {
      let type_ident =
        type_ident_from_id(&array.reference.id, registry, PrefixWithMod::Yes).await?;
      let deserialize_element = generate_deserialize_from_frozen(
        &FrozenTy::FrozenScalar(FrozenScalar {
          reference: array.reference.to_owned(),
        }),
        registry,
        CheckType::No,
      )
      .await?;
      let type_enum = match registry
        .type_of(&Selector::Id(array.reference.id.to_owned()))
        .await
        .map_err(GenerationError::RegistryError)?
      {
        RecordType::Enumeration => quote! { TYPE_ENUMERATION },
        RecordType::Structure => quote! { TYPE_STRUCTURE },
        _ => unreachable!("unexpected type of element in array"),
      };
      let raw_id = RawUuidValue(&array.reference.id);
      Ok(quote! {(|| {
        assert_eq!(reader.next_type(), Some(TYPE_ARRAY));
        let (ty, count) = reader.get_array();
        assert_eq!(ty, #type_enum);
        assert_eq!(reader.get_structure_field(), #raw_id);
        let mut res = Vec::<#type_ident>::with_capacity(count as usize);
        for _i in 0..count {
          res.push(#deserialize_element);
        }
        res
      })()})
    }
  }
}

pub fn token_stream_to_file<P: AsRef<path::Path>>(
  file_path: P,
  tokens: &TokenStream,
) -> Result<Directory, VfsError> {
  let file_path = file_path.as_ref();
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

/// Generates the const declaration of the ID associated to the given name (and ident).
pub fn generate_const_id_declaration(
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

fn param_ident(param_id: &Uuid, param: &Parameter) -> Ident {
  let param_id_sanitized = param_id.to_string().replace("-", "");
  format_ident!(
    "param_{}_{}",
    param.name.to_case(Case::Snake),
    param_id_sanitized
  )
}

pub fn variable_ident(name: &String) -> Ident {
  format_ident!("{}", name.to_case(Case::Snake))
}

async fn type_ident_from_frozen(
  ty: &FrozenTy,
  registry: &mut dyn ReadableRegistry,
  with_mod: PrefixWithMod,
) -> Result<TokenStream, GenerationError> {
  Ok(match ty {
    FrozenTy::Primitive(primitive) => match primitive {
      &Primitive::UNIT => quote! { () },
      &Primitive::BOOLEAN => quote!(bool),
      &Primitive::U8 => quote!(u8),
      &Primitive::U16 => quote!(u16),
      &Primitive::U32 => quote!(u32),
      &Primitive::U64 => quote!(u64),
      &Primitive::I8 => quote!(i8),
      &Primitive::I16 => quote!(i16),
      &Primitive::I32 => quote!(i32),
      &Primitive::I64 => quote!(i64),
      &Primitive::F32 => quote!(f32),
      &Primitive::F64 => quote!(f64),
      &Primitive::STRING => quote!(String),
      &Primitive::ARRAY_BOOLEAN => quote!(Vec<bool>),
      &Primitive::ARRAY_U8 => quote!(Vec<u8>),
      &Primitive::ARRAY_U16 => quote!(Vec<u16>),
      &Primitive::ARRAY_U32 => quote!(Vec<u32>),
      &Primitive::ARRAY_U64 => quote!(Vec<u64>),
      &Primitive::ARRAY_I8 => quote!(Vec<i8>),
      &Primitive::ARRAY_I16 => quote!(Vec<i16>),
      &Primitive::ARRAY_I32 => quote!(Vec<i32>),
      &Primitive::ARRAY_I64 => quote!(Vec<i64>),
      &Primitive::ARRAY_F32 => quote!(Vec<f32>),
      &Primitive::ARRAY_F64 => quote!(Vec<f64>),
      &Primitive::ARRAY_STRING => quote!(Vec<String>),
    },
    FrozenTy::FrozenScalar(scalar) => {
      type_ident_from_id(&scalar.reference.id, registry, with_mod).await?
    }
    FrozenTy::FrozenArray(array) => {
      let type_ident = type_ident_from_id(&array.reference.id, registry, with_mod).await?;
      quote! { Vec<#type_ident> }
    }
  })
}

async fn type_ident_from_id(
  id: &Uuid,
  registry: &mut dyn ReadableRegistry,
  with_mod: PrefixWithMod,
) -> Result<TokenStream, GenerationError> {
  let type_def = registry
    .get_type(&Selector::Id(id.to_owned()), &VersionReq::STAR)
    .await
    .map_err(GenerationError::RegistryError)?;
  type_ident_from_definition(&type_def, id, registry, with_mod).await
}

async fn type_ident_from_definition(
  type_def: &TypeDefinitionFrozen,
  id: &Uuid,
  registry: &mut dyn ReadableRegistry,
  with_mod: PrefixWithMod,
) -> Result<TokenStream, GenerationError> {
  Ok(match type_def {
    TypeDefinitionFrozen::Primitive(primitive) => match primitive {
      PrimitiveKind::Unit => quote! { () },
      PrimitiveKind::Boolean => quote!(bool),
      PrimitiveKind::U8 => quote!(u8),
      PrimitiveKind::U16 => quote!(u16),
      PrimitiveKind::U32 => quote!(u32),
      PrimitiveKind::U64 => quote!(u64),
      PrimitiveKind::I8 => quote!(i8),
      PrimitiveKind::I16 => quote!(i16),
      PrimitiveKind::I32 => quote!(i32),
      PrimitiveKind::I64 => quote!(i64),
      PrimitiveKind::F32 => quote!(f32),
      PrimitiveKind::F64 => quote!(f64),
      PrimitiveKind::String => quote!(String),
      PrimitiveKind::ArrayBoolean => quote!(Vec<bool>),
      PrimitiveKind::ArrayU8 => quote!(Vec<u8>),
      PrimitiveKind::ArrayU16 => quote!(Vec<u16>),
      PrimitiveKind::ArrayU32 => quote!(Vec<u32>),
      PrimitiveKind::ArrayU64 => quote!(Vec<u64>),
      PrimitiveKind::ArrayI8 => quote!(Vec<i8>),
      PrimitiveKind::ArrayI16 => quote!(Vec<i16>),
      PrimitiveKind::ArrayI32 => quote!(Vec<i32>),
      PrimitiveKind::ArrayI64 => quote!(Vec<i64>),
      PrimitiveKind::ArrayF32 => quote!(Vec<f32>),
      PrimitiveKind::ArrayF64 => quote!(Vec<f64>),
      PrimitiveKind::ArrayString => quote!(Vec<String>),
    },
    TypeDefinitionFrozen::Enumeration(enumeration) => {
      type_ident_from_name_and_id(&enumeration.name, id, registry, with_mod)
        .await
        .map_err(GenerationError::RegistryError)?
    }
    TypeDefinitionFrozen::Structure(structure) => {
      type_ident_from_name_and_id(&structure.name, id, registry, with_mod)
        .await
        .map_err(GenerationError::RegistryError)?
    }
  })
}

async fn type_ident_from_name_and_id(
  name: &String,
  id: &Uuid,
  registry: &mut dyn ReadableRegistry,
  with_mod: PrefixWithMod,
) -> Result<TokenStream, RegistryError> {
  let mod_prefix = match with_mod {
    PrefixWithMod::Yes => generated_mod_ident_from_id(id, registry).await?,
    PrefixWithMod::No => quote! {},
  };
  let type_ident = type_ident(name);
  Ok(quote! { #mod_prefix #type_ident })
}

fn type_kind_ident_from_primitive(primitive: &PrimitiveKind) -> TokenStream {
  match primitive {
    PrimitiveKind::Unit => quote! { TYPE_UNIT },
    PrimitiveKind::Boolean => quote! { TYPE_BOOLEAN },
    PrimitiveKind::U8 => quote! { TYPE_U8 },
    PrimitiveKind::U16 => quote! { TYPE_U16 },
    PrimitiveKind::U32 => quote! { TYPE_U32 },
    PrimitiveKind::U64 => quote! { TYPE_U64 },
    PrimitiveKind::I8 => quote! { TYPE_I8 },
    PrimitiveKind::I16 => quote! { TYPE_I16 },
    PrimitiveKind::I32 => quote! { TYPE_I32 },
    PrimitiveKind::I64 => quote! { TYPE_I64 },
    PrimitiveKind::F32 => quote! { TYPE_F32 },
    PrimitiveKind::F64 => quote! { TYPE_F64 },
    PrimitiveKind::String => quote! { TYPE_STRING },
    PrimitiveKind::ArrayBoolean
    | PrimitiveKind::ArrayU8
    | PrimitiveKind::ArrayU16
    | PrimitiveKind::ArrayU32
    | PrimitiveKind::ArrayU64
    | PrimitiveKind::ArrayI8
    | PrimitiveKind::ArrayI16
    | PrimitiveKind::ArrayI32
    | PrimitiveKind::ArrayI64
    | PrimitiveKind::ArrayF32
    | PrimitiveKind::ArrayF64
    | PrimitiveKind::ArrayString => quote! { TYPE_ARRAY },
  }
}

/// Generates the identifier for the mod publishing the record of the given id.
async fn generated_mod_ident_from_id(
  id: &Uuid,
  registry: &mut dyn ReadableRegistry,
) -> Result<TokenStream, RegistryError> {
  let mod_path = registry.resolve_id(id).await?;
  let mod_ident = mod_ident_from_path(&mod_path);
  Ok(quote! { arora_generated::#mod_ident })
}

fn mod_ident_from_path(path: &String) -> TokenStream {
  let path_parts = path.split(".").collect::<Vec<&str>>();
  let path_parts = path_parts
    .iter()
    .map(|part| format_ident!("{}", part.to_case(Case::Snake)));
  quote! { #(#path_parts ::)* }
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

#[derive(Display, Debug)]
pub enum GenerationError {
  ModuleDeclarationError(ModuleDeclarationError),
  RegistryError(RegistryError),
  VfsError(VfsError),
  IoError(std::io::Error),
  Generic(String),
}

impl std::error::Error for GenerationError {}

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

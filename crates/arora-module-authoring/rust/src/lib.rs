pub mod rustfmt;

use arora_module_core::{
    header::generate_header_file, ImportAsset, ModuleAsset, ModuleDeclarationError, ModuleMetadata,
};
use arora_registry::{
    local::LocalRegistry, EnumerationFrozen, ModuleFrozen, ReadableRegistry, RegistryError,
    StructureFrozen, TypeDefinitionFrozen,
};
use arora_types::record::ty::PrimitiveKind;
use arora_types::record::Selector;
use arora_types::record::{
    module::frozen::{ExportKind, Parameter},
    ty::{FrozenTy, Primitive},
};
use arora_types::ty::{
    BOOLEAN_ID, F32_ID, F64_ID, I16_ID, I32_ID, I64_ID, I8_ID, KEY_VALUE_ID, STRING_ID, U16_ID,
    U32_ID, U64_ID, U8_ID, UNIT_ID,
};
use arora_types::AroraType;
use arora_vfs::{Directory, Entry as VfsEntry, File, VfsError};
use async_recursion::async_recursion;
use convert_case::{Case, Casing};
use derive_more::Display;
use proc_macro2::Ident;
use quote::{__private::TokenStream, format_ident, quote, ToTokens};
use semver::{Version, VersionReq};
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fmt::Display,
    path,
};
use uuid::Uuid;

/// Generates a set of sources organized in a virtual directory
/// from a set of assets as produced by [`arora_module_core::analyze_module`].
/// First, the types, then the modules, then the imports.
/// A record in `extern_types` is not generated: code naming it names its
/// declared type.
pub async fn generate_sources(
    assets: Vec<ModuleAsset>,
    registry: &mut dyn ReadableRegistry,
    extern_types: &ExternTypes,
) -> Result<Directory, GenerationError> {
    let mut result = generate_common_sources()?;
    let mut imports_by_module: HashMap<Uuid, Vec<ImportAsset>> = HashMap::new();
    let mut current_module = Option::<(Uuid, Version, ModuleFrozen, ModuleMetadata)>::None;
    for asset in assets {
        match asset {
            // A record with a declared Rust type is not generated.
            ModuleAsset::Type(id, version, _) if extern_types.contains(&id) => {
                extern_types.check_version(&id, &version)?;
            }
            ModuleAsset::Type(id, version, ty) => match ty {
                TypeDefinitionFrozen::Primitive(_kind) => {}
                TypeDefinitionFrozen::Enumeration(enumeration) => {
                    let parent_path = registry
                        .resolve_id(&enumeration.parent)
                        .await
                        .map_err(GenerationError::RegistryError)?;
                    let enum_sources =
                        generate_enumeration_source(&id, &version, &enumeration, &parent_path)?;
                    result = result.merge_with(&enum_sources);
                }
                TypeDefinitionFrozen::Structure(structure) => {
                    let parent_path = registry
                        .resolve_id(&structure.parent)
                        .await
                        .map_err(GenerationError::RegistryError)?;
                    let struct_sources = generate_structure_source(
                        &id,
                        &version,
                        &structure,
                        registry,
                        extern_types,
                        &parent_path,
                    )
                    .await?;
                    result = result.merge_with(&struct_sources);
                }
            },
            ModuleAsset::Import(import) => {
                match imports_by_module.entry(import.module_id.to_owned()) {
                    Entry::Occupied(mut entry) => entry.get_mut().push(import),
                    Entry::Vacant(entry) => {
                        entry.insert(vec![import]);
                    }
                }
            }
            ModuleAsset::Module(module_id, version, module, metadata) => {
                let module_sources =
                    generate_module_source(&module, registry, extern_types).await?;
                result = result.merge_with(&module_sources);
                assert!(current_module.is_none()); // Only one module to generate at a time.
                current_module = Some((module_id, version, module, metadata));
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
        let imports_sources = generate_imports_from_module_source(
            &module_id,
            &module_path,
            imports,
            registry,
            extern_types,
        )
        .await?;
        result = result.merge_with(&imports_sources);
        all_imports.append(imports);
    }

    // Produce the stripped `module.yaml` file.
    let current_module = current_module.unwrap();
    result = result.merge_with(
        &generate_header_file(
            &current_module.0,
            &current_module.1,
            &current_module.2,
            &all_imports,
            &current_module.3,
        )
        .map_err(GenerationError::ModuleDeclarationError)?,
    );

    // Also declare arora engine functions.
    // On wasm32 these are real imports from the host engine; on other
    // targets (host iteration / `cargo test`) we emit panicking stubs so
    // the crate still links and trivial tests can run natively.
    let engine_functions_declarations = quote! {
      #[cfg(target_arch = "wasm32")]
      #[link(wasm_import_module = "env")]
      extern "C" {
        pub fn arora_dispatch(module_id: usize, method_id: usize, arg: usize) -> usize;
        pub fn arora_dispatch_indirect(callable_id: u64) -> usize;
      }

      #[cfg(not(target_arch = "wasm32"))]
      pub unsafe extern "C" fn arora_dispatch(
        _module_id: usize,
        _method_id: usize,
        _arg: usize,
      ) -> usize {
        panic!(
          "arora_dispatch called on the host; this module is meant to run as a wasm guest under the arora engine"
        );
      }

      #[cfg(not(target_arch = "wasm32"))]
      pub unsafe extern "C" fn arora_dispatch_indirect(_callable_id: u64) -> usize {
        panic!(
          "arora_dispatch_indirect called on the host; this module is meant to run as a wasm guest under the arora engine"
        );
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
        dir.insert("mod.rs", File::new(tokens.to_string()))
            .map_err(GenerationError::VfsError)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Generates YAML record files alongside a `records.json` index from the type assets
/// produced by [`arora_module_core::analyze_module`].
/// Must be called BEFORE [`generate_sources`] because `generate_sources` consumes `assets`.
pub fn generate_records(
    assets: &[ModuleAsset],
    registry: &LocalRegistry,
) -> Result<Directory, GenerationError> {
    use arora_registry::local::ROOT_ID;

    let mut vfs = Directory::new();
    vfs.ensure_directories(&path::PathBuf::from("folder"))
        .map_err(GenerationError::VfsError)?;
    vfs.ensure_directories(&path::PathBuf::from("enumeration"))
        .map_err(GenerationError::VfsError)?;
    vfs.ensure_directories(&path::PathBuf::from("structure"))
        .map_err(GenerationError::VfsError)?;

    let mut json_records: Vec<serde_json::Value> = vec![
        serde_json::json!({"id": UNIT_ID.to_string(), "name": "unit"}),
        serde_json::json!({"id": BOOLEAN_ID.to_string(), "name": "bool"}),
        serde_json::json!({"id": U8_ID.to_string(), "name": "u8"}),
        serde_json::json!({"id": U16_ID.to_string(), "name": "u16"}),
        serde_json::json!({"id": U32_ID.to_string(), "name": "u32"}),
        serde_json::json!({"id": U64_ID.to_string(), "name": "u64"}),
        serde_json::json!({"id": I8_ID.to_string(), "name": "i8"}),
        serde_json::json!({"id": I16_ID.to_string(), "name": "i16"}),
        serde_json::json!({"id": I32_ID.to_string(), "name": "i32"}),
        serde_json::json!({"id": I64_ID.to_string(), "name": "i64"}),
        serde_json::json!({"id": F32_ID.to_string(), "name": "f32"}),
        serde_json::json!({"id": F64_ID.to_string(), "name": "f64"}),
        serde_json::json!({"id": STRING_ID.to_string(), "name": "str"}),
    ];
    // Count of the hand-ordered primitive entries above. Everything appended
    // after them is derived from `assets`, whose iteration order is not stable,
    // so we sort that tail below to keep `records.json` from churning.
    let primitive_count = json_records.len();

    let mut seen_folders: HashSet<Uuid> = HashSet::new();

    for asset in assets {
        let (id, version, ty) = match asset {
            ModuleAsset::Type(id, version, ty) => (id, version, ty),
            _ => continue,
        };

        match ty {
            TypeDefinitionFrozen::Primitive(_) => {}
            TypeDefinitionFrozen::Enumeration(e) => {
                let yaml = serde_yaml::to_string(e)
                    .map_err(|err| GenerationError::Generic(err.to_string()))?;
                let file_name = format!("{}@{}.yaml", id, version);
                vfs.insert_at_path(
                    path::PathBuf::from("enumeration").join(&file_name),
                    File::new(yaml.as_bytes()),
                )
                .map_err(GenerationError::VfsError)?;

                let mut entry = serde_json::json!({"id": id.to_string(), "name": e.name});
                if e.parent != ROOT_ID {
                    entry["parent"] = serde_json::json!(e.parent.to_string());
                }
                json_records.push(entry);
                add_folder_chain(
                    &e.parent,
                    registry,
                    &mut vfs,
                    &mut json_records,
                    &mut seen_folders,
                )?;
            }
            TypeDefinitionFrozen::Structure(s) => {
                let yaml = serde_yaml::to_string(s)
                    .map_err(|err| GenerationError::Generic(err.to_string()))?;
                let file_name = format!("{}@{}.yaml", id, version);
                vfs.insert_at_path(
                    path::PathBuf::from("structure").join(&file_name),
                    File::new(yaml.as_bytes()),
                )
                .map_err(GenerationError::VfsError)?;

                let mut entry = serde_json::json!({"id": id.to_string(), "name": s.name});
                if s.parent != ROOT_ID {
                    entry["parent"] = serde_json::json!(s.parent.to_string());
                }
                json_records.push(entry);
                add_folder_chain(
                    &s.parent,
                    registry,
                    &mut vfs,
                    &mut json_records,
                    &mut seen_folders,
                )?;
            }
        }
    }

    // Sort the dynamically-collected records by id so `records.json` is stable
    // across builds; the leading primitives keep their fixed, hand-authored order.
    json_records[primitive_count..].sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    let records_json = serde_json::to_string(&json_records)
        .map_err(|err| GenerationError::Generic(err.to_string()))?;
    vfs.insert("records.json", File::new(records_json.as_bytes()))
        .map_err(GenerationError::VfsError)?;

    Ok(vfs)
}

fn add_folder_chain(
    folder_id: &Uuid,
    registry: &LocalRegistry,
    vfs: &mut Directory,
    json_records: &mut Vec<serde_json::Value>,
    seen: &mut HashSet<Uuid>,
) -> Result<(), GenerationError> {
    use arora_registry::local::ROOT_ID;

    if *folder_id == ROOT_ID || seen.contains(folder_id) {
        return Ok(());
    }

    let Some((name, parent_opt)) = registry.record_name_and_parent(folder_id) else {
        return Ok(());
    };

    seen.insert(*folder_id);

    let yaml = match parent_opt {
        Some(p) => format!("name: {}\nparent: {}\n", name, p),
        None => format!("name: {}\n", name),
    };
    let file_name = format!("{}.yaml", folder_id);
    vfs.insert_at_path(
        path::PathBuf::from("folder").join(&file_name),
        File::new(yaml.as_bytes()),
    )
    .map_err(GenerationError::VfsError)?;

    let mut entry = serde_json::json!({"id": folder_id.to_string(), "name": name});
    if let Some(p) = parent_opt {
        if p != ROOT_ID {
            entry["parent"] = serde_json::json!(p.to_string());
        }
        add_folder_chain(&p, registry, vfs, json_records, seen)?;
    }
    json_records.push(entry);

    Ok(())
}

pub fn generate_common_sources() -> Result<Directory, GenerationError> {
    let source = quote! {
      use derive_more::Display;

      #[derive(Display, Debug)]
      pub struct DeserializationError {
        #[display("deserialization error: {}", message)]
        pub message: String,
      }

      impl std::error::Error for DeserializationError {}
    };
    token_stream_to_file("error.rs", &source).map_err(GenerationError::VfsError)
}

/// The records whose Rust type is declared outside the generated sources — a
/// `#[derive(AroraType)]` type, usually in another crate — each with the path
/// generated code names it by. The generator references such a type at its
/// path instead of generating one, so a record has one Rust type wherever it
/// is used.
///
/// A Rust path belongs to the Rust output only, so it is given to the Rust
/// generator rather than stored with the record: records are shared with the
/// other generators and with remote registries.
#[derive(Debug, Clone, Default)]
pub struct ExternTypes {
    paths: HashMap<Uuid, (String, Version)>,
}

impl ExternTypes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares `T` as the Rust type of the record it names
    /// ([`AroraType::arora_type_id`]), referenced in generated code as `path`
    /// (for example `arora_behavior::Status`).
    pub fn declare<T: AroraType>(&mut self, path: &str) -> &mut Self {
        self.paths.insert(
            T::arora_type_id(),
            (path.to_string(), T::arora_type_version().0),
        );
        self
    }

    /// Whether the record `id` has a declared Rust type.
    fn contains(&self, id: &Uuid) -> bool {
        self.paths.contains_key(id)
    }

    /// Checks that the declared type of the record `id` is the record at
    /// `version`: a declared type pins one version of its record.
    fn check_version(&self, id: &Uuid, version: &Version) -> Result<(), GenerationError> {
        match self.paths.get(id) {
            Some((path, declared)) if declared != version => {
                Err(GenerationError::Generic(format!(
                    "record {id} is used at version {version}, but its declared type `{path}` is \
                 version {declared}"
                )))
            }
            _ => Ok(()),
        }
    }

    /// The path of the declared Rust type of the record `id`, if it has one.
    fn path(&self, id: &Uuid) -> Result<Option<TokenStream>, GenerationError> {
        self.paths
            .get(id)
            .map(|(path, _)| {
                path.parse::<TokenStream>().map_err(|err| {
                    GenerationError::Generic(format!(
                        "`{path}`, declared for record {id}, is not a Rust path: {err}"
                    ))
                })
            })
            .transpose()
    }
}

/// Generates a Rust source file declaring the given enumeration: a unit-variant
/// enum deriving `AroraType` under the record's id, version and variant ids,
/// so it converts to and from `Value` like any other declared type.
/// It depends on `arora_types`.
pub fn generate_enumeration_source(
    id: &Uuid,
    version: &Version,
    enumeration: &EnumerationFrozen,
    parent_path: &str,
) -> Result<Directory, GenerationError> {
    let enum_ident = type_ident(&enumeration.name);
    let type_attribute = type_attribute(id, version, &enumeration.name, &enum_ident)?;
    let mut variants = Vec::new();
    for (variant_id, variant) in &enumeration.variants {
        // `#[derive(AroraType)]` declares unit variants only.
        if variant.ty != FrozenTy::Primitive(Primitive::UNIT) {
            return Err(GenerationError::Generic(format!(
                "enumeration {} variant {}: a variant carrying a value cannot be declared \
                 with #[derive(AroraType)]",
                enumeration.name, variant.name
            )));
        }
        let variant_ident = type_ident(&variant.name);
        let attribute = member_attribute(variant_id, &variant.name, &variant_ident, false);
        variants.push(quote! { #attribute #variant_ident });
    }
    let source = quote! {
      #[derive(Debug, Clone, Copy, PartialEq, Eq, arora_types::AroraType)]
      #type_attribute
      pub enum #enum_ident {
        #(#variants),*
      }
    };
    token_stream_to_file(type_file_path(&enumeration.name, parent_path), &source)
        .map_err(GenerationError::VfsError)
}

/// Generates a Rust source file declaring the given structure: a struct
/// deriving `AroraType` under the record's id, version and field ids, so it
/// converts to and from `Value` like any other declared type. A field of a
/// record in `extern_types` names that record's declared type.
/// It depends on `arora_types`.
pub async fn generate_structure_source(
    id: &Uuid,
    version: &Version,
    structure: &StructureFrozen,
    registry: &mut dyn ReadableRegistry,
    extern_types: &ExternTypes,
    parent_path: &str,
) -> Result<Directory, GenerationError> {
    let struct_ident = type_ident(&structure.name);
    let type_attribute = type_attribute(id, version, &structure.name, &struct_ident)?;
    let mut fields = Vec::new();
    let mut references_generated = false;
    for (field_id, field) in &structure.fields {
        let unsupported = |shape: &str| {
            GenerationError::Generic(format!(
                "structure {} field {}: {shape} cannot be declared with #[derive(AroraType)]",
                structure.name, field.name
            ))
        };
        // A dynamic field carries a `Value` as-is: the derive's `keyvalue`.
        let keyvalue = match &field.ty {
            FrozenTy::FrozenScalar(scalar) if scalar.reference.id == *KEY_VALUE_ID => true,
            FrozenTy::FrozenScalar(scalar) if is_dynamic_value_id(&scalar.reference.id) => {
                return Err(unsupported("a dynamic array value"))
            }
            FrozenTy::FrozenArray(array) if is_dynamic_value_id(&array.reference.id) => {
                return Err(unsupported("an array of dynamic values"))
            }
            FrozenTy::FrozenOption(option) => match &*option.element {
                FrozenTy::Primitive(primitive) if is_primitive_array(&primitive.kind) => {
                    return Err(unsupported("an optional array"))
                }
                FrozenTy::FrozenArray(_) => return Err(unsupported("an optional array")),
                FrozenTy::FrozenScalar(scalar) if is_dynamic_value_id(&scalar.reference.id) => {
                    return Err(unsupported("an optional dynamic value"))
                }
                FrozenTy::FrozenOption(_) => return Err(unsupported("an optional optional")),
                _ => false,
            },
            _ => false,
        };
        let self_reference = match &field.ty {
            FrozenTy::FrozenScalar(scalar) => scalar.reference.id == *id,
            FrozenTy::FrozenOption(option) => {
                matches!(&*option.element, FrozenTy::FrozenScalar(scalar) if scalar.reference.id == *id)
            }
            _ => false,
        };
        references_generated |=
            !self_reference && references_generated_record(&field.ty, extern_types);
        // A reference back to this very structure would make it infinitely
        // sized, so it is boxed. (An array of it needs no box: a `Vec` is
        // already heap-allocated.)
        let field_type = match &field.ty {
            FrozenTy::FrozenScalar(scalar) if scalar.reference.id == *id => {
                quote! { Box<#struct_ident> }
            }
            FrozenTy::FrozenOption(option) if matches!(&*option.element, FrozenTy::FrozenScalar(scalar) if scalar.reference.id == *id) =>
            {
                quote! { Option<Box<#struct_ident>> }
            }
            ty => type_ident_from_frozen(ty, registry, extern_types).await?,
        };
        let field_ident = variable_ident(&field.name);
        let attribute = member_attribute(field_id, &field.name, &field_ident, keyvalue);
        fields.push(quote! { #attribute pub #field_ident: #field_type });
    }

    // Fields of other generated types name them `arora_generated::<module>::…`.
    let generated_modules_use = if references_generated {
        quote! { use crate::arora_generated; }
    } else {
        quote! {}
    };
    let source = quote! {
      #generated_modules_use

      #[derive(Debug, Clone, PartialEq, arora_types::AroraType)]
      #type_attribute
      pub struct #struct_ident {
        #(#fields),*
      }
    };
    token_stream_to_file(type_file_path(&structure.name, parent_path), &source)
        .map_err(GenerationError::VfsError)
}

/// The `#[arora(…)]` attribute of a generated type: its record id, its record
/// version when not the derive's default, and its record name when not its
/// Rust identifier.
fn type_attribute(
    id: &Uuid,
    version: &Version,
    name: &str,
    ident: &Ident,
) -> Result<TokenStream, GenerationError> {
    // `#[derive(AroraType)]` reads a field type by its last path segment, so a
    // type named like one it interprets would be read as that type.
    const INTERPRETED_NAMES: [&str; 9] = [
        "String", "Uuid", "Value", "Vec", "Option", "Box", "HashMap", "BTreeMap", "HashSet",
    ];
    if INTERPRETED_NAMES.contains(&ident.to_string().as_str()) {
        return Err(GenerationError::Generic(format!(
            "{name}: a type named `{ident}` cannot be declared with #[derive(AroraType)]"
        )));
    }
    let id = id.to_string();
    let mut arguments = vec![quote! { id = #id }];
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(GenerationError::Generic(format!(
            "{name}: version {version} has a pre-release or build part, which \
             #[derive(AroraType)] cannot pin"
        )));
    }
    if *version != Version::new(1, 0, 0) {
        let version = version.to_string();
        arguments.push(quote! { version = #version });
    }
    if *ident != name {
        arguments.push(quote! { name = #name });
    }
    Ok(quote! { #[arora(#(#arguments),*)] })
}

/// The `#[arora(…)]` attribute of a generated field or variant: its record id,
/// and its record name when not its Rust identifier.
fn member_attribute(id: &Uuid, name: &str, ident: &Ident, keyvalue: bool) -> TokenStream {
    let id = id.to_string();
    let mut arguments = vec![quote! { id = #id }];
    if *ident != name {
        arguments.push(quote! { name = #name });
    }
    if keyvalue {
        arguments.push(quote! { keyvalue });
    }
    quote! { #[arora(#(#arguments),*)] }
}

/// The file a generated type is written to: its record path, in snake case.
fn type_file_path(name: &str, parent_path: &str) -> String {
    let base_file_name = name.to_case(Case::Snake);
    if parent_path.is_empty() {
        format!("{}.rs", base_file_name)
    } else {
        format!("{}/{}.rs", parent_path.replace('.', "/"), base_file_name)
    }
}

/// Whether a field of type `ty` names a type generated beside it.
fn references_generated_record(ty: &FrozenTy, extern_types: &ExternTypes) -> bool {
    let generated = |id: &Uuid| !is_dynamic_value_id(id) && !extern_types.contains(id);
    match ty {
        FrozenTy::Primitive(_) => false,
        FrozenTy::FrozenScalar(scalar) => generated(&scalar.reference.id),
        FrozenTy::FrozenArray(array) => generated(&array.reference.id),
        FrozenTy::FrozenOption(option) => {
            references_generated_record(&option.element, extern_types)
        }
    }
}

fn is_primitive_array(kind: &PrimitiveKind) -> bool {
    matches!(
        kind,
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
            | PrimitiveKind::ArrayString
    )
}

/// Generates a virtual source file with wrappers for every symbol imported by the module.
/// It contains human-readable public functions that can be used by the module implementation,
/// under the path of the module, as `path::to::module::import`.
async fn generate_imports_from_module_source(
    module_id: &Uuid,
    module_path: &str,
    imports: &Vec<ImportAsset>,
    registry: &mut dyn ReadableRegistry,
    extern_types: &ExternTypes,
) -> Result<Directory, GenerationError> {
    // Parameters and returns of generated types name them
    // `arora_generated::<module>::…`; every other type is named by its full path.
    let references_generated = imports.iter().any(|import| {
        let ExportKind::Function(function_symbol) = &import.import.kind;
        references_generated_record(&function_symbol.return_ty, extern_types)
            || function_symbol
                .parameters
                .values()
                .any(|param| references_generated_record(&param.ty, extern_types))
    });
    let generated_modules_use = if references_generated {
        quote! { use crate::arora_generated; }
    } else {
        quote! {}
    };

    let splitted_module_path: Vec<&str> = module_path.split(".").collect();
    let module_name = splitted_module_path.last().unwrap();

    // Declare the ID of the module to use it locally.
    let module_const_id_ident =
        format_ident!("{}_MODULE_ID", module_name.to_case(Case::UpperSnake),);
    let module_id_declaration = generate_const_id_declaration(
        &module_name.to_string(),
        &module_const_id_ident,
        module_id,
        Public::No,
    );

    // Declare each imported function.
    let mut functions_declarations = Vec::<TokenStream>::new();
    for import in imports {
        let ExportKind::Function(function_symbol) = &import.import.kind;
        let function_name = &import.import.name;
        let function_ident = format_ident!("{}", function_name);
        let mut parameters_declarations = Vec::new();
        for param in function_symbol.parameters.values() {
            let maybe_mut = if param.mutable {
                quote! { mut }
            } else {
                quote! {}
            };
            let param_name_ident = format_ident!("{}", param.name);
            let param_type_ident =
                type_ident_from_frozen(&param.ty, registry, extern_types).await?;
            parameters_declarations.push(quote! {
              #maybe_mut #param_name_ident: #param_type_ident
            });
        }
        let ret_type_ident =
            type_ident_from_frozen(&function_symbol.return_ty, registry, extern_types).await?;

        // And implement the call.
        // First declare the const ids.
        let function_const_id_ident = function_const_id_ident(function_name);
        let function_id_declaration = generate_const_id_declaration(
            function_name,
            &function_const_id_ident,
            &import.id,
            Public::No,
        );
        let param_ids_declarations = {
            let mut param_ids_declarations = Vec::new();
            for (id, param) in &function_symbol.parameters {
                param_ids_declarations.push(generate_const_id_declaration(
                    &format!("{}.{}", function_name, param.name),
                    &function_param_const_id_ident(function_name, &param.name),
                    id,
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
            for param in function_symbol.parameters.values() {
                let function_param_const_id_ident =
                    function_param_const_id_ident(function_name, &param.name);
                let param_name_ident = format_ident!("{}", param.name);
                let serialize_arg = generate_serialize_from_frozen(
                    &param.ty,
                    param_name_ident.into_token_stream(),
                    registry,
                    extern_types,
                )
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
          let writer = &mut writer;
          writer.begin_structure(#function_const_id_ident.as_slice(), #nof_args);
          #(#add_args)*
          let arg = writer.finalize();
        };

        // Then perform the call.
        let perform_call = quote! {
          let result_buffer_addr = unsafe {
            arora_dispatch(
              #module_const_id_ident.as_ptr() as usize,
              #function_const_id_ident.as_ptr() as usize,
              arg.as_ptr() as usize,
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
            unsafe { std::slice::from_raw_parts(result_buffer_ptr, input_size) };
          let mut reader = BufferReader::new(&input);
          let reader = &mut reader;
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
        let deserialize_ret = generate_deserialize_from_frozen(
            &function_symbol.return_ty,
            registry,
            extern_types,
            CheckType::Yes,
        )
        .await?;

        let process_params = if nof_args > 1 {
            let declare_mutable_params = {
                let mut declare_mutable_params = Vec::new();
                for param in function_symbol.parameters.values() {
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
                for param in function_symbol.parameters.values() {
                    if param.mutable {
                        let param_name_ident = format_ident!("{}", param.name);
                        let function_param_const_id_ident =
                            function_param_const_id_ident(function_name, &param.name);
                        let deserialize_param = generate_deserialize_from_frozen(
                            &param.ty,
                            registry,
                            extern_types,
                            CheckType::Yes,
                        )
                        .await?;
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
      #generated_modules_use
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
    extern_types: &ExternTypes,
) -> Result<Directory, GenerationError> {
    // Function Uses.
    let exports = &module.exports;
    let use_functions = exports
        .values()
        .map(|export| format_ident!("{}", export.name));

    // Function and param IDs.
    let function_ids = exports.iter().flat_map(|(function_id, export)| {
        let ExportKind::Function(function_symbol) = &export.kind;
        let mut id_declarations = Vec::with_capacity(function_symbol.parameters.len() + 1);
        id_declarations.push(generate_const_id_declaration(
            &export.name,
            &function_const_id_ident(&export.name),
            function_id,
            Public::Yes,
        ));
        for (param_id, param) in &function_symbol.parameters {
            id_declarations.push(generate_const_id_declaration(
                &format!("{}.{}", export.name, param.name),
                &function_param_const_id_ident(&export.name, &param.name),
                param_id,
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
              let reader = &mut reader;
              let type_raw_id_opt = reader.next_type();
              if type_raw_id_opt.is_none() {
                return Err("input is empty".to_string());
              }
              if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
                return Err(format!("expected structure input, got type {:?}", type_raw_id_opt));
              }
              let (structure_raw_id, field_count) = reader.get_structure();
              if structure_raw_id != &#const_id_ident {
                return Err("function id mismatch in input".to_string());
              }
            };

            let param_declarations = {
                let mut param_declarations = Vec::new();
                for (param_id, param) in &function_symbol.parameters {
                    let param_var_ident = param_ident(param_id, param);
                    // Every parameter reaches the function as an `Option`: absent
                    // is `None`. An optional parameter already is one.
                    let declared_ty = match &param.ty {
                        FrozenTy::FrozenOption(option) => &*option.element,
                        other => other,
                    };
                    let param_type_ident =
                        type_ident_from_frozen(declared_ty, registry, extern_types).await?;
                    param_declarations.push(
                        quote! { let mut #param_var_ident: Option<#param_type_ident> = None; },
                    );
                }
                param_declarations
            };

            let deserialization_cases = {
                let mut deserialization_cases = Vec::new();
                for (param_id, param) in &function_symbol.parameters {
                    let param_const_id_ident =
                        function_param_const_id_ident(&export.name, &param.name);
                    let param_var_ident = param_ident(param_id, param);
                    let deserialize = generate_deserialize_from_frozen(
                        &param.ty,
                        registry,
                        extern_types,
                        CheckType::YesResult,
                    )
                    .await?;
                    let assign = if param.ty.is_option() {
                        quote! { #param_var_ident = #deserialize; }
                    } else {
                        quote! { #param_var_ident = Some(#deserialize); }
                    };
                    deserialization_cases.push(quote! {
                      if field_raw_id == #param_const_id_ident {
                        #assign
                      }
                    });
                }
                deserialization_cases
            };

            let deserialize_params = if function_symbol.parameters.is_empty() {
                quote! {
                  if field_count != 0 {
                    return Err(format!("expected 0 parameters but got {}", field_count));
                  }
                }
            } else {
                quote! {
                  #(#param_declarations)*
                  for _ in 0..field_count {
                    let field_raw_id = reader.get_structure_field();
                    #(#deserialization_cases else)* {
                      return Err(format!("unexpected parameter {:?}", field_raw_id));
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
                    FrozenTy::Primitive(Primitive { kind }) if *kind == PrimitiveKind::Unit => {
                        quote! { _ }
                    }
                    _ => quote! { result },
                };
                let serialize_result = generate_serialize_from_frozen(
                    &function_symbol.return_ty,
                    result_ident.clone(),
                    registry,
                    extern_types,
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
                        let param_const_id_ident =
                            function_param_const_id_ident(&export.name, &param.name);
                        let written = if param.ty.is_option() {
                            quote! { #param_var_ident }
                        } else {
                            quote! { #param_var_ident.unwrap() }
                        };
                        let serialize_param = generate_serialize_from_frozen(
                            &param.ty,
                            written,
                            registry,
                            extern_types,
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
            let doc = export.name.to_string();
            function_declarations.push(quote! {
        #[doc = #doc]
        #[no_mangle]
        pub extern "C" fn #arora_function_ident (input_addr: usize) -> usize {
          let input_ptr = input_addr as *const u8;
          const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
          let input_size_bytes: &[u8; 4] = unsafe {
            std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE)
          }.try_into().expect("input is too small");
          let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
          let input = unsafe {
            // The size prefix counts itself.
            std::slice::from_raw_parts(input_ptr, input_size)
          };
          let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
            #call_check
            #deserialize_params
            let mut writer = BufferWriter::new();
            let writer = &mut writer;
            writer.begin_structure(&#const_id_ident, (#nof_mutated_params + 1) as u32);
            writer.add_structure_field(&#const_id_ident);
            #call_and_write_result
            #(#write_mutated_params)*
            ::std::result::Result::Ok(writer.finalize())
          })();
          match _result {
            ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
            ::std::result::Result::Err(msg) => {
              let mut writer = BufferWriter::new();
              writer.add_error(&msg);
              ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
            }
          }
        }
      });
        }
        function_declarations
    };

    // Parameters and returns of generated types name them
    // `arora_generated::<module>::…`.
    let references_generated = exports.values().any(|export| {
        let ExportKind::Function(function_symbol) = &export.kind;
        references_generated_record(&function_symbol.return_ty, extern_types)
            || function_symbol
                .parameters
                .values()
                .any(|param| references_generated_record(&param.ty, extern_types))
    });
    let generated_modules_use = if references_generated {
        quote! { use crate::arora_generated; }
    } else {
        quote! {}
    };

    // Putting it all together.
    let source = quote! {
      use arora_buffers::*;
      #generated_modules_use
      use crate::{#(#use_functions),*};
      #(#function_declarations)*
      #(#function_ids)*
    };
    token_stream_to_file("export.rs", &source).map_err(GenerationError::VfsError)
}

#[async_recursion(?Send)]
async fn generate_serialize_from_frozen(
    ty: &FrozenTy,
    value_expression: TokenStream,
    registry: &mut dyn ReadableRegistry,
    extern_types: &ExternTypes,
) -> Result<TokenStream, GenerationError> {
    match ty {
        // A presence flag, then the element when present — the framing
        // `froto_value` gives `Value::Option`.
        FrozenTy::FrozenOption(option) => {
            let element = generate_serialize_from_frozen(
                &option.element,
                quote! { (*__element) },
                registry,
                extern_types,
            )
            .await?;
            Ok(quote! {
              match &#value_expression {
                Some(__element) => {
                  writer.add_option_some();
                  #element;
                }
                None => writer.add_option_none(),
              }
            })
        }
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
                PrimitiveKind::String => quote! { writer.add_string(#value_expression.as_str()) },
                PrimitiveKind::ArrayBoolean => generate_serialize_primitive_array(
                    &BOOLEAN_ID,
                    quote! { writer.add_boolean_bulk },
                ),
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
                        writer.add_string(s.as_str());
                      }
                    }
                }
            })
        }
        // A value of a record type crosses the buffer as its `Value`, through
        // the generic value codec: the encoding the engine reads it with.
        FrozenTy::FrozenScalar(scalar) => {
            // A dynamic value already is a `Value`.
            if is_dynamic_value_id(&scalar.reference.id) {
                return Ok(quote! {
                  arora_buffers::froto_value::serialize_to_writer(&#value_expression, writer)
                });
            }
            // `writer` is always a `&mut BufferWriter` binding.
            Ok(quote! {
              arora_buffers::froto_value::serialize_to_writer(
                &arora_types::value::Value::from(::std::clone::Clone::clone(&#value_expression)),
                writer,
              )
            })
        }
        FrozenTy::FrozenArray(array) => {
            if is_dynamic_value_id(&array.reference.id) {
                return Ok(quote! {
                  arora_buffers::froto_value::serialize_to_writer(
                    &arora_types::value::Value::ArrayValue(::std::clone::Clone::clone(&#value_expression)),
                    writer,
                  )
                });
            }
            let element_type =
                type_ident_from_id(&array.reference.id, registry, extern_types).await?;
            Ok(quote! {
              arora_buffers::froto_value::serialize_to_writer(
                &arora_types::value::Value::array_of_type::<#element_type>(
                  #value_expression
                    .iter()
                    .cloned()
                    .map(arora_types::value::Value::from)
                    .collect(),
                ),
                writer,
              )
            })
        }
    }
}

#[async_recursion(?Send)]
async fn generate_deserialize_from_frozen(
    ty: &FrozenTy,
    registry: &mut dyn ReadableRegistry,
    extern_types: &ExternTypes,
    check_type: CheckType,
) -> Result<TokenStream, GenerationError> {
    match ty {
        // An optional arrives framed (a presence flag, then the element), or
        // as its bare element when present. The element carries its own tag,
        // so it is read with a type check whatever the optional's context.
        FrozenTy::FrozenOption(option) => {
            let element_check = if check_type == CheckType::YesResult {
                CheckType::YesResult
            } else {
                CheckType::Yes
            };
            let element = generate_deserialize_from_frozen(
                &option.element,
                registry,
                extern_types,
                element_check,
            )
            .await?;
            Ok(quote! {{
              if reader.peek_type() == Some(TYPE_OPTION) {
                reader.next_type();
                if reader.get_option_presence() {
                  Some(#element)
                } else {
                  None
                }
              } else {
                Some(#element)
              }
            }})
        }
        FrozenTy::Primitive(primitive) => {
            let type_kind_ident = type_kind_ident_from_primitive(&primitive.kind);

            let generate_deserialize = |deserialize: TokenStream| {
                let type_check = match check_type {
                    CheckType::Yes => quote! {
                      {
                        let _next_type = reader.next_type();
                        assert_eq!(_next_type, Some(#type_kind_ident), "type mismatch");
                      }
                    },
                    CheckType::YesResult => quote! {
                      {
                        let _next_type = reader.next_type();
                        if _next_type != Some(#type_kind_ident) {
                          return Err(format!("type mismatch: expected {:?} but got {:?}", #type_kind_ident, _next_type));
                        }
                      }
                    },
                };
                quote! {{
                    #type_check
                    #deserialize
                  }
                }
            };

            let generate_deserialize_base_type = |type_ident: TokenStream| {
                let getter = format_ident!("get_{}", type_ident.to_string());
                generate_deserialize(quote! { reader.#getter() })
            };

            let generate_deserialize_array = |deserialize_array: TokenStream| {
                let array_type_check = match check_type {
                    CheckType::Yes => quote! {
                      {
                        let _at = reader.next_type();
                        assert_eq!(_at, Some(TYPE_ARRAY));
                      }
                      let (ty, count) = reader.get_array();
                      assert_eq!(ty, #type_kind_ident);
                    },
                    _ => quote! {
                      {
                        let _at = reader.next_type();
                        if _at != Some(TYPE_ARRAY) {
                          return Err(format!("expected array, got {:?}", _at));
                        }
                      }
                      let (ty, count) = reader.get_array();
                      if ty != #type_kind_ident {
                        return Err(format!("expected array element type {:?}, got {:?}", #type_kind_ident, ty));
                      }
                    },
                };
                quote! {{
                    #array_type_check
                    #deserialize_array
                  }
                }
            };
            Ok(match primitive.kind {
                PrimitiveKind::Unit => {
                    quote! { Result::<(), DeserializationError>::Ok(reader.get_unit()) }
                }
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
        // A value of a record type is read as a `Value`, through the generic
        // value codec, and converted to its Rust type.
        FrozenTy::FrozenScalar(scalar) => {
            // A dynamic value is kept as the `Value` read.
            if is_dynamic_value_id(&scalar.reference.id) {
                return Ok(quote! {
                  arora_buffers::froto_value::deserialize_from_reader(reader)
                });
            }
            let type_ident =
                type_ident_from_id(&scalar.reference.id, registry, extern_types).await?;
            let type_str = type_ident.to_string();
            // `reader` is always a `&mut BufferReader` binding.
            let read = quote! {
              <#type_ident as ::std::convert::TryFrom<arora_types::value::Value>>::try_from(
                arora_buffers::froto_value::deserialize_from_reader(reader),
              )
            };
            Ok(fail_deserialization(read, &type_str, check_type))
        }
        FrozenTy::FrozenArray(array) => {
            let (type_ident, convert) = if is_dynamic_value_id(&array.reference.id) {
                (dynamic_value_type(), quote! { Ok(__element) })
            } else {
                let type_ident =
                    type_ident_from_id(&array.reference.id, registry, extern_types).await?;
                let convert = quote! {
                  <#type_ident as ::std::convert::TryFrom<arora_types::value::Value>>::try_from(__element)
                    .map_err(|e| e.to_string())
                };
                (type_ident, convert)
            };
            let type_str = format!("array of {}", type_ident);
            let read = quote! {
              arora_buffers::froto_value::deserialize_from_reader(reader)
                .into_elements()
                .and_then(|__elements| {
                  __elements
                    .into_iter()
                    .map(|__element| #convert)
                    .collect::<::std::result::Result<::std::vec::Vec<#type_ident>, ::std::string::String>>()
                })
            };
            Ok(fail_deserialization(read, &type_str, check_type))
        }
    }
}

/// An expression unwrapping `read`, a `Result` whose error displays: a
/// `return Err(String)` from the enclosing export shim under
/// [`CheckType::YesResult`], a panic otherwise.
fn fail_deserialization(read: TokenStream, type_str: &str, check_type: CheckType) -> TokenStream {
    match check_type {
        CheckType::YesResult => quote! {
          #read.map_err(|e| format!("failed to deserialize {}: {}", #type_str, e))?
        },
        _ => quote! {
          #read.unwrap_or_else(|e| panic!("failed to deserialize {}: {}", #type_str, e))
        },
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
    let parent_dir = match output.ensure_directories(parent_path) {
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
        function_name.to_case(Case::UpperSnake),
    )
}

pub fn function_param_const_id_ident(function_name: &String, param_name: &String) -> Ident {
    format_ident!(
        "{}_{}_PARAMETER_RAW_ID",
        function_name.to_case(Case::UpperSnake),
        param_name.to_case(Case::UpperSnake),
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

#[async_recursion(?Send)]
async fn type_ident_from_frozen(
    ty: &FrozenTy,
    registry: &mut dyn ReadableRegistry,
    extern_types: &ExternTypes,
) -> Result<TokenStream, GenerationError> {
    Ok(match ty {
        FrozenTy::FrozenOption(option) => {
            let element = type_ident_from_frozen(&option.element, registry, extern_types).await?;
            quote! { Option<#element> }
        }
        FrozenTy::Primitive(primitive) => match *primitive {
            Primitive::UNIT => quote! { () },
            Primitive::BOOLEAN => quote!(bool),
            Primitive::U8 => quote!(u8),
            Primitive::U16 => quote!(u16),
            Primitive::U32 => quote!(u32),
            Primitive::U64 => quote!(u64),
            Primitive::I8 => quote!(i8),
            Primitive::I16 => quote!(i16),
            Primitive::I32 => quote!(i32),
            Primitive::I64 => quote!(i64),
            Primitive::F32 => quote!(f32),
            Primitive::F64 => quote!(f64),
            Primitive::STRING => quote!(String),
            Primitive::ARRAY_BOOLEAN => quote!(Vec<bool>),
            Primitive::ARRAY_U8 => quote!(Vec<u8>),
            Primitive::ARRAY_U16 => quote!(Vec<u16>),
            Primitive::ARRAY_U32 => quote!(Vec<u32>),
            Primitive::ARRAY_U64 => quote!(Vec<u64>),
            Primitive::ARRAY_I8 => quote!(Vec<i8>),
            Primitive::ARRAY_I16 => quote!(Vec<i16>),
            Primitive::ARRAY_I32 => quote!(Vec<i32>),
            Primitive::ARRAY_I64 => quote!(Vec<i64>),
            Primitive::ARRAY_F32 => quote!(Vec<f32>),
            Primitive::ARRAY_F64 => quote!(Vec<f64>),
            Primitive::ARRAY_STRING => quote!(Vec<String>),
        },
        FrozenTy::FrozenScalar(scalar) => {
            type_ident_from_id(&scalar.reference.id, registry, extern_types).await?
        }
        FrozenTy::FrozenArray(array) => {
            let type_ident =
                type_ident_from_id(&array.reference.id, registry, extern_types).await?;
            quote! { Vec<#type_ident> }
        }
    })
}

/// The well-known "dynamic" record ids. A field referencing one of these is a
/// deliberately open/recursive union: rather than a declared struct, it is kept
/// as the generic `arora_types::value::Value` escape hatch (see the module docs
/// / PR notes). This is how e.g. a Vizij keyframe `value` — whose leaves are a
/// mix of primitives, declared structs and further unions — is represented.
fn is_dynamic_value_id(id: &Uuid) -> bool {
    *id == *arora_types::ty::ARRAY_VALUE_ID || *id == *arora_types::ty::KEY_VALUE_ID
}

/// The Rust type a dynamic-value field takes: the generic runtime `Value`.
fn dynamic_value_type() -> TokenStream {
    quote! { arora_types::value::Value }
}

async fn type_ident_from_id(
    id: &Uuid,
    registry: &mut dyn ReadableRegistry,
    extern_types: &ExternTypes,
) -> Result<TokenStream, GenerationError> {
    if is_dynamic_value_id(id) {
        return Ok(dynamic_value_type());
    }
    if let Some(path) = extern_types.path(id)? {
        return Ok(path);
    }
    let type_def = registry
        .get_type(&Selector::Id(id.to_owned()), &VersionReq::STAR)
        .await
        .map_err(GenerationError::RegistryError)?;
    type_ident_from_definition(&type_def, id, registry, extern_types).await
}

async fn type_ident_from_definition(
    type_def: &TypeDefinitionFrozen,
    id: &Uuid,
    registry: &mut dyn ReadableRegistry,
    extern_types: &ExternTypes,
) -> Result<TokenStream, GenerationError> {
    if let Some(path) = extern_types.path(id)? {
        return Ok(path);
    }
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
            generated_type_path(&enumeration.name, id, registry)
                .await
                .map_err(GenerationError::RegistryError)?
        }
        TypeDefinitionFrozen::Structure(structure) => {
            generated_type_path(&structure.name, id, registry)
                .await
                .map_err(GenerationError::RegistryError)?
        }
    })
}

/// The path of the type generated for the record `id`, named `name`.
async fn generated_type_path(
    name: &String,
    id: &Uuid,
    registry: &mut dyn ReadableRegistry,
) -> Result<TokenStream, RegistryError> {
    let mod_path = registry.resolve_id(id).await?;
    let mod_ident = mod_ident_from_path(&mod_path);
    let type_ident = type_ident(name);
    Ok(quote! { arora_generated::#mod_ident #type_ident })
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

fn mod_ident_from_path(path: &str) -> TokenStream {
    let path_parts = path.split(".").collect::<Vec<&str>>();
    let path_parts = path_parts
        .iter()
        .map(|part| format_ident!("{}", part.to_case(Case::Snake)));
    quote! { #(#path_parts ::)* }
}

/// How generated deserialization reacts to a value of an unexpected type.
#[derive(Clone, Copy, Eq, PartialEq)]
enum CheckType {
    /// Panics (import shims).
    Yes,
    /// Returns `Err(String)` from the enclosing export shim.
    YesResult,
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

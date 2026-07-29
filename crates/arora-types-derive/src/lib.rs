//! `#[derive(AroraType)]` — generate an arora `ty::low::Type` from a Rust type,
//! so the Rust definition is the source of truth for the schema instead of a
//! hand-authored YAML record.
//!
//! The generated impl produces the type's own `ty::low::Type`, the id it is
//! referenced by, and a `TypeRegistry` carrying it and its transitive
//! dependencies. Type and field ids must be pinned with
//! `#[arora(id = "…uuid…")]`: a name-hash id silently changes when a type or
//! field is renamed, so it is not a reliable identity. A ROS type may instead
//! set `#[arora(name = "pkg/msg/Name")]` on the struct to opt into name-hashing
//! its qualified name (a ROS name is the stable spec identity) — that also
//! name-hashes the struct's fields.
//!
//! Mirrors the type-directed walk it feeds: named-field structs whose fields are
//! primitive scalars, `String`, `Uuid`, other `#[derive(AroraType)]` types, a
//! `Vec<T>` of any of those (a homogeneous array), an `Option<T>`, or a
//! `#[arora(keyvalue)]` field carrying dynamically-typed values. Maps and enums
//! are rejected pending a `ty::low` model extension.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Fields, Type};

#[proc_macro_derive(AroraType, attributes(arora))]
pub fn derive_arora_type(input: TokenStream) -> TokenStream {
  let input = syn::parse_macro_input!(input as DeriveInput);
  expand(input)
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
}

fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
  let name = &input.ident;
  let name_str = name.to_string();

  let Data::Struct(data) = &input.data else {
    return Err(syn::Error::new(
      Span::call_site(),
      "AroraType can only be derived for structs",
    ));
  };
  let Fields::Named(named) = &data.fields else {
    return Err(syn::Error::new_spanned(
      &data.fields,
      "AroraType requires a struct with named fields",
    ));
  };

  let meta = parse_arora_meta(&input.attrs)?;
  // The arora type name: an `#[arora(name = "…")]` override (the ROS-qualified
  // name for generated messages), else the Rust type's own name.
  let type_name = meta.name.clone().unwrap_or_else(|| name_str.clone());
  // Name-hash mode: a type opts in with `#[arora(name = "…")]` (ROS types, whose
  // qualified name is the stable identity). Then it and its fields may name-hash;
  // otherwise every id must be pinned explicitly.
  let name_hash_mode = meta.name.is_some();
  let type_id_expr = id_expr(meta.id, &type_name, name_hash_mode, Span::call_site())?;

  let mut field_entries = Vec::new();
  let mut register_calls = Vec::new();
  for field in &named.named {
    let fname = field.ident.as_ref().expect("a named field has an ident");
    // A raw identifier (`r#type`, for a field whose ROS name is a Rust keyword)
    // names the plain word in arora, so its name and id match the ROS field.
    let fname_raw = fname.to_string();
    let fname_str = fname_raw
      .strip_prefix("r#")
      .unwrap_or(&fname_raw)
      .to_string();
    let field_meta = parse_arora_meta(&field.attrs)?;
    let field_id_expr = id_expr(field_meta.id, &fname_str, name_hash_mode, field.span())?;
    // A `#[arora(keyvalue)]` field is an opaque bag of dynamically-typed arora
    // values: it references the well-known KeyValue type by id and has no nested
    // user type to register, whatever its Rust type happens to be.
    let (type_ref_expr, nested) = if field_meta.keyvalue {
      (
        quote! { arora_types::module::low::TypeRef::Scalar { id: *arora_types::ty::KEY_VALUE_ID } },
        None,
      )
    } else {
      type_ref_for(&field.ty)?
    };
    field_entries.push(quote! {
      (
        #field_id_expr,
        arora_types::ty::low::StructureField {
          name: #fname_str.to_string(),
          type_ref: #type_ref_expr,
        },
      )
    });
    if let Some(ty) = nested {
      register_calls.push(quote! {
        <#ty as arora_types::AroraType>::register_types(registry);
      });
    }
  }

  Ok(quote! {
    impl arora_types::AroraType for #name {
      fn arora_type_id() -> arora_types::Uuid {
        #type_id_expr
      }

      fn arora_type() -> arora_types::ty::low::Type {
        arora_types::ty::low::Type {
          name: #type_name.to_string(),
          id: <Self as arora_types::AroraType>::arora_type_id(),
          // Fully qualified: a derived type may live in a module that shadows
          // `String` (e.g. the generated `std_msgs::String` message).
          description: ::std::string::String::new(),
          kind: arora_types::ty::low::TypeKind::Structure(
            arora_types::ty::low::Structure::from_fields([
              #(#field_entries),*
            ]),
          ),
        }
      }

      fn register_types(registry: &mut arora_types::ty::TypeRegistry) {
        // Insert self before recursing so a type reachable from itself (through
        // a field) is visited exactly once.
        let id = <Self as arora_types::AroraType>::arora_type_id();
        if registry.contains_key(&id) {
          return;
        }
        registry.insert(id, <Self as arora_types::AroraType>::arora_type());
        #(#register_calls)*
      }
    }
  })
}

/// The id expression for a struct or field. An explicit `#[arora(id = "…")]`
/// wins. Otherwise a name hash is emitted **only** in name-hash mode — a type
/// that opted in with `#[arora(name = "…")]` (the ROS case, where the qualified
/// name is the stable identity). In strict mode an explicit id is required: a
/// name hash silently changes when a type or field is renamed, so it is not a
/// reliable identity.
fn id_expr(
  explicit: Option<(String, Span)>,
  name: &str,
  name_hash_mode: bool,
  err_span: Span,
) -> syn::Result<TokenStream2> {
  match explicit {
    Some((uuid, span)) => uuid_bytes_expr(&uuid, span),
    None if name_hash_mode => Ok(quote! { arora_types::gen_uuid_from_str(#name) }),
    None => Err(syn::Error::new(
      err_span,
      "AroraType requires an explicit `#[arora(id = \"<uuid>\")]` here — a name-hash \
       id changes when the type or field is renamed, so it is not a reliable \
       identity. A ROS type may set `#[arora(name = \"<pkg/msg/Name>\")]` on the \
       struct to opt into name-hashing its qualified name instead.",
    )),
  }
}

/// A parsed `#[arora(…)]` attribute: an explicit `id`, an explicit `name`, a
/// `keyvalue` marker, or a combination.
#[derive(Default)]
struct AroraMeta {
  id: Option<(String, Span)>,
  name: Option<String>,
  /// A `#[arora(keyvalue)]` field: its schema is the well-known KeyValue type,
  /// its contents dynamically typed. Used for a field that carries arbitrary
  /// arora values whose types are not known statically (e.g. a call's args).
  keyvalue: bool,
}

/// Parse `#[arora(id = "…", name = "…")]` from an attribute list. `id` pins the
/// type/field id (otherwise it is a hash of the name). `name` overrides the
/// arora type name — used to carry the ROS-qualified name
/// (`geometry_msgs/msg/Point`) — and, when no `id` is given, is what the default
/// id hashes, so a generated type and the same type defined at runtime agree.
fn parse_arora_meta(attrs: &[Attribute]) -> syn::Result<AroraMeta> {
  let mut parsed = AroraMeta::default();
  for attr in attrs {
    if !attr.path().is_ident("arora") {
      continue;
    }
    attr.parse_nested_meta(|meta| {
      if meta.path.is_ident("id") {
        let lit: syn::LitStr = meta.value()?.parse()?;
        parsed.id = Some((lit.value(), lit.span()));
        Ok(())
      } else if meta.path.is_ident("name") {
        let lit: syn::LitStr = meta.value()?.parse()?;
        parsed.name = Some(lit.value());
        Ok(())
      } else if meta.path.is_ident("keyvalue") {
        parsed.keyvalue = true;
        Ok(())
      } else {
        Err(meta.error(
          "unknown `arora` attribute (expected `id = \"…\"`, `name = \"…\"`, or `keyvalue`)",
        ))
      }
    })?;
  }
  Ok(parsed)
}

/// Validate a UUID literal at macro time and emit it as a `Uuid::from_bytes`.
fn uuid_bytes_expr(literal: &str, span: Span) -> syn::Result<TokenStream2> {
  let uuid = uuid::Uuid::parse_str(literal)
    .map_err(|e| syn::Error::new(span, format!("invalid uuid: {e}")))?;
  let bytes = uuid.as_bytes().iter().map(|b| quote! { #b });
  Ok(quote! { arora_types::Uuid::from_bytes([ #(#bytes),* ]) })
}

/// The `TypeRef` a field of type `ty` is referenced by, and — for a nested
/// user-defined type — that type, so its definition is registered too.
fn type_ref_for(ty: &Type) -> syn::Result<(TokenStream2, Option<&Type>)> {
  // `[T; N]` -> a fixed-length homogeneous array of `N` elements of type `T`.
  if let Type::Array(array) = ty {
    let (element_id, nested) = element_id_for(&array.elem)?;
    let len = &array.len;
    let expr = quote! {
      arora_types::module::low::TypeRef::FixedArray { id: #element_id, len: (#len) as usize }
    };
    return Ok((expr, nested));
  }

  let Type::Path(type_path) = ty else {
    return Err(syn::Error::new(
      ty.span(),
      "unsupported field type (expected a named type)",
    ));
  };
  let segment = type_path
    .path
    .segments
    .last()
    .ok_or_else(|| syn::Error::new(ty.span(), "empty type path"))?;
  let ident = segment.ident.to_string();

  // `Vec<T>` -> a homogeneous array whose element type is `T`.
  if ident == "Vec" {
    let element = single_type_arg(segment, "Vec")?;
    let (element_id, nested) = element_id_for(element)?;
    let expr = quote! {
      arora_types::module::low::TypeRef::Array { id: #element_id }
    };
    return Ok((expr, nested));
  }

  // `Option<T>` -> an optional value of element type `T`.
  if ident == "Option" {
    let element = single_type_arg(segment, "Option")?;
    let (element_id, nested) = element_id_for(element)?;
    let expr = quote! {
      arora_types::module::low::TypeRef::Option { id: #element_id }
    };
    return Ok((expr, nested));
  }

  // The remaining containers still need a `ty::low` model extension — a `Map`
  // `TypeRef` exists but the derive does not emit it yet, and an array carries a
  // single element id only (so no nested arrays) — so reject rather than
  // mis-encode.
  if matches!(ident.as_str(), "HashMap" | "BTreeMap" | "HashSet" | "Box") {
    return Err(syn::Error::new(
      ty.span(),
      format!("`{ident}` fields are not supported by #[derive(AroraType)] yet"),
    ));
  }

  let (id_expr, nested) = element_id_for(ty)?;
  let expr = quote! {
    arora_types::module::low::TypeRef::Scalar { id: #id_expr }
  };
  Ok((expr, nested))
}

/// The type id a scalar or nested-struct type is referenced by — a well-known
/// primitive id, or the nested type's `arora_type_id()` — plus that nested type
/// so it gets registered. Shared by plain fields and `Vec` elements. Rejects
/// containers: an array/option/map element would need its own registered type,
/// which the `ty::low` model does not carry.
fn element_id_for(ty: &Type) -> syn::Result<(TokenStream2, Option<&Type>)> {
  let Type::Path(type_path) = ty else {
    return Err(syn::Error::new(
      ty.span(),
      "unsupported type (expected a named type)",
    ));
  };
  let ident = type_path
    .path
    .segments
    .last()
    .ok_or_else(|| syn::Error::new(ty.span(), "empty type path"))?
    .ident
    .to_string();
  if matches!(
    ident.as_str(),
    "Vec" | "Option" | "HashMap" | "BTreeMap" | "HashSet" | "Box"
  ) {
    return Err(syn::Error::new(
      ty.span(),
      format!("`{ident}` as an array element is not supported yet"),
    ));
  }
  if let Some(id) = primitive_id_ident(&ident) {
    let id = syn::Ident::new(id, Span::call_site());
    Ok((quote! { *arora_types::ty::#id }, None))
  } else {
    Ok((
      quote! { <#ty as arora_types::AroraType>::arora_type_id() },
      Some(ty),
    ))
  }
}

/// The single type argument `T` of a `Container<T>` path segment (a `Vec<T>` or
/// an `Option<T>`); `container` names it for the error messages.
fn single_type_arg<'a>(segment: &'a syn::PathSegment, container: &str) -> syn::Result<&'a Type> {
  let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
    return Err(syn::Error::new(
      segment.ident.span(),
      format!("`{container}` needs a single type argument"),
    ));
  };
  let mut elements = args.args.iter().filter_map(|arg| match arg {
    syn::GenericArgument::Type(ty) => Some(ty),
    _ => None,
  });
  let element = elements.next().ok_or_else(|| {
    syn::Error::new(
      segment.ident.span(),
      format!("`{container}` needs a type argument"),
    )
  })?;
  if elements.next().is_some() {
    return Err(syn::Error::new(
      segment.ident.span(),
      format!("`{container}` must have exactly one type argument"),
    ));
  }
  Ok(element)
}

/// The well-known primitive id constant (in `arora_types::ty`) a Rust primitive
/// maps to, if it is one.
fn primitive_id_ident(ident: &str) -> Option<&'static str> {
  Some(match ident {
    "bool" => "BOOLEAN_ID",
    "i8" => "I8_ID",
    "i16" => "I16_ID",
    "i32" => "I32_ID",
    "i64" => "I64_ID",
    "u8" => "U8_ID",
    "u16" => "U16_ID",
    "u32" => "U32_ID",
    "u64" => "U64_ID",
    "f32" => "F32_ID",
    "f64" => "F64_ID",
    "String" => "STRING_ID",
    "Uuid" => "UUID_ID",
    _ => return None,
  })
}

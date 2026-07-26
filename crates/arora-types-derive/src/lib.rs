//! `#[derive(AroraType)]` — generate an arora `ty::low::Type` from a Rust type,
//! so the Rust definition is the source of truth for the schema instead of a
//! hand-authored YAML record.
//!
//! The generated impl produces the type's own `ty::low::Type`, the id it is
//! referenced by, and a `TypeRegistry` carrying it and its transitive
//! dependencies. Field and type ids default to a hash of the name (matching
//! `arora_types::gen_uuid_from_str`, so a derived type agrees with the
//! name-hashing serde bridge); annotate the struct or a field with
//! `#[arora(id = "…uuid…")]` to pin an explicit id.
//!
//! First cut mirrors the type-directed walk it feeds: named-field structs whose
//! fields are primitive scalars, `String`, other `#[derive(AroraType)]` types,
//! or a `Vec<T>` of any of those (a homogeneous array). Options, maps and enums
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

  let type_id_expr = id_expr(arora_id_attr(&input.attrs)?, &name_str)?;

  let mut field_entries = Vec::new();
  let mut register_calls = Vec::new();
  for field in &named.named {
    let fname = field.ident.as_ref().expect("a named field has an ident");
    let fname_str = fname.to_string();
    let field_id_expr = id_expr(arora_id_attr(&field.attrs)?, &fname_str)?;
    let (type_ref_expr, nested) = type_ref_for(&field.ty)?;
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
          name: #name_str.to_string(),
          id: <Self as arora_types::AroraType>::arora_type_id(),
          description: String::new(),
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

/// The id expression for a struct or field: an explicit `#[arora(id = "…")]`
/// emitted as its raw bytes, or a name hash by default.
fn id_expr(explicit: Option<(String, Span)>, name: &str) -> syn::Result<TokenStream2> {
  match explicit {
    Some((uuid, span)) => uuid_bytes_expr(&uuid, span),
    None => Ok(quote! { arora_types::gen_uuid_from_str(#name) }),
  }
}

/// Parse a single `#[arora(id = "…")]` from an attribute list, if present.
fn arora_id_attr(attrs: &[Attribute]) -> syn::Result<Option<(String, Span)>> {
  let mut found = None;
  for attr in attrs {
    if !attr.path().is_ident("arora") {
      continue;
    }
    attr.parse_nested_meta(|meta| {
      if meta.path.is_ident("id") {
        let lit: syn::LitStr = meta.value()?.parse()?;
        found = Some((lit.value(), lit.span()));
        Ok(())
      } else {
        Err(meta.error("unknown `arora` attribute (expected `id = \"…\"`)"))
      }
    })?;
  }
  Ok(found)
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
    let element = vec_element(segment)?;
    let (element_id, nested) = element_id_for(element)?;
    let expr = quote! {
      arora_types::module::low::TypeRef::Array { id: #element_id }
    };
    return Ok((expr, nested));
  }

  // The remaining containers still need a `ty::low` model extension — there is
  // no Option/Map `TypeRef`, and an array carries a single element id only (so
  // no nested arrays) — so reject rather than mis-encode.
  if matches!(
    ident.as_str(),
    "Option" | "HashMap" | "BTreeMap" | "HashSet" | "Box"
  ) {
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

/// The single element type `T` of a `Vec<T>` path segment.
fn vec_element(segment: &syn::PathSegment) -> syn::Result<&Type> {
  let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
    return Err(syn::Error::new(
      segment.ident.span(),
      "`Vec` needs a single type argument",
    ));
  };
  let mut elements = args.args.iter().filter_map(|arg| match arg {
    syn::GenericArgument::Type(ty) => Some(ty),
    _ => None,
  });
  let element = elements
    .next()
    .ok_or_else(|| syn::Error::new(segment.ident.span(), "`Vec` needs a type argument"))?;
  if elements.next().is_some() {
    return Err(syn::Error::new(
      segment.ident.span(),
      "`Vec` must have exactly one type argument",
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
    _ => return None,
  })
}

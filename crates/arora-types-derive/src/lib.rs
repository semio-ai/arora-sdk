//! `#[derive(AroraType)]` — generate an arora `ty::low::Type` from a Rust type,
//! so the Rust definition is the source of truth for the schema instead of a
//! hand-authored YAML record.
//!
//! The generated impls produce the type's own `ty::low::Type`, the id it is
//! referenced by, the record version it is pinned at, a `TypeRegistry`
//! carrying it and its transitive dependencies, and the value-plane
//! conversions `From<T> for Value` and `TryFrom<Value> for T` — a structure
//! under the type's id with each field under its id, in declared order; a
//! unit-variant enum as an enumeration under its variant's id. Type and field
//! ids must be pinned with `#[arora(id = "…")]`, spelled as a hex UUID or as
//! its thirteen-emoji form (`arora_id`): a name-hash id silently changes when
//! a type or field is renamed, so it is not a reliable identity. A ROS type
//! may instead set `#[arora(name = "pkg/msg/Name")]` on the struct to opt into
//! name-hashing its qualified name (a ROS name is the stable spec identity) —
//! that also name-hashes the struct's fields. `#[arora(version = "…")]` pins
//! the record version (`1.0.0` by default).
//!
//! A `#[arora(keyvalue)]` field crosses the value plane as-is when its Rust
//! type is `Value`, and through `value_serde` (name-hashed, unseeded)
//! otherwise.
//!
//! Mirrors the type-directed walk it feeds: named-field structs whose fields are
//! primitive scalars, `String`, `Uuid`, other `#[derive(AroraType)]` types, a
//! `Vec<T>` of any of those (a homogeneous array), an `Option<T>`, or a
//! `#[arora(keyvalue)]` field carrying dynamically-typed values; and enums with
//! **unit variants** (a `ty::low` enumeration, each variant id pinned like a
//! field). Maps and payload-carrying enum variants are rejected pending a
//! `ty::low` model extension.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
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
  match &input.data {
    Data::Struct(syn::DataStruct {
      fields: Fields::Named(named),
      ..
    }) => expand_struct(&input, named),
    Data::Enum(data) => expand_enum(&input, data),
    Data::Struct(_) => Err(syn::Error::new_spanned(
      &input,
      "AroraType requires a struct with named fields",
    )),
    Data::Union(_) => Err(syn::Error::new(
      Span::call_site(),
      "AroraType cannot be derived for a union",
    )),
  }
}

/// Derive `AroraType` for a named-field struct: a `ty::low` structure whose
/// fields are the struct's fields, each referenced by its pinned id.
fn expand_struct(input: &DeriveInput, named: &syn::FieldsNamed) -> syn::Result<TokenStream2> {
  let name = &input.ident;
  let name_str = name.to_string();

  let meta = parse_arora_meta(&input.attrs)?;
  // The arora type name: an `#[arora(name = "…")]` override (the ROS-qualified
  // name for generated messages), else the Rust type's own name.
  let type_name = meta.name.clone().unwrap_or_else(|| name_str.clone());
  // Name-hash mode: a type opts in with `#[arora(name = "…")]` (ROS types, whose
  // qualified name is the stable identity). Then it and its fields may name-hash;
  // otherwise every id must be pinned explicitly.
  let name_hash_mode = meta.name.is_some();
  let type_id_expr = id_expr(
    meta.id.clone(),
    &type_name,
    name_hash_mode,
    Span::call_site(),
  )?;

  let mut field_entries = Vec::new();
  let mut register_calls = Vec::new();
  let mut field_idents = Vec::new();
  let mut field_temps = Vec::new();
  let mut encode_fields = Vec::new();
  let mut decode_fields = Vec::new();
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

    // The value-plane conversions of this field.
    let shape = if field_meta.keyvalue {
      Shape::KeyValue(field.ty.clone())
    } else {
      shape_of(&field.ty)?
    };
    let what = format!("field `{}` of `{}`", fname_str, name_str);
    let temp_in = format_ident!("__field_{}", field_idents.len());
    let encode = shape.encode(quote! { #temp_in });
    encode_fields.push(quote! {
      arora_types::value::StructureField { id: #field_id_expr, value: ::std::boxed::Box::new(#encode) }
    });
    let decode = shape.decode(&what);
    let temp = format_ident!("__field_{}", field_idents.len());
    decode_fields.push(quote! {
      let #temp = {
        let __id = #field_id_expr;
        let __value = __fields
          .iter()
          .find(|__field| __field.id == __id)
          .map(|__field| (*__field.value).clone())
          .ok_or_else(|| arora_types::value::ConversionError { message: format!("missing {}", #what) })?;
        #decode
      };
    });
    field_temps.push(temp);
    field_idents.push(fname.clone());
  }
  let version_fn = version_fn(&meta);
  let mismatch = format!("expected a `{}` structure", name_str);

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

      #version_fn
    }

    impl ::std::convert::From<#name> for arora_types::value::Value {
      fn from(value: #name) -> arora_types::value::Value {
        let #name { #(#field_idents: #field_temps),* } = value;
        arora_types::value::Value::Structure(arora_types::value::Structure {
          id: <#name as arora_types::AroraType>::arora_type_id(),
          fields: vec![ #(#encode_fields),* ],
        })
      }
    }

    impl ::std::convert::TryFrom<arora_types::value::Value> for #name {
      type Error = arora_types::value::ConversionError;
      fn try_from(value: arora_types::value::Value) -> ::std::result::Result<Self, Self::Error> {
        let arora_types::value::Value::Structure(arora_types::value::Structure { id: __id, fields: __fields }) = value else {
          return Err(arora_types::value::ConversionError { message: #mismatch.to_string() });
        };
        if __id != <#name as arora_types::AroraType>::arora_type_id() {
          return Err(arora_types::value::ConversionError {
            message: format!("{}, got structure {}", #mismatch, __id),
          });
        }
        #(#decode_fields)*
        Ok(#name { #(#field_idents: #field_temps),* })
      }
    }
  })
}

/// Derive `AroraType` for an enum: a `ty::low` enumeration whose values are the
/// variants, each referenced by its pinned id. Only **unit** variants are
/// supported (a payload-carrying variant needs a `ty::low` reference for its
/// data, pending a model extension) — enough for the value-plane enums Arora
/// exchanges (e.g. the behavior `Status`). The enum id and each variant id pin
/// with `#[arora(id = "…")]`, exactly like a struct and its fields; the produced
/// `Value::Enumeration { id, variant_id, value: Unit }` form is what the value
/// plane already speaks.
fn expand_enum(input: &DeriveInput, data: &syn::DataEnum) -> syn::Result<TokenStream2> {
  let name = &input.ident;
  let name_str = name.to_string();

  let meta = parse_arora_meta(&input.attrs)?;
  let type_name = meta.name.clone().unwrap_or_else(|| name_str.clone());
  let name_hash_mode = meta.name.is_some();
  let type_id_expr = id_expr(
    meta.id.clone(),
    &type_name,
    name_hash_mode,
    Span::call_site(),
  )?;
  let version_fn = version_fn(&meta);
  let mismatch = format!("expected a `{}` enumeration", name_str);

  let mut value_entries = Vec::new();
  let mut encode_arms = Vec::new();
  let mut decode_arms = Vec::new();
  for variant in &data.variants {
    if !matches!(variant.fields, Fields::Unit) {
      return Err(syn::Error::new_spanned(
        &variant.fields,
        "AroraType enums support only unit variants (a variant payload needs a \
         `ty::low` reference, pending a model extension)",
      ));
    }
    let vname = variant.ident.to_string();
    let vident = &variant.ident;
    let variant_meta = parse_arora_meta(&variant.attrs)?;
    let variant_id_expr = id_expr(variant_meta.id, &vname, name_hash_mode, variant.span())?;
    encode_arms.push(quote! { #name::#vident => #variant_id_expr });
    decode_arms.push(quote! { x if x == #variant_id_expr => #name::#vident });
    value_entries.push(quote! {
      (
        #variant_id_expr,
        arora_types::ty::low::EnumerationValue {
          name: #vname.to_string(),
          // A unit variant carries no payload: it references the unit primitive,
          // so the value form is `Value::Enumeration { id, variant_id, Unit }`.
          type_ref: arora_types::module::low::TypeRef::Scalar { id: *arora_types::ty::UNIT_ID },
        },
      )
    });
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
          description: ::std::string::String::new(),
          kind: arora_types::ty::low::TypeKind::Enumeration(
            arora_types::ty::low::Enumeration {
              values: [ #(#value_entries),* ].into_iter().collect(),
            },
          ),
        }
      }

      fn register_types(registry: &mut arora_types::ty::TypeRegistry) {
        let id = <Self as arora_types::AroraType>::arora_type_id();
        if registry.contains_key(&id) {
          return;
        }
        registry.insert(id, <Self as arora_types::AroraType>::arora_type());
      }

      #version_fn
    }

    impl ::std::convert::From<#name> for arora_types::value::Value {
      fn from(value: #name) -> arora_types::value::Value {
        arora_types::value::Value::Enumeration(arora_types::value::Enumeration {
          id: <#name as arora_types::AroraType>::arora_type_id(),
          variant_id: match value { #(#encode_arms),* },
          value: ::std::boxed::Box::new(arora_types::value::Value::Unit),
        })
      }
    }

    impl ::std::convert::TryFrom<arora_types::value::Value> for #name {
      type Error = arora_types::value::ConversionError;
      fn try_from(value: arora_types::value::Value) -> ::std::result::Result<Self, Self::Error> {
        let arora_types::value::Value::Enumeration(e) = &value else {
          return Err(arora_types::value::ConversionError { message: #mismatch.to_string() });
        };
        if e.id != <#name as arora_types::AroraType>::arora_type_id() {
          return Err(arora_types::value::ConversionError {
            message: format!("{}, got enumeration {}", #mismatch, e.id),
          });
        }
        Ok(match e.variant_id {
          #(#decode_arms,)*
          other => return Err(arora_types::value::ConversionError {
            message: format!("{}: unknown variant {}", #mismatch, other),
          }),
        })
      }
    }
  })
}

/// The `arora_type_version` override, when `#[arora(version = "…")]` is set.
fn version_fn(meta: &AroraMeta) -> TokenStream2 {
  match meta.version {
    Some((major, minor, patch)) => quote! {
      fn arora_type_version() -> arora_types::record::Version {
        arora_types::record::Version::from(arora_types::SemanticVersion {
          major: #major,
          minor: #minor,
          patch: #patch,
        })
      }
    },
    None => quote! {},
  }
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
  /// `#[arora(version = "major.minor.patch")]`: the record version the type
  /// is pinned at; `1.0.0` when absent.
  version: Option<(u32, u32, u32)>,
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
      } else if meta.path.is_ident("version") {
        let lit: syn::LitStr = meta.value()?.parse()?;
        let parts: Vec<u32> = lit
          .value()
          .split('.')
          .map(|p| p.parse::<u32>())
          .collect::<Result<_, _>>()
          .map_err(|_| syn::Error::new(lit.span(), "version must be `major.minor.patch`"))?;
        if parts.len() != 3 {
          return Err(syn::Error::new(lit.span(), "version must be `major.minor.patch`"));
        }
        parsed.version = Some((parts[0], parts[1], parts[2]));
        Ok(())
      } else {
        Err(meta.error(
          "unknown `arora` attribute (expected `id = \"…\"`, `name = \"…\"`, `version = \"…\"`, or `keyvalue`)",
        ))
      }
    })?;
  }
  Ok(parsed)
}

/// Validate an id literal — hex UUID or thirteen emoji — at macro time and
/// emit it as a `Uuid::from_bytes`.
fn uuid_bytes_expr(literal: &str, span: Span) -> syn::Result<TokenStream2> {
  let uuid = arora_id::parse(literal).map_err(|e| syn::Error::new(span, e.to_string()))?;
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

// ---- value-plane conversions -----------------------------------------------

/// How a field's Rust type crosses the value plane. Mirrors the schema
/// classification above; the two are produced from the same syntax so they
/// cannot disagree.
enum Shape {
  Primitive {
    variant: &'static str,
  },
  /// `Option<T>`.
  Option(Box<Shape>),
  /// `Vec<T>`.
  Vec(Box<Shape>),
  /// `[T; N]`.
  FixedArray(Box<Shape>, syn::Expr),
  /// A nested type converting through its own `From`/`TryFrom<Value>`.
  Nested(Type),
  /// `#[arora(keyvalue)]`: `Value` passes as-is; anything else goes through
  /// `value_serde`.
  KeyValue(Type),
}

fn shape_of(ty: &Type) -> syn::Result<Shape> {
  if let Type::Array(array) = ty {
    return Ok(Shape::FixedArray(
      Box::new(shape_of(&array.elem)?),
      array.len.clone(),
    ));
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
  match ident.as_str() {
    "Vec" => Ok(Shape::Vec(Box::new(shape_of(single_type_arg(
      segment, "Vec",
    )?)?))),
    "Option" => Ok(Shape::Option(Box::new(shape_of(single_type_arg(
      segment, "Option",
    )?)?))),
    _ => Ok(match primitive_variant(&ident) {
      Some(variant) => Shape::Primitive { variant },
      None => Shape::Nested(ty.clone()),
    }),
  }
}

/// The `Value` variant a Rust primitive travels as.
fn primitive_variant(ident: &str) -> Option<&'static str> {
  Some(match ident {
    "bool" => "Boolean",
    "i8" => "I8",
    "i16" => "I16",
    "i32" => "I32",
    "i64" => "I64",
    "u8" => "U8",
    "u16" => "U16",
    "u32" => "U32",
    "u64" => "U64",
    "f32" => "F32",
    "f64" => "F64",
    "String" => "String",
    "Uuid" => "Uuid",
    _ => return None,
  })
}

impl Shape {
  /// An expression converting `expr` (owned, of this shape) to a `Value`.
  fn encode(&self, expr: TokenStream2) -> TokenStream2 {
    match self {
      Shape::Primitive { .. } | Shape::Nested(_) => {
        quote! { <arora_types::value::Value as ::std::convert::From<_>>::from(#expr) }
      }
      Shape::Option(inner) => {
        let e = inner.encode(quote! { __inner });
        quote! {
          arora_types::value::Value::Option((#expr).map(|__inner| ::std::boxed::Box::new(#e)))
        }
      }
      Shape::Vec(inner) | Shape::FixedArray(inner, _) => {
        let e = inner.encode(quote! { __element });
        let element_id = inner.element_id();
        quote! {
          arora_types::value::Value::array_of(
            #element_id,
            (#expr).into_iter().map(|__element| #e).collect(),
          )
        }
      }
      Shape::KeyValue(ty) => {
        if is_value_type(ty) {
          quote! { #expr }
        } else {
          quote! {
            arora_types::value_serde::to_value(&#expr)
              .expect("a serializable field converts to a Value")
          }
        }
      }
    }
  }

  /// The id of this shape's type — an array's element type.
  fn element_id(&self) -> TokenStream2 {
    match self {
      Shape::Primitive { variant } => {
        let id = syn::Ident::new(
          match *variant {
            "Boolean" => "BOOLEAN_ID",
            "I8" => "I8_ID",
            "I16" => "I16_ID",
            "I32" => "I32_ID",
            "I64" => "I64_ID",
            "U8" => "U8_ID",
            "U16" => "U16_ID",
            "U32" => "U32_ID",
            "U64" => "U64_ID",
            "F32" => "F32_ID",
            "F64" => "F64_ID",
            "String" => "STRING_ID",
            _ => "UUID_ID",
          },
          Span::call_site(),
        );
        quote! { *arora_types::ty::#id }
      }
      Shape::Nested(ty) => quote! { <#ty as arora_types::AroraType>::arora_type_id() },
      Shape::Option(_) => quote! { *arora_types::ty::OPTION_ID },
      Shape::Vec(_) | Shape::FixedArray(..) => quote! { *arora_types::ty::ARRAY_VALUE_ID },
      Shape::KeyValue(_) => quote! { *arora_types::ty::KEY_VALUE_ID },
    }
  }

  /// An expression reading `__value: Value` as this shape, or returning a
  /// `ConversionError` naming `what`.
  fn decode(&self, what: &str) -> TokenStream2 {
    let fail = |message: TokenStream2| {
      quote! {
        return Err(arora_types::value::ConversionError { message: #message })
      }
    };
    match self {
      Shape::Primitive { variant } => {
        let variant = syn::Ident::new(variant, Span::call_site());
        let mismatch = fail(quote! { format!("{}: unexpected value {}", #what, other) });
        quote! { match __value { arora_types::value::Value::#variant(x) => x, other => #mismatch } }
      }
      Shape::Nested(ty) => quote! {
        <#ty as ::std::convert::TryFrom<arora_types::value::Value>>::try_from(__value)
          .map_err(|e| arora_types::value::ConversionError { message: format!("{}: {}", #what, e) })?
      },
      Shape::Option(inner) => {
        let d = inner.decode(what);
        let mismatch = fail(quote! { format!("{}: expected an option, got {}", #what, other) });
        quote! {
          match __value {
            arora_types::value::Value::Option(None) => None,
            arora_types::value::Value::Option(Some(__boxed)) => Some({ let __value = *__boxed; #d }),
            other => #mismatch,
          }
        }
      }
      Shape::Vec(inner) => {
        let d = inner.decode(what);
        quote! {
          {
            let mut __out = ::std::vec::Vec::new();
            for __value in __value.into_elements().map_err(|e| arora_types::value::ConversionError { message: format!("{}: {}", #what, e) })? {
              __out.push({ #d });
            }
            __out
          }
        }
      }
      Shape::FixedArray(inner, len) => {
        let d = inner.decode(what);
        let wrong_len =
          fail(quote! { format!("{}: expected {} elements, got {}", #what, #len, __out.len()) });
        quote! {
          {
            let mut __out = ::std::vec::Vec::new();
            for __value in __value.into_elements().map_err(|e| arora_types::value::ConversionError { message: format!("{}: {}", #what, e) })? {
              __out.push({ #d });
            }
            match <[_; #len]>::try_from(__out) {
              Ok(array) => array,
              Err(__out) => #wrong_len,
            }
          }
        }
      }
      Shape::KeyValue(ty) => {
        if is_value_type(ty) {
          quote! { __value }
        } else {
          quote! {
            arora_types::value_serde::from_value(__value)
              .map_err(|e| arora_types::value::ConversionError { message: format!("{}: {}", #what, e) })?
          }
        }
      }
    }
  }
}

/// Whether a type path names arora's `Value` itself.
fn is_value_type(ty: &Type) -> bool {
  matches!(ty, Type::Path(p) if p.path.segments.last().map(|s| s.ident == "Value").unwrap_or(false))
}

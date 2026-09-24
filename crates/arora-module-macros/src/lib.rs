//! The declaration macros: an Arora module declared on a Rust module.
//!
//! - `#[export(id = "…")]` on a function, with `#[param(id = "…")]` on each
//!   parameter, declares one exported function. It keeps the function and
//!   emits a hidden sibling module holding the function's ids, its header
//!   export, its frozen signature, the invocation (arguments out of a `Call`
//!   **by parameter id**), the client stub, and the `#[no_mangle]` entry point
//!   an executor looks up when the module is built as an artifact.
//! - `declare_module! { id = "…", name = "…", …, exports = [a, b] }`
//!   aggregates those into `ids`, `header(executor)`, `record(parent)`,
//!   `exports()`, `client`, and a marker type `Module` implementing
//!   [`AroraModule`](arora_types::module::declared::AroraModule). A
//!   declaration names no executor: only the step that builds the artifact
//!   knows whether it is native or wasm, so `header` takes it there.
//! - `#[module(id = "…", …)]` on an **inline** Rust module is sugar: it scans
//!   the module for `#[export]` functions and injects `declare_module!`.
//! - `module_from_header!("…/module.yaml", types = ["<uuid>" => Type, …])`
//!   is the consumer side for a module that is not a Rust declaration: ids
//!   and typed stubs from its resolved header.
//!
//! The macros are re-exported by `arora-module`, which is what a module crate
//! depends on; generated code reaches `arora-types` and `arora-buffers`
//! through that facade.
//!
//! Type mapping follows `#[derive(AroraType)]`: a Rust primitive maps by table
//! to its well-known id, `PrimitiveKind` and `Value` variant; `Vec<T>` to an
//! array of `T`; `arora_types::value::Value` to the dynamic KeyValue type; any
//! other path is taken to be an `AroraType` that also converts through
//! `From`/`TryFrom<Value>`. `&mut T` is a mutable parameter. `Option` and maps
//! are rejected: the record vocabulary cannot express them.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    parse_macro_input, Attribute, FnArg, Ident, Item, ItemFn, ItemMod, Pat, Path, ReturnType,
    Token, Type,
};

// =============================================================================
// Type mapping
// =============================================================================

/// How a Rust type maps onto arora's vocabularies.
enum Kind {
    Unit,
    Primitive {
        kind: &'static str,
        id: &'static str,
        variant: &'static str,
    },
    /// `Vec<T>`: a homogeneous array of `T` (a primitive or a user structure).
    Array(Box<Kind>),
    /// `arora_types::value::Value`: a dynamically typed value (the KeyValue type).
    Dynamic,
    /// Anything else: an `AroraType` converting through `Into`/`TryFrom<Value>`.
    User(Type),
}

fn primitive(ident: &str) -> Option<Kind> {
    let prim = |kind, id, variant| Kind::Primitive { kind, id, variant };
    Some(match ident {
        "bool" => prim("Boolean", "BOOLEAN_ID", "Boolean"),
        "u8" => prim("U8", "U8_ID", "U8"),
        "u16" => prim("U16", "U16_ID", "U16"),
        "u32" => prim("U32", "U32_ID", "U32"),
        "u64" => prim("U64", "U64_ID", "U64"),
        "i8" => prim("I8", "I8_ID", "I8"),
        "i16" => prim("I16", "I16_ID", "I16"),
        "i32" => prim("I32", "I32_ID", "I32"),
        "i64" => prim("I64", "I64_ID", "I64"),
        "f32" => prim("F32", "F32_ID", "F32"),
        "f64" => prim("F64", "F64_ID", "F64"),
        "String" => prim("String", "STRING_ID", "String"),
        _ => return None,
    })
}

fn kind_of(ty: &Type) -> syn::Result<Kind> {
    if let Type::Tuple(t) = ty {
        if t.elems.is_empty() {
            return Ok(Kind::Unit);
        }
    }
    let Type::Path(path) = ty else {
        return Err(syn::Error::new(
            ty.span(),
            "unsupported type (expected a named type)",
        ));
    };
    let segment = path
        .path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new(ty.span(), "empty type path"))?;
    let ident = segment.ident.to_string();
    if let Some(kind) = primitive(&ident) {
        return Ok(kind);
    }
    match ident.as_str() {
        "Vec" => {
            let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                return Err(syn::Error::new(ty.span(), "`Vec` needs a type argument"));
            };
            let Some(syn::GenericArgument::Type(element)) = args.args.first() else {
                return Err(syn::Error::new(ty.span(), "`Vec` needs a type argument"));
            };
            match kind_of(element)? {
                Kind::Array(_) | Kind::Unit | Kind::Dynamic => Err(syn::Error::new(
                    ty.span(),
                    "an array element is a primitive or a structure",
                )),
                element => Ok(Kind::Array(Box::new(element))),
            }
        }
        "Value" => Ok(Kind::Dynamic),
        "Option" | "HashMap" | "BTreeMap" | "Box" => Err(syn::Error::new(
            ty.span(),
            format!(
                "`{ident}` is not expressible in the record vocabulary (no optional or map form)"
            ),
        )),
        _ => Ok(Kind::User(ty.clone())),
    }
}

impl Kind {
    /// The well-known or user type id this kind is referenced by (an element
    /// id for arrays).
    fn element_id(&self) -> TokenStream2 {
        match self {
            Kind::Unit => quote! { *arora_types::ty::UNIT_ID },
            Kind::Primitive { id, .. } => {
                let id = format_ident!("{id}");
                quote! { *arora_types::ty::#id }
            }
            Kind::Dynamic => quote! { *arora_types::ty::KEY_VALUE_ID },
            Kind::User(ty) => quote! { <#ty as arora_types::AroraType>::arora_type_id() },
            Kind::Array(_) => unreachable!("an array of arrays is rejected at parse time"),
        }
    }

    /// The `module::low::TypeRef` a header declares this kind as.
    fn type_ref(&self) -> TokenStream2 {
        match self {
            Kind::Array(element) => {
                let id = element.element_id();
                quote! { arora_types::module::low::TypeRef::Array { id: #id } }
            }
            other => {
                let id = other.element_id();
                quote! { arora_types::module::low::TypeRef::Scalar { id: #id } }
            }
        }
    }

    /// The `record::ty::FrozenTy` a described signature carries. A user type
    /// is pinned at the version its declaration carries (`#[arora(version)]`,
    /// `1.0.0` when absent).
    fn frozen_ty(&self) -> TokenStream2 {
        let reference = |ty: &Type| {
            quote! {
              arora_types::record::FrozenReference {
                id: <#ty as arora_types::AroraType>::arora_type_id(),
                version: <#ty as arora_types::AroraType>::arora_type_version(),
              }
            }
        };
        let dynamic_reference = quote! {
          arora_types::record::FrozenReference {
            id: *arora_types::ty::KEY_VALUE_ID,
            version: arora_types::record::Version::from(arora_types::SemanticVersion { major: 1, minor: 0, patch: 0 }),
          }
        };
        match self {
            Kind::Unit => {
                quote! { arora_types::record::ty::FrozenTy::from(arora_types::record::ty::PrimitiveKind::Unit) }
            }
            Kind::Primitive { kind, .. } => {
                let kind = format_ident!("{kind}");
                quote! { arora_types::record::ty::FrozenTy::from(arora_types::record::ty::PrimitiveKind::#kind) }
            }
            Kind::Dynamic => quote! {
              arora_types::record::ty::FrozenTy::FrozenScalar(arora_types::record::ty::FrozenScalar { reference: #dynamic_reference })
            },
            Kind::User(ty) => {
                let reference = reference(ty);
                quote! { arora_types::record::ty::FrozenTy::FrozenScalar(arora_types::record::ty::FrozenScalar { reference: #reference }) }
            }
            Kind::Array(element) => match &**element {
                Kind::Primitive { kind, .. } => {
                    let kind = format_ident!("Array{kind}");
                    quote! { arora_types::record::ty::FrozenTy::from(arora_types::record::ty::PrimitiveKind::#kind) }
                }
                Kind::User(ty) => {
                    let reference = reference(ty);
                    quote! { arora_types::record::ty::FrozenTy::FrozenArray(arora_types::record::ty::FrozenArray { reference: #reference }) }
                }
                _ => unreachable!("rejected at parse time"),
            },
        }
    }

    /// The Rust type this kind is passed and returned as.
    fn rust_type(&self) -> TokenStream2 {
        match self {
            Kind::Unit => quote! { () },
            Kind::Primitive { variant, .. } => {
                let t = match *variant {
                    "Boolean" => "bool",
                    "U8" => "u8",
                    "U16" => "u16",
                    "U32" => "u32",
                    "U64" => "u64",
                    "I8" => "i8",
                    "I16" => "i16",
                    "I32" => "i32",
                    "I64" => "i64",
                    "F32" => "f32",
                    "F64" => "f64",
                    _ => "String",
                };
                let t = format_ident!("{t}");
                quote! { #t }
            }
            Kind::Dynamic => quote! { arora_types::value::Value },
            Kind::User(ty) => quote! { #ty },
            Kind::Array(element) => {
                let e = element.rust_type();
                quote! { ::std::vec::Vec<#e> }
            }
        }
    }

    /// Decode `__value: Value` into this kind; `what` names the place for the
    /// error. Evaluates to the decoded value or `return Err(#fail(msg))`.
    fn decode(&self, what: &str, fail: &TokenStream2) -> TokenStream2 {
        let mismatch = quote! {
          other => return Err(#fail(format!("{}: unexpected value {}", #what, other)))
        };
        match self {
            Kind::Unit => quote! { () },
            Kind::Primitive { variant, .. } => {
                let variant = format_ident!("{variant}");
                quote! { match __value { arora_types::value::Value::#variant(x) => x, #mismatch } }
            }
            Kind::Dynamic => quote! { __value },
            Kind::User(ty) => quote! {
              <#ty as ::std::convert::TryFrom<arora_types::value::Value>>::try_from(__value)
                .map_err(|_| #fail(format!("{}: value does not convert", #what)))?
            },
            Kind::Array(element) => match &**element {
                Kind::Primitive { variant, .. } => {
                    let variant = format_ident!("Array{variant}");
                    quote! { match __value { arora_types::value::Value::#variant(x) => x, #mismatch } }
                }
                Kind::User(ty) => quote! {
                  match __value {
                    arora_types::value::Value::ArrayStructure { id, elements }
                      if id == <#ty as arora_types::AroraType>::arora_type_id() =>
                    {
                      let mut out = ::std::vec::Vec::with_capacity(elements.len());
                      for element in elements {
                        let __value = arora_types::value::Value::Structure(arora_types::value::Structure { id, fields: element.fields });
                        out.push(
                          <#ty as ::std::convert::TryFrom<arora_types::value::Value>>::try_from(__value)
                            .map_err(|_| #fail(format!("{}: an element does not convert", #what)))?,
                        );
                      }
                      out
                    }
                    #mismatch
                  }
                },
                _ => unreachable!("rejected at parse time"),
            },
        }
    }

    /// Encode a value of this kind as a `Value`.
    fn encode(&self, expr: TokenStream2) -> TokenStream2 {
        match self {
            Kind::Unit => quote! { { let _ = #expr; arora_types::value::Value::Unit } },
            Kind::Dynamic => quote! { #expr },
            Kind::Primitive { .. } | Kind::User(_) => {
                quote! { <arora_types::value::Value as ::std::convert::From<_>>::from(#expr) }
            }
            Kind::Array(element) => match &**element {
                Kind::Primitive { variant, .. } => {
                    let variant = format_ident!("Array{variant}");
                    quote! { arora_types::value::Value::#variant(#expr) }
                }
                Kind::User(ty) => quote! {
                  arora_types::value::Value::ArrayStructure {
                    id: <#ty as arora_types::AroraType>::arora_type_id(),
                    elements: (#expr).into_iter().map(|element| {
                      match <arora_types::value::Value as ::std::convert::From<_>>::from(element) {
                        arora_types::value::Value::Structure(s) => arora_types::value::StructureWithoutId { fields: s.fields },
                        _ => unreachable!("a structure converts to Value::Structure"),
                      }
                    }).collect(),
                  }
                },
                _ => unreachable!("rejected at parse time"),
            },
        }
    }
}

// =============================================================================
// Attribute parsing
// =============================================================================

fn uuid_expr(literal: &str, span: Span) -> syn::Result<TokenStream2> {
    let uuid = uuid::Uuid::parse_str(literal)
        .map_err(|e| syn::Error::new(span, format!("invalid uuid: {e}")))?;
    let bytes = uuid.as_bytes().iter().map(|b| quote! { #b });
    Ok(quote! { arora_types::Uuid::from_bytes([ #(#bytes),* ]) })
}

/// An id literal and where it was written, so a bad one is reported there.
type SpannedId = (String, Span);

/// Parse `#[<name>(id = "…", name = "…")]` out of `attrs`, removing it.
fn take_id_attr(
    attrs: &mut Vec<Attribute>,
    attr_name: &str,
) -> syn::Result<Option<(SpannedId, Option<String>)>> {
    let Some(pos) = attrs.iter().position(|a| a.path().is_ident(attr_name)) else {
        return Ok(None);
    };
    let attr = attrs.remove(pos);
    let mut id = None;
    let mut name = None;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("id") {
            let lit: syn::LitStr = meta.value()?.parse()?;
            id = Some((lit.value(), lit.span()));
            Ok(())
        } else if meta.path.is_ident("name") {
            let lit: syn::LitStr = meta.value()?.parse()?;
            name = Some(lit.value());
            Ok(())
        } else {
            Err(meta.error(format!(
                "unknown `{attr_name}` attribute (expected `id = \"…\"`)"
            )))
        }
    })?;
    let id = id.ok_or_else(|| {
        syn::Error::new(
            attr.span(),
            format!(
        "#[{attr_name}] requires an explicit `id = \"<uuid>\"` — a name-hash id changes on rename"
      ),
        )
    })?;
    Ok(Some((id, name)))
}

// =============================================================================
// #[export]
// =============================================================================

struct Param {
    name: String,
    ident: Ident,
    id: SpannedId,
    kind: Kind,
    mutable: bool,
}

struct Export {
    name: String,
    ident: Ident,
    id: SpannedId,
    params: Vec<Param>,
    ret: Kind,
}

fn parse_export(f: &mut ItemFn, attr: TokenStream2) -> syn::Result<Export> {
    // The attribute's own arguments arrive separately; reuse the id parser by
    // re-attaching them as an attribute.
    let attr: Attribute = syn::parse_quote! { #[export(#attr)] };
    let mut attrs = vec![attr];
    let Some((id, name)) = take_id_attr(&mut attrs, "export")? else {
        unreachable!("just attached");
    };
    let ident = f.sig.ident.clone();
    let name = name.unwrap_or_else(|| ident.to_string());
    let mut params = Vec::new();
    for arg in &mut f.sig.inputs {
        let FnArg::Typed(pt) = arg else {
            return Err(syn::Error::new(
                arg.span(),
                "an exported function takes no `self`",
            ));
        };
        let Pat::Ident(pat) = &*pt.pat else {
            return Err(syn::Error::new(
                pt.pat.span(),
                "parameters must be plain identifiers",
            ));
        };
        let Some((param_id, param_name)) = take_id_attr(&mut pt.attrs, "param")? else {
            return Err(syn::Error::new(
                pt.span(),
                "every parameter of an exported function needs `#[param(id = \"<uuid>\")]`",
            ));
        };
        let (mutable, ty) = match &*pt.ty {
            Type::Reference(r) if r.mutability.is_some() => (true, (*r.elem).clone()),
            Type::Reference(r) => {
                return Err(syn::Error::new(
                    r.span(),
                    "a parameter is by value or `&mut T`",
                ))
            }
            other => (false, other.clone()),
        };
        params.push(Param {
            name: param_name.unwrap_or_else(|| pat.ident.to_string()),
            ident: pat.ident.clone(),
            id: param_id,
            kind: kind_of(&ty)?,
            mutable,
        });
    }
    let ret = match &f.sig.output {
        ReturnType::Default => Kind::Unit,
        ReturnType::Type(_, ty) => kind_of(ty)?,
    };
    Ok(Export {
        name,
        ident,
        id,
        params,
        ret,
    })
}

fn export_module_ident(fn_ident: &Ident) -> Ident {
    format_ident!("__arora_export_{}", fn_ident)
}

fn upper_snake(name: &str) -> String {
    name.to_uppercase()
}

/// `#[export(id = "…")]`: keep the function, emit its declaration beside it.
#[proc_macro_attribute]
pub fn export(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut f = parse_macro_input!(item as ItemFn);
    expand_export(&mut f, attr.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_export(f: &mut ItemFn, attr: TokenStream2) -> syn::Result<TokenStream2> {
    let export = parse_export(f, attr)?;
    let fn_ident = &export.ident;
    let fn_name = &export.name;
    let fn_id = uuid_expr(&export.id.0, export.id.1)?;
    let module_ident = export_module_ident(fn_ident);
    let fail = quote! { (|message: ::std::string::String| arora_types::call::CallError::Guest { message }) };

    let param_consts = export
        .params
        .iter()
        .map(|p| {
            let c = format_ident!("{}", upper_snake(&p.name));
            let id = uuid_expr(&p.id.0, p.id.1)?;
            let doc = format!("`{}.{}`: {}", fn_name, p.name, p.id.0);
            Ok(quote! { #[doc = #doc] pub const #c: arora_types::Uuid = #id; })
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let fn_doc = format!("`{}`: {}", fn_name, export.id.0);

    let header_params = export.params.iter().map(|p| {
        let c = format_ident!("{}", upper_snake(&p.name));
        let name = &p.name;
        let ty = p.kind.type_ref();
        let mutable = p.mutable;
        quote! {
          arora_types::module::low::Parameter {
            id: ids::#c, name: #name.to_string(), ty: #ty, mutable: #mutable,
            default_value: ::std::option::Option::None,
          }
        }
    });
    let ret_ref = export.ret.type_ref();

    let frozen_params = export.params.iter().map(|p| {
        let c = format_ident!("{}", upper_snake(&p.name));
        let name = &p.name;
        let ty = p.kind.frozen_ty();
        let mutable = p.mutable;
        quote! {
          parameter_ordering.push(ids::#c);
          parameters.insert(ids::#c, arora_types::record::module::frozen::Parameter {
            name: #name.to_string(), ty: #ty, mutable: #mutable,
          });
        }
    });
    let ret_frozen = export.ret.frozen_ty();
    let dependency_pushes: Vec<TokenStream2> = export
        .params
        .iter()
        .map(|p| &p.kind)
        .chain(std::iter::once(&export.ret))
        .filter_map(|kind| match kind {
            Kind::User(ty) => Some(ty),
            Kind::Array(element) => match &**element {
                Kind::User(ty) => Some(ty),
                _ => None,
            },
            _ => None,
        })
        .map(|ty| {
            quote! {
              out.push(arora_types::record::FrozenReference {
                id: <#ty as arora_types::AroraType>::arora_type_id(),
                version: <#ty as arora_types::AroraType>::arora_type_version(),
              });
            }
        })
        .collect();

    let decode_params = export.params.iter().map(|p| {
        let c = format_ident!("{}", upper_snake(&p.name));
        let ident = &p.ident;
        let what = format!("parameter `{}` of `{}`", p.name, fn_name);
        let decode = p.kind.decode(&what, &fail);
        let mutability = if p.mutable {
            quote! { mut }
        } else {
            quote! {}
        };
        let missing = format!("missing parameter `{}` of `{}`", p.name, fn_name);
        quote! {
          let #mutability #ident = {
            let __value = call.args.iter().find(|field| field.id == ids::#c)
              .map(|field| (*field.value).clone())
              .ok_or_else(|| #fail(#missing.to_string()))?;
            #decode
          };
        }
    });
    let call_args = export.params.iter().map(|p| {
        let ident = &p.ident;
        if p.mutable {
            quote! { &mut #ident }
        } else {
            quote! { #ident }
        }
    });
    let push_mutated = export.params.iter().filter(|p| p.mutable).map(|p| {
    let c = format_ident!("{}", upper_snake(&p.name));
    let ident = &p.ident;
    let encode = p.kind.encode(quote! { #ident });
    quote! {
      __mutated.push(arora_types::value::StructureField { id: ids::#c, value: ::std::boxed::Box::new(#encode) });
    }
  });
    let encode_ret = export.ret.encode(quote! { __ret });

    // The client stub: build the call by id, dispatch through a bridge, decode.
    let stub_params = export.params.iter().map(|p| {
        let ident = &p.ident;
        let ty = p.kind.rust_type();
        if p.mutable {
            quote! { #ident: &mut #ty }
        } else {
            quote! { #ident: #ty }
        }
    });
    let stub_args = export.params.iter().map(|p| {
    let c = format_ident!("{}", upper_snake(&p.name));
    let ident = &p.ident;
    let encode = if p.mutable { p.kind.encode(quote! { ::std::clone::Clone::clone(&*#ident) }) } else { p.kind.encode(quote! { #ident }) };
    quote! { arora_types::value::StructureField { id: ids::#c, value: ::std::boxed::Box::new(#encode) } }
  });
    let stub_read_mutated = export.params.iter().filter(|p| p.mutable).map(|p| {
        let c = format_ident!("{}", upper_snake(&p.name));
        let ident = &p.ident;
        let what = format!("mutated parameter `{}` of `{}`", p.name, fn_name);
        let decode = p.kind.decode(&what, &fail);
        quote! {
          if let Some(field) = __result.mutated.iter().find(|field| field.id == ids::#c) {
            let __value = (*field.value).clone();
            *#ident = #decode;
          }
        }
    });
    let ret_ty = export.ret.rust_type();
    let ret_what = format!("return value of `{}`", fn_name);
    let decode_ret = export.ret.decode(&ret_what, &fail);
    let ret_needs_value = !matches!(export.ret, Kind::Unit);

    let shim_ident = format_ident!("arora_function_{}", export.id.0.replace('-', "_"));
    let client_macro_ident = format_ident!("__arora_client_{}", fn_ident);
    let client_params: Vec<TokenStream2> = export
        .params
        .iter()
        .map(|p| {
            let ident = &p.ident;
            let ty = p.kind.rust_type();
            if p.mutable {
                quote! { #ident: &mut #ty }
            } else {
                quote! { #ident: #ty }
            }
        })
        .collect();
    let client_args: Vec<TokenStream2> = export
        .params
        .iter()
        .map(|p| {
            let ident = &p.ident;
            quote! { #ident }
        })
        .collect();

    Ok(quote! {
      #f

      /// Left for the module's aggregate: the client stub bound to the module
      /// id the aggregate knows and this function does not. Textually scoped —
      /// the aggregate comes after the functions it names.
      #[doc(hidden)]
      #[allow(unused_macros)]
      macro_rules! #client_macro_ident {
        ($module_id:expr) => {
          /// The stub for this export, bound to the module's id.
          pub fn #fn_ident<B: arora_types::call::CallBridge + ?Sized>(
            bridge: &mut B,
            #(#client_params),*
          ) -> ::std::result::Result<#ret_ty, arora_types::call::CallError> {
            super::#module_ident::call(bridge, $module_id, #(#client_args),*)
          }
        };
      }

      #[doc(hidden)]
      #[allow(non_snake_case, unused_imports, clippy::all)]
      pub mod #module_ident {
        use super::*;
        use ::arora_module::__rt::{buffers as arora_buffers, types as arora_types};

        pub mod ids {
          use ::arora_module::__rt::types as arora_types;

          #[doc = #fn_doc]
          pub const FUNCTION: arora_types::Uuid = #fn_id;
          #(#param_consts)*
        }

        /// The export as a header declares it.
        pub fn export() -> arora_types::module::low::ExportSymbol {
          arora_types::module::low::ExportSymbol::Function(arora_types::module::low::ExportFunction {
            id: ids::FUNCTION,
            name: #fn_name.to_string(),
            parameters: vec![ #(#header_params),* ],
            ret: #ret_ref,
          })
        }

        /// The record references this function's signature pins (its user
        /// types, versioned) — a module record's dependencies.
        pub fn dependencies() -> ::std::vec::Vec<arora_types::record::FrozenReference> {
          let mut out = ::std::vec::Vec::new();
          #(#dependency_pushes)*
          out
        }

        /// The frozen signature a described function carries.
        pub fn signature() -> arora_types::record::module::frozen::Function {
          let mut parameters = ::std::collections::HashMap::new();
          let mut parameter_ordering = ::std::vec::Vec::new();
          #(#frozen_params)*
          arora_types::record::module::frozen::Function { parameters, parameter_ordering, return_ty: #ret_frozen }
        }

        /// The invocation: arguments out of the call by parameter id, the Rust
        /// function, the result back. Shared by the host closure and the guest
        /// shim — the ABI differs, the marshalling does not.
        pub fn invoke(call: arora_types::call::Call) -> ::std::result::Result<arora_types::call::CallResult, arora_types::call::CallError> {
          let mut __mutated: ::std::vec::Vec<arora_types::value::StructureField> = ::std::vec::Vec::new();
          #(#decode_params)*
          let __ret = super::#fn_ident(#(#call_args),*);
          #(#push_mutated)*
          ::std::result::Result::Ok(arora_types::call::CallResult { ret: #encode_ret, mutated: __mutated })
        }

        /// The export in its callable form: what a host registers, and what
        /// an artifact's shim wraps.
        pub fn arora_function() -> arora_types::module::declared::AroraFunction {
          arora_types::module::declared::AroraFunction {
            id: ids::FUNCTION,
            name: #fn_name,
            signature: signature(),
            invoke: ::std::boxed::Box::new(invoke),
          }
        }

        /// The client stub: the same function called through a `CallBridge` —
        /// the interface a consumer of this module programs against.
        pub fn call<B: arora_types::call::CallBridge + ?Sized>(
          bridge: &mut B,
          module_id: arora_types::Uuid,
          #(#stub_params),*
        ) -> ::std::result::Result<#ret_ty, arora_types::call::CallError> {
          let __result = bridge.arora_call(arora_types::call::Call {
            module_id: ::std::option::Option::Some(module_id),
            id: ids::FUNCTION,
            args: vec![ #(#stub_args),* ],
          })?;
          #(#stub_read_mutated)*
          let __value = __result.ret;
          let _ = #ret_needs_value;
          ::std::result::Result::Ok(#decode_ret)
        }

        /// The entry point an executor looks up by name when this module is
        /// built as an artifact — a wasm guest or a native shared library: the
        /// size-prefixed argument buffer in, the size-prefixed result buffer
        /// out, both in the `Value` wire format the engine speaks.
        #[no_mangle]
        pub extern "C" fn #shim_ident(input_addr: usize) -> usize {
          let input = unsafe {
            let size = u32::from_le_bytes(*(input_addr as *const [u8; 4])) as usize;
            ::std::slice::from_raw_parts(input_addr as *const u8, size)
          };
          let result = (|| -> ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> {
            let arora_types::value::Value::Structure(structure) = arora_buffers::serde_uuid::deserialize(input) else {
              return Err("argument buffer is not a structure".to_string());
            };
            if structure.id != ids::FUNCTION {
              return Err(format!("argument structure id {} differs from function id {}", structure.id, ids::FUNCTION));
            }
            let result = invoke(arora_types::call::Call { module_id: None, id: ids::FUNCTION, args: structure.fields })
              .map_err(|e| e.to_string())?;
            let mut fields = ::std::vec::Vec::with_capacity(1 + result.mutated.len());
            fields.push(arora_types::value::StructureField { id: ids::FUNCTION, value: ::std::boxed::Box::new(result.ret) });
            fields.extend(result.mutated);
            Ok(arora_buffers::serde_uuid::serialize(&arora_types::value::Value::Structure(arora_types::value::Structure { id: ids::FUNCTION, fields })))
          })();
          match result {
            Ok(buffer) => ::std::boxed::Box::leak(buffer).as_ptr() as usize,
            Err(message) => {
              let mut writer = arora_buffers::BufferWriter::new();
              writer.add_error(&message);
              ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
            }
          }
        }
      }
    })
}

// =============================================================================
// module! { … } and #[module]
// =============================================================================

struct ModuleArgs {
    id: SpannedId,
    name: Option<String>,
    version: (u32, u32, u32),
    author: String,
    license: String,
    description: Option<String>,
    executable_mime: String,
    exports: Option<Vec<Ident>>,
}

/// `key = "value", …, exports = [a, b]` — the attribute's and the macro's
/// argument syntax.
impl Parse for ModuleArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut id = None;
        let mut name = None;
        let mut version = (0, 0, 0);
        let mut author = String::new();
        let mut license = String::new();
        let mut description = None;
        let mut executable_mime = String::new();
        let mut exports = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "exports" {
                let content;
                syn::bracketed!(content in input);
                let list: Punctuated<Ident, Token![,]> =
                    content.parse_terminated(Ident::parse, Token![,])?;
                exports = Some(list.into_iter().collect());
            } else {
                let lit: syn::LitStr = input.parse()?;
                let (value, span) = (lit.value(), lit.span());
                match key.to_string().as_str() {
          "id" => id = Some((value, span)),
          "name" => name = Some(value),
          "version" => {
            let parts: Vec<u32> = value
              .split('.')
              .map(|p| p.parse::<u32>())
              .collect::<Result<_, _>>()
              .map_err(|_| syn::Error::new(span, "version must be `major.minor.patch`"))?;
            if parts.len() != 3 {
              return Err(syn::Error::new(span, "version must be `major.minor.patch`"));
            }
            version = (parts[0], parts[1], parts[2]);
          }
          "executor" => {
            return Err(syn::Error::new(
              key.span(),
              "a declaration does not name an executor: only the export that builds the artifact \
               knows whether it is native or wasm — `header(executor)` takes it there",
            ))
          }
          "author" => author = value,
          "license" => license = value,
          "description" => description = Some(value),
          "executable_mime" => executable_mime = value,
          other => {
            return Err(syn::Error::new(
              key.span(),
              format!("unknown module attribute `{other}`"),
            ))
          }
        }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        let id = id.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "a module declaration requires `id = \"<uuid>\"`",
            )
        })?;
        Ok(ModuleArgs {
            id,
            name,
            version,
            author,
            license,
            description,
            executable_mime,
            exports,
        })
    }
}

/// `declare_module! { id = "…", name = "…", …, exports = [a, b] }`: the
/// aggregate — `ids`, `header()`, `host_functions()` — from the named
/// `#[export]`s.
#[proc_macro]
pub fn declare_module(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as ModuleArgs);
    expand_aggregate(&args, None)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[module(id = "…", …)]` on an inline module: scan it for `#[export]`
/// functions and inject the `declare_module!` aggregate at its end.
#[proc_macro_attribute]
pub fn module(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ModuleArgs);
    let mut module = parse_macro_input!(item as ItemMod);
    expand_module_attr(args, &mut module)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_module_attr(mut args: ModuleArgs, module: &mut ItemMod) -> syn::Result<TokenStream2> {
    let Some((_, items)) = module.content.as_mut() else {
        return Err(syn::Error::new(
      module.span(),
      "#[module] must be on an inline module (`mod name { … }`): an attribute macro does not see \
       the items of `mod name;` — put `declare_module! { …, exports = […] }` inside that file instead",
    ));
    };
    if args.exports.is_none() {
        let mut exports = Vec::new();
        for item in items.iter() {
            if let Item::Fn(f) = item {
                if f.attrs.iter().any(|a| {
                    a.path()
                        .segments
                        .last()
                        .map(|s| s.ident == "export")
                        .unwrap_or(false)
                }) {
                    exports.push(f.sig.ident.clone());
                }
            }
        }
        args.exports = Some(exports);
    }
    if args.name.is_none() {
        args.name = Some(module.ident.to_string());
    }
    // The attribute the functions inside carry is this crate's; bring it into
    // the module so a declaration needs no import of its own.
    let import: syn::Item = syn::parse_quote! {
      #[allow(unused_imports)]
      use ::arora_module::export;
    };
    items.insert(0, import);
    let aggregate: syn::File = syn::parse2(expand_aggregate(&args, None)?)?;
    items.extend(aggregate.items);
    Ok(quote! { #module })
}

fn expand_aggregate(args: &ModuleArgs, _scope: Option<&Path>) -> syn::Result<TokenStream2> {
    let module_id = uuid_expr(&args.id.0, args.id.1)?;
    let module_name = args
        .name
        .clone()
        .ok_or_else(|| syn::Error::new(args.id.1, "`declare_module!` requires `name = \"…\"`"))?;
    let exports = args.exports.clone().ok_or_else(|| {
        syn::Error::new(args.id.1, "`declare_module!` requires `exports = [fn, …]`")
    })?;
    let (major, minor, patch) = args.version;
    let author = &args.author;
    let license = &args.license;
    let description = match &args.description {
        Some(d) => quote! { ::std::option::Option::Some(#d.to_string()) },
        None => quote! { ::std::option::Option::None },
    };
    let executable_mime = &args.executable_mime;

    let export_mods: Vec<Ident> = exports.iter().map(export_module_ident).collect();
    let export_names: Vec<String> = exports.iter().map(|e| e.to_string()).collect();
    let client_macros: Vec<Ident> = exports
        .iter()
        .map(|e| format_ident!("__arora_client_{}", e))
        .collect();
    let id_reexports = exports.iter().zip(&export_mods).map(|(fn_ident, m)| {
        quote! { pub use super::#m::ids as #fn_ident; }
    });

    // Wrapped in a const block: a `module!` invocation must expand to items,
    // and a `mod` item cannot be spliced into the middle of a file twice.
    Ok(quote! {
      use ::arora_module::__rt::types as arora_types;

      /// The module's ids: its own, and one submodule per exported function
      /// holding `FUNCTION` and the parameter ids.
      pub mod ids {
        use ::arora_module::__rt::types as arora_types;

        pub const MODULE: arora_types::Uuid = #module_id;
        #(#id_reexports)*
      }

      /// The module's header — what a `module.yaml` declares, in the resolved
      /// (`low`) form the runtime loads. The executor is the exporter's to
      /// name: a declaration does not know whether it will be built native or
      /// wasm, so `header` takes it from the step that builds the artifact
      /// (in the SDK: `Header::executor` becomes `Option`, `None` here, and a
      /// load refuses `None`).
      pub fn header(executor: arora_types::module::low::Executor) -> arora_types::module::low::Header {
        arora_types::module::low::Header {
          id: ids::MODULE,
          name: #module_name.to_string(),
          author: #author.to_string(),
          description: #description,
          license: #license.to_string(),
          version: arora_types::SemanticVersion { major: #major, minor: #minor, patch: #patch },
          executor,
          exports: vec![ #(#export_mods::export()),* ],
          imports: vec![],
          executable_mime: #executable_mime.to_string(),
        }
      }

      /// Every exported function, callable — what a host folds into a module
      /// it registers (`HostModule::of::<Module>()`). The module crate never
      /// depends on the engine.
      pub fn exports() -> ::std::vec::Vec<arora_types::module::declared::AroraFunction> {
        vec![ #(#export_mods::arora_function()),* ]
      }

      /// The module as a **record** — the frozen form the store (semio-db)
      /// serves and accepts: exports keyed by id with their frozen signatures,
      /// and the versioned type references they depend on. `parent` is the
      /// folder the record lives under; `executable` is attached at publication.
      pub fn record(parent: arora_types::Uuid) -> arora_types::record::module::frozen::Module {
        let mut exports = ::std::collections::HashMap::new();
        let mut dependencies: ::std::vec::Vec<arora_types::record::FrozenReference> = ::std::vec::Vec::new();
        #(
          exports.insert(#export_mods::ids::FUNCTION, arora_types::record::module::frozen::Export {
            name: #export_names.to_string(),
            kind: arora_types::record::module::frozen::ExportKind::Function(#export_mods::signature()),
          });
          for dependency in #export_mods::dependencies() {
            if !dependencies.contains(&dependency) {
              dependencies.push(dependency);
            }
          }
        )*
        arora_types::record::module::frozen::Module {
          parent,
          name: #module_name.to_string(),
          exports,
          executable: ::std::option::Option::None,
          dependencies,
        }
      }

      /// The client stubs, one per export, bound to this module's id: the
      /// interface a consumer programs against, dispatching through any
      /// `CallBridge`.
      pub mod client {
        use super::*;
        use ::arora_module::__rt::types as arora_types;

        #(#client_macros!(super::ids::MODULE);)*
      }

      /// The module as a type: what generic code over declared modules names.
      pub struct Module;

      impl arora_types::module::declared::AroraModule for Module {
        fn id() -> arora_types::Uuid { ids::MODULE }
        fn header(executor: arora_types::module::low::Executor) -> arora_types::module::low::Header { header(executor) }
        fn record(parent: arora_types::Uuid) -> arora_types::record::module::frozen::Module { record(parent) }
        fn exports() -> ::std::vec::Vec<arora_types::module::declared::AroraFunction> { exports() }
      }
    })
}

// =============================================================================
// module_from_header!("path/module.yaml", types = ["<uuid>" => Type, …])
// =============================================================================

struct FromHeaderArgs {
    path: syn::LitStr,
    types: Vec<(String, Type)>,
}

impl Parse for FromHeaderArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: syn::LitStr = input.parse()?;
        let mut types = Vec::new();
        if input.parse::<Token![,]>().is_ok() && !input.is_empty() {
            let key: Ident = input.parse()?;
            if key != "types" {
                return Err(syn::Error::new(
                    key.span(),
                    "expected `types = [\"<uuid>\" => Type, …]`",
                ));
            }
            input.parse::<Token![=]>()?;
            let content;
            syn::bracketed!(content in input);
            while !content.is_empty() {
                let id: syn::LitStr = content.parse()?;
                content.parse::<Token![=>]>()?;
                let ty: Type = content.parse()?;
                types.push((id.value(), ty));
                if !content.is_empty() {
                    content.parse::<Token![,]>()?;
                }
            }
        }
        Ok(FromHeaderArgs { path, types })
    }
}

/// The consumer side for a module that is not a Rust declaration: from its
/// resolved (`low`) header, `ids`, `header()` and one `call` stub per export.
/// User types are named by the `types` map (`"<uuid>" => Type`); a user type
/// left unmapped is passed as a raw `Value`.
#[proc_macro]
pub fn module_from_header(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as FromHeaderArgs);
    expand_from_header(&args)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn kind_of_type_ref(
    type_ref: &arora_types::module::low::TypeRef,
    types: &[(String, Type)],
    span: Span,
) -> syn::Result<Kind> {
    use arora_types::module::low::TypeRef;
    let of_id = |id: &arora_types::Uuid| -> syn::Result<Kind> {
        if *id == *arora_types::ty::UNIT_ID {
            return Ok(Kind::Unit);
        }
        if *id == *arora_types::ty::KEY_VALUE_ID {
            return Ok(Kind::Dynamic);
        }
        let table: [(&arora_types::Uuid, &str); 12] = [
            (&*arora_types::ty::BOOLEAN_ID, "bool"),
            (&*arora_types::ty::U8_ID, "u8"),
            (&*arora_types::ty::U16_ID, "u16"),
            (&*arora_types::ty::U32_ID, "u32"),
            (&*arora_types::ty::U64_ID, "u64"),
            (&*arora_types::ty::I8_ID, "i8"),
            (&*arora_types::ty::I16_ID, "i16"),
            (&*arora_types::ty::I32_ID, "i32"),
            (&*arora_types::ty::I64_ID, "i64"),
            (&*arora_types::ty::F32_ID, "f32"),
            (&*arora_types::ty::F64_ID, "f64"),
            (&*arora_types::ty::STRING_ID, "String"),
        ];
        if let Some((_, name)) = table.iter().find(|(known, _)| **known == *id) {
            return Ok(primitive(name).expect("a primitive"));
        }
        match types.iter().find(|(known, _)| known == &id.to_string()) {
            Some((_, ty)) => Ok(Kind::User(ty.clone())),
            None => Ok(Kind::Dynamic),
        }
    };
    match type_ref {
        TypeRef::Scalar { id } => of_id(id),
        TypeRef::Array { id } => match of_id(id)? {
            Kind::Dynamic => Ok(Kind::Array(Box::new(Kind::Dynamic))),
            element => Ok(Kind::Array(Box::new(element))),
        },
        TypeRef::FixedArray { .. } | TypeRef::Map { .. } | TypeRef::Option { .. } => {
            Err(syn::Error::new(
                span,
                "fixed arrays, maps and options are not supported by the header macro yet",
            ))
        }
    }
}

fn expand_from_header(args: &FromHeaderArgs) -> syn::Result<TokenStream2> {
    use arora_types::module::low::ExportSymbol;
    let span = args.path.span();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| syn::Error::new(span, "CARGO_MANIFEST_DIR"))?;
    let path = std::path::Path::new(&manifest_dir).join(args.path.value());
    let yaml = std::fs::read_to_string(&path)
        .map_err(|e| syn::Error::new(span, format!("cannot read {}: {e}", path.display())))?;
    let header: arora_types::module::low::Header = serde_yaml::from_str(&yaml)
        .map_err(|e| syn::Error::new(span, format!("not a module header: {e}")))?;
    let path_str = path.display().to_string();

    let module_id = uuid_expr(&header.id.to_string(), span)?;
    let module_name = &header.name;
    let mut id_mods = Vec::new();
    let mut stubs = Vec::new();
    let fail = quote! { (|message: ::std::string::String| arora_types::call::CallError::Guest { message }) };
    for export in &header.exports {
        let ExportSymbol::Function(f) = export;
        let fn_ident = format_ident!("{}", f.name);
        let fn_id = uuid_expr(&f.id.to_string(), span)?;
        let fn_name = &f.name;
        let mut param_consts = Vec::new();
        let mut stub_params = Vec::new();
        let mut stub_args = Vec::new();
        let mut read_mutated = Vec::new();
        for p in &f.parameters {
            let c = format_ident!("{}", upper_snake(&p.name));
            let ident = format_ident!("{}", p.name);
            let id = uuid_expr(&p.id.to_string(), span)?;
            param_consts.push(quote! { pub const #c: arora_types::Uuid = #id; });
            let kind = kind_of_type_ref(&p.ty, &args.types, span)?;
            let ty = kind.rust_type();
            if p.mutable {
                stub_params.push(quote! { #ident: &mut #ty });
                let encode = kind.encode(quote! { ::std::clone::Clone::clone(&*#ident) });
                stub_args.push(quote! { arora_types::value::StructureField { id: ids::#fn_ident::#c, value: ::std::boxed::Box::new(#encode) } });
                let what = format!("mutated parameter `{}` of `{}`", p.name, fn_name);
                let decode = kind.decode(&what, &fail);
                read_mutated.push(quote! {
          if let Some(field) = __result.mutated.iter().find(|field| field.id == ids::#fn_ident::#c) {
            let __value = (*field.value).clone();
            *#ident = #decode;
          }
        });
            } else {
                stub_params.push(quote! { #ident: #ty });
                let encode = kind.encode(quote! { #ident });
                stub_args.push(quote! { arora_types::value::StructureField { id: ids::#fn_ident::#c, value: ::std::boxed::Box::new(#encode) } });
            }
        }
        let ret_kind = kind_of_type_ref(&f.ret, &args.types, span)?;
        let ret_ty = ret_kind.rust_type();
        let ret_what = format!("return value of `{}`", fn_name);
        let decode_ret = ret_kind.decode(&ret_what, &fail);
        id_mods.push(quote! {
          pub mod #fn_ident {
            use ::arora_module::__rt::types as arora_types;

            pub const FUNCTION: arora_types::Uuid = #fn_id;
            #(#param_consts)*
          }
        });
        stubs.push(quote! {
          /// The stub for this export: the call by id through a `CallBridge`.
          pub fn #fn_ident<B: arora_types::call::CallBridge + ?Sized>(
            bridge: &mut B,
            #(#stub_params),*
          ) -> ::std::result::Result<#ret_ty, arora_types::call::CallError> {
            let __result = bridge.arora_call(arora_types::call::Call {
              module_id: ::std::option::Option::Some(ids::MODULE),
              id: ids::#fn_ident::FUNCTION,
              args: vec![ #(#stub_args),* ],
            })?;
            #(#read_mutated)*
            let __value = __result.ret;
            ::std::result::Result::Ok(#decode_ret)
          }
        });
    }

    Ok(quote! {
      use ::arora_module::__rt::types as arora_types;

      /// The module's ids, read from its header.
      pub mod ids {
        use ::arora_module::__rt::types as arora_types;

        pub const MODULE: arora_types::Uuid = #module_id;
        #(#id_mods)*
      }

      /// The header the stubs were generated from, as written: a consumer
      /// that wants it parsed does so with its own YAML reader, so a module
      /// crate carries none.
      pub const HEADER_YAML: &str = include_str!(#path_str);

      /// The module's name, from its header.
      pub const NAME: &str = #module_name;

      #(#stubs)*
    })
}

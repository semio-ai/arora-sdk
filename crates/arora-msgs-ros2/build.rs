//! Generate the bundled ROS 2 message types from the vendored `.msg` files.
//!
//! For every `msgs/<package>/<Type>.msg` this emits, into `OUT_DIR`, a Rust
//! struct `<package>::<Type>` deriving `AroraType` (so the Rust definition is the
//! schema source of truth) and `serde`, carrying its ROS-qualified name and its
//! `.msg` constants, plus a per-package `register` function and a crate-wide
//! `registry()`. `lib.rs` includes the result, so `arora_msgs_ros2::geometry_msgs::Point`
//! is the generated struct and `arora_msgs_ros2::registry()` is every type at once.
//!
//! Type ids: the structs carry `#[arora(name = "package/msg/Type")]` and no
//! explicit id, so each id is `gen_uuid_from_str` of that ROS-qualified name — a
//! pure function of the name, identical whether the type is generated here or
//! defined from the same `.msg` at runtime, and stable across builds. No id is
//! baked per build.

use std::collections::HashSet;
use std::path::Path;
use std::{env, fs};

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{format_ident, quote};

// The parser is shared with the runtime (`define_from_msg`); it is std-only so
// it compiles here without pulling arora-types into the build script.
#[path = "src/schema.rs"]
mod schema;

use schema::{Arr, Base, Field, MessageSpec, Primitive};

fn main() {
    println!("cargo:rerun-if-changed=msgs");
    println!("cargo:rerun-if-changed=src/schema.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let msgs_dir = Path::new(&manifest).join("msgs");
    let out_dir = env::var("OUT_DIR").unwrap();

    // One package per subdirectory of msgs/, in a stable (sorted) order.
    let mut packages: Vec<String> = fs::read_dir(&msgs_dir)
        .expect("msgs/ directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    packages.sort();

    // Parse every package before emitting: `Default`-ability crosses packages
    // (a message nests types from others), so it is computed over the whole set.
    let parsed: Vec<Vec<MessageSpec>> = packages
        .iter()
        .map(|package| parse_package(&msgs_dir.join(package), package))
        .collect();
    let default_able = compute_default_able(&parsed);

    for (package, specs) in packages.iter().zip(&parsed) {
        let source = emit_package(specs, &default_able);
        fs::write(Path::new(&out_dir).join(format!("{package}.rs")), source).unwrap();
    }

    let generated = emit_root(&packages);
    fs::write(Path::new(&out_dir).join("generated.rs"), generated).unwrap();
}

/// Parse every `.msg` in a package directory, in a stable (sorted) order.
fn parse_package(dir: &Path, package: &str) -> Vec<MessageSpec> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .expect("package directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "msg" || x == "action"))
        .collect();
    files.sort();

    files
        .iter()
        .flat_map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy();
            let text = fs::read_to_string(path).unwrap();
            if path.extension().is_some_and(|x| x == "action") {
                schema::parse_action(package, &name, &text)
                    .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
            } else {
                vec![schema::parse_msg(package, &name, &text)
                    .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))]
            }
        })
        .collect()
}

/// The source for one package: its structs, their constants, and a `register`.
fn emit_package(specs: &[MessageSpec], default_able: &HashSet<String>) -> String {
    let structs = specs.iter().map(|spec| emit_struct(spec, default_able));
    let type_idents: Vec<_> = specs.iter().map(|s| format_ident!("{}", s.name)).collect();
    // The serde-big-array trait, imported only for a package that has a fixed
    // array longer than 32 (so no unused import elsewhere).
    let has_big_array = specs.iter().any(|s| {
        s.fields
            .iter()
            .any(|f| matches!(f.ty.arr, Arr::Fixed(n) if n > 32))
    });
    let big_array_use = if has_big_array {
        quote! { use serde_big_array::BigArray; }
    } else {
        quote! {}
    };
    let tokens = quote! {
        #big_array_use

        #(#structs)*

        /// Register every type in this package, and the types they nest, into
        /// `registry`.
        pub fn register(registry: &mut crate::Ros2Registry) {
            #( registry.register_arora::<#type_idents>(); )*
        }
    };
    tokens.to_string()
}

/// One message struct: `#[derive(AroraType, serde…)] #[arora(name = "pkg/msg/Type")]`
/// with its fields, plus its `.msg` constants as associated `const`s.
fn emit_struct(spec: &MessageSpec, default_able: &HashSet<String>) -> TokenStream {
    let struct_ident = format_ident!("{}", spec.name);
    let rep_name = spec.rep2016_name();
    // Action sub-messages keep their REP-2016 `Name_Goal` form as the Rust
    // ident too, so the code reads like the wire.
    let case_allowance = if spec.name.contains('_') {
        quote! { #[allow(non_camel_case_types)] }
    } else {
        quote! {}
    };

    let fields = spec.fields.iter().map(|field| {
        let fname = field_ident(&field.name);
        let fty = rust_type(&field.ty.base, field.ty.arr);
        // serde implements its traits for arrays only up to length 32; a larger
        // fixed array (float64[36] covariance) goes through serde-big-array.
        let serde_attr = if matches!(field.ty.arr, Arr::Fixed(n) if n > 32) {
            quote! { #[serde(with = "BigArray")] }
        } else {
            quote! {}
        };
        quote! { #serde_attr pub #fname: #fty, }
    });

    // Derive `Default` unless the type (transitively) holds a fixed array longer
    // than 32 — those are the only shapes that are not `Default`. (Honouring
    // `.msg` field default *values*, e.g. Quaternion w=1, is a separate follow-up.)
    let default_derive = if default_able.contains(&rep_name) {
        quote! { Default, }
    } else {
        quote! {}
    };
    let derives = quote! {
        #[derive(Debug, Clone, #default_derive PartialEq, serde::Serialize, serde::Deserialize, arora_types::AroraType)]
    };

    let consts = spec.constants.iter().map(|c| {
        let cname = format_ident!("{}", c.name);
        if c.ty == Primitive::String {
            let value = c.value.as_str();
            quote! { pub const #cname: &str = #value; }
        } else {
            let cty = format_ident!("{}", c.ty.rust_name());
            let value: TokenStream = c.value.parse().unwrap_or_else(|_| {
                panic!("constant {} has a non-literal value {:?}", c.name, c.value)
            });
            quote! { pub const #cname: #cty = #value; }
        }
    });
    let const_impl = if spec.constants.is_empty() {
        quote! {}
    } else {
        quote! { impl #struct_ident { #(#consts)* } }
    };

    // Compile-time ROS identity (RosMessage) so consumers build a MessageTypeName
    // and match a topic's type without stringly-typed lookups.
    let package = spec.package.as_str();
    let type_name = spec.name.as_str();

    quote! {
        #derives
        #[arora(name = #rep_name)]
        #case_allowance
        pub struct #struct_ident {
            #(#fields)*
        }
        #const_impl
        impl crate::RosMessage for #struct_ident {
            const ROS_TYPE_NAME: &'static str = #rep_name;
            const PACKAGE: &'static str = #package;
            const TYPE_NAME: &'static str = #type_name;
        }
    }
}

/// The set of REP-2016 names whose message types can derive `Default`. A message
/// cannot iff it has a fixed array longer than 32 (`[T; N>32]` is not `Default`),
/// or a single/fixed-array field of a non-`Default` type. `Vec<_>` fields never
/// block it (they default to empty). Computed as a fixpoint because
/// Default-ability propagates through nested-message fields.
fn compute_default_able(parsed: &[Vec<MessageSpec>]) -> HashSet<String> {
    let mut ok: HashSet<String> = parsed
        .iter()
        .flat_map(|specs| specs.iter().map(|s| s.rep2016_name()))
        .collect();
    loop {
        let mut changed = false;
        for specs in parsed {
            for spec in specs {
                let name = spec.rep2016_name();
                if ok.contains(&name) && !spec.fields.iter().all(|f| field_default_able(f, &ok)) {
                    ok.remove(&name);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    ok
}

/// Whether one field leaves its message `Default`-able.
fn field_default_able(field: &Field, ok: &HashSet<String>) -> bool {
    let element_default_able = match &field.ty.base {
        Base::Primitive(_) => true,
        Base::Named { package, name } => ok.contains(&format!("{package}/msg/{name}")),
    };
    match field.ty.arr {
        Arr::Unbounded => true, // Vec<_> defaults to empty regardless of element
        Arr::Unit => element_default_able,
        Arr::Fixed(n) => n <= 32 && element_default_able,
    }
}

/// A field identifier, made a raw identifier (`r#type`) when the ROS field name
/// is a Rust keyword (`type` in `JoyFeedback`, `final` in `LiveSpeech`, …). The
/// `AroraType` derive strips the `r#` back off, so the arora field name and its
/// id stay the plain ROS name.
fn field_ident(name: &str) -> Ident {
    // The Rust 2021 keywords (strict + reserved). ROS field names never hit the
    // handful that cannot be raw identifiers (`self`, `super`, `crate`, `Self`).
    const KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false",
        "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
        "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
        "unsafe", "use", "where", "while", "async", "await", "abstract", "become", "box", "do",
        "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
    ];
    if KEYWORDS.contains(&name) {
        Ident::new_raw(name, Span::call_site())
    } else {
        Ident::new(name, Span::call_site())
    }
}

/// The Rust type for a field: the element, wrapped by its array shape.
fn rust_type(base: &Base, arr: Arr) -> TokenStream {
    let element = match base {
        // Fully qualified so it is never shadowed by the generated
        // `std_msgs::String` message struct inside that module.
        Base::Primitive(Primitive::String) => quote! { ::std::string::String },
        Base::Primitive(p) => {
            let ident = format_ident!("{}", p.rust_name());
            quote! { #ident }
        }
        Base::Named { package, name } => {
            let pkg = format_ident!("{}", package);
            let ty = format_ident!("{}", name);
            quote! { crate::#pkg::#ty }
        }
    };
    match arr {
        Arr::Unit => element,
        Arr::Unbounded => quote! { Vec<#element> },
        Arr::Fixed(n) => {
            let n = Literal::usize_unsuffixed(n);
            quote! { [#element; #n] }
        }
    }
}

/// The crate-level `generated.rs`: one module per package, plus `registry()`.
fn emit_root(packages: &[String]) -> String {
    let modules = packages.iter().map(|package| {
        let ident = format_ident!("{}", package);
        let file = format!("{package}.rs");
        quote! {
            // Generated code: ROS field names are verbatim (some are acronyms
            // like `ZCR`), and most types are unused by any given consumer.
            #[allow(non_snake_case, dead_code, clippy::all)]
            pub mod #ident {
                include!(concat!(env!("OUT_DIR"), "/", #file));
            }
        }
    });
    let registers = packages.iter().map(|package| {
        let ident = format_ident!("{}", package);
        quote! { #ident::register(&mut registry); }
    });
    let tokens = quote! {
        #(#modules)*

        /// A [`Ros2Registry`](crate::Ros2Registry) holding every bundled ROS 2
        /// message type — the std/geometry/sensor/hri packages and their
        /// dependencies — indexed by id and by name.
        pub fn registry() -> crate::Ros2Registry {
            let mut registry = crate::Ros2Registry::new();
            #(#registers)*
            registry
        }
    };
    tokens.to_string()
}

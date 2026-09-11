//! The Rust declaration reproduces the header the SDK generator emitted from
//! `modules/polly/module.yaml`.

use arora_types::module::low::{ExportSymbol, Header};
use case_polly_outline::polly;
use std::path::PathBuf;

fn generated_header() -> Header {
  let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../../../../../modules/polly/src/arora_generated/module.yaml");
  let yaml = std::fs::read_to_string(&path).expect("read the generated polly header");
  serde_yaml::from_str(&yaml).expect("parse the generated polly header")
}

/// The exports of a header as YAML values sorted by function id — the
/// generator emits them in `HashMap` order.
fn exports_by_id(header: &Header) -> Vec<(uuid::Uuid, serde_yaml::Value)> {
  let mut exports: Vec<_> = header
    .exports
    .iter()
    .map(|e| {
      let ExportSymbol::Function(f) = e;
      (f.id, serde_yaml::to_value(e).expect("serialize an export"))
    })
    .collect();
  exports.sort_by_key(|(id, _)| *id);
  exports
}

#[test]
fn the_declared_ids_are_the_yaml_ids() {
  let generated = generated_header();
  assert_eq!(polly::ids::MODULE, generated.id);
  let ids: Vec<_> = generated
    .exports
    .iter()
    .map(|e| {
      let ExportSymbol::Function(f) = e;
      (
        f.name.as_str(),
        f.id,
        f.parameters.iter().map(|p| p.id).collect::<Vec<_>>(),
      )
    })
    .collect();
  assert!(ids.contains(&(
    "say",
    polly::ids::say::FUNCTION,
    vec![polly::ids::say::TEXT]
  )));
  assert!(ids.contains(&("hello_world", polly::ids::hello_world::FUNCTION, vec![])));
}

#[test]
fn the_declared_header_reproduces_the_generated_exports() {
  let declared = polly::header(arora_types::module::low::Executor {
    name: "native".into(),
    min_version: None,
    max_version: None,
  });
  let generated = generated_header();
  assert_eq!(declared.id, generated.id);
  assert_eq!(declared.name, generated.name);
  // The executor is the exporter's: here, the one the generator's header names.
  assert_eq!(declared.executor.name, generated.executor.name);
  assert_eq!(exports_by_id(&declared), exports_by_id(&generated));
}

#[test]
fn the_declared_header_keeps_what_the_generator_strips() {
  // The generator emits a header with author/license/description/version
  // stripped; the Rust declaration carries them, as the source `module.yaml` does.
  let declared = polly::header(arora_types::module::low::Executor {
    name: "native".into(),
    min_version: None,
    max_version: None,
  });
  assert_eq!(declared.author, "Semio");
  assert_eq!(declared.license, "Proprietary");
  assert_eq!(
    declared.description.as_deref(),
    Some("AWS Polly support module")
  );
  assert_eq!(
    (
      declared.version.major,
      declared.version.minor,
      declared.version.patch
    ),
    (0, 1, 0)
  );
  assert_eq!(declared.executable_mime, "application/x-binary");
}

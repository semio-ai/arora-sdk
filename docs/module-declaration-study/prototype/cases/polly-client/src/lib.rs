//! polly's interface read from the generator's header — what a caller that
//! does not depend on polly's declaration programs against. The one user type
//! in that header (`behavior_tree.Status`) is mapped to a Rust type by id.

arora_module_derive::module_from_header!(
  "../../../../../modules/polly/src/arora_generated/module.yaml",
  types = ["325a5767-e344-4532-860e-0749bcf2e428" => case_polly::Status]
);

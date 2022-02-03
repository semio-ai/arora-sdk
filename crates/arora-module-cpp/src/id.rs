pub fn function_uuid(name: &str) -> String {
  format!("{}_UUID", name.replace('-', "_")).to_uppercase()
}

pub fn module_uuid(name: &str) -> String {
  format!("{}_MODULE_UUID", name.replace('-', "_")).to_uppercase()
}

pub fn type_uuid(name: &str) -> String {
  format!("{}_TYPE_UUID", name.replace('-', "_")).to_uppercase()
}

pub fn parameter_uuid(export: &str, name: &str) -> String {
  format!(
    "{}_PARAMETER_{}_UUID",
    export.replace('-', "_"),
    name.replace('-', "_")
  )
  .to_uppercase()
}

pub fn value_uuid(export: &str, name: &str) -> String {
  format!(
    "{}_VALUE_{}_UUID",
    export.replace('-', "_"),
    name.replace('-', "_")
  )
  .to_uppercase()
}

pub fn field_uuid(export: &str, name: &str) -> String {
  format!(
    "{}_FIELD_{}_UUID",
    export.replace('-', "_"),
    name.replace('-', "_")
  )
  .to_uppercase()
}

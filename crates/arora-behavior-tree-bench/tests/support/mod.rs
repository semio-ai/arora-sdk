use arora_types::value::Value;

pub fn store_hash(mut entries: Vec<(&str, Option<Value>)>) -> String {
    entries.sort_by_key(|(name, _)| *name);
    arora_behavior_tree_bench::measure::values_hash(
        &entries
            .into_iter()
            .flat_map(|(name, value)| [Some(Value::String(name.into())), value])
            .collect::<Vec<_>>(),
    )
}

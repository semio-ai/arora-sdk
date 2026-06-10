#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use arora_registry::{
        local::LocalRegistry, local_yaml::load_records_from_yaml_dir, ReadableRegistry,
    };
    use uuid::Uuid;

    /// Check that the records describing the behavior tree types can be loaded correctly.
    #[tokio::test]
    async fn test_load_records_from_yaml_dir() {
        let mut registry = LocalRegistry::new();
        let path = PathBuf::from("records");
        load_records_from_yaml_dir(path, &mut registry)
            .await
            .unwrap();
        assert_eq!(
            registry
                .resolve_id(&Uuid::from_str("325a5767-e344-4532-860e-0749bcf2e428").unwrap())
                .await
                .unwrap(),
            "behavior_tree.Status"
        );
    }
}

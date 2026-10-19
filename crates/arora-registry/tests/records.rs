//! A registry holding the records a module and its types are described by:
//! added from a Rust declaration, each in its scope, looked up exactly,
//! checked for what they depend on, written in the record-directory layout
//! and read back.

use arora_registry::local::{LocalRegistry, ROOT_ID};
use arora_registry::local_yaml::{
    load_records, load_records_from_yaml_dir, records_as_yaml, write_records_to_yaml_dir,
    RecordKind,
};
use arora_registry::{
    EditableRegistry, EnumerationFrozen, FolderPublic, ReadableRegistry, RegistryError,
    StructureFrozen, TypeDefinitionFrozen,
};
use arora_types::record::scope::Scope;
use arora_types::record::structure::frozen::StructureField;
use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
use arora_types::record::FrozenReference;
use arora_types::{AroraType, Uuid};
use semver::{Version, VersionReq};

/// The scope `sampler` and `upsampler` state.
pub const SAMPLES: Scope = Scope {
    id: Uuid::from_u128(0x5a3e_f01d),
    handle: "samples",
};

/// The scope `resampler` states.
pub const STUDIO: Scope = Scope {
    id: Uuid::from_u128(0x5707_f01d),
    handle: "studio",
};

/// What `sample` returns: a structure naming an enumeration, both stating no
/// scope.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "5a3e0000-0000-4000-8000-000000000001", version = "1.1.0")]
pub struct Sample {
    #[arora(id = "5a3e0000-0000-4000-8000-000000000011")]
    value: f32,
    #[arora(id = "5a3e0000-0000-4000-8000-000000000012")]
    mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, AroraType)]
#[arora(id = "5a3e0000-0000-4000-8000-000000000002")]
pub enum Mode {
    #[arora(id = "5a3e0000-0000-4000-8000-000000000021")]
    Idle,
    #[arora(id = "5a3e0000-0000-4000-8000-000000000022")]
    Busy,
}

/// A type a record cannot express: it holds a fixed-length array.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "5a3e0000-0000-4000-8000-000000000003")]
pub struct Window {
    #[arora(id = "5a3e0000-0000-4000-8000-000000000031")]
    bounds: [f64; 2],
}

#[arora_module::module(
    id = "5a3e0000-0000-4000-8000-0000000000a0",
    name = "sampler",
    version = "0.3.0",
    author = "Semio",
    license = "Apache-2.0",
    description = "A declared module whose functions name records",
    executable_mime = "application/wasm",
    scope = crate::SAMPLES
)]
pub mod sampler {
    use super::{Mode, Sample};

    #[export(id = "5a3e0000-0000-4000-8000-0000000000a1")]
    pub fn sample(#[param(id = "5a3e0000-0000-4000-8000-0000000000a2")] value: f32) -> Sample {
        Sample {
            value,
            mode: Mode::Idle,
        }
    }

    #[export(id = "5a3e0000-0000-4000-8000-0000000000a3")]
    pub fn samples() -> Vec<Sample> {
        Vec::new()
    }
}

#[arora_module::module(
    id = "5a3e0000-0000-4000-8000-0000000000b0",
    name = "windowed",
    version = "0.1.0",
    author = "Semio",
    license = "Apache-2.0",
    description = "A declared module naming a type without a record",
    executable_mime = "application/wasm"
)]
pub mod windowed {
    use super::Window;

    #[export(id = "5a3e0000-0000-4000-8000-0000000000b1")]
    pub fn window() -> Window {
        Window { bounds: [0.0, 1.0] }
    }
}

/// A module of another scope naming `Sample`, which states no scope.
#[arora_module::module(
    id = "5a3e0000-0000-4000-8000-0000000000c0",
    name = "resampler",
    version = "0.1.0",
    author = "Semio",
    license = "Apache-2.0",
    description = "A declared module sharing a type that states no scope",
    executable_mime = "application/wasm",
    scope = crate::STUDIO
)]
pub mod resampler {
    use super::Sample;

    #[export(id = "5a3e0000-0000-4000-8000-0000000000c1")]
    pub fn resample(
        #[param(id = "5a3e0000-0000-4000-8000-0000000000c2")] sample: Sample,
    ) -> Sample {
        sample
    }
}

/// A module of `sampler`'s scope naming `Sample`.
#[arora_module::module(
    id = "5a3e0000-0000-4000-8000-0000000000c8",
    name = "upsampler",
    version = "0.1.0",
    author = "Semio",
    license = "Apache-2.0",
    description = "A declared module of the scope its shared type lives in",
    executable_mime = "application/wasm",
    scope = crate::SAMPLES
)]
pub mod upsampler {
    use super::Sample;

    #[export(id = "5a3e0000-0000-4000-8000-0000000000c9")]
    pub fn upsample(
        #[param(id = "5a3e0000-0000-4000-8000-0000000000ca")] sample: Sample,
    ) -> Vec<Sample> {
        vec![sample]
    }
}

/// The scope `Reading` states.
pub const SENSORS: Scope = Scope {
    id: Uuid::from_u128(0x5e45_f01d),
    handle: "sensors",
};

/// The scopes of the two modules sharing `Reading`.
pub const WEATHER: Scope = Scope {
    id: Uuid::from_u128(0xd0_f01d),
    handle: "weather",
};
pub const LOGS: Scope = Scope {
    id: Uuid::from_u128(0xe0_f01d),
    handle: "logs",
};

/// A type two modules of different scopes share, stating its scope.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "5a3e0000-0000-4000-8000-000000000004", scope = SENSORS)]
pub struct Reading {
    #[arora(id = "5a3e0000-0000-4000-8000-000000000041")]
    value: f32,
    #[arora(id = "5a3e0000-0000-4000-8000-000000000042")]
    scale: Scale,
}

/// Named only through `Reading`, so it lives where `Reading` does.
#[derive(Debug, Clone, Copy, PartialEq, AroraType)]
#[arora(id = "5a3e0000-0000-4000-8000-000000000005")]
pub enum Scale {
    #[arora(id = "5a3e0000-0000-4000-8000-000000000051")]
    Celsius,
}

#[arora_module::module(
    id = "5a3e0000-0000-4000-8000-0000000000d0",
    name = "thermometer",
    version = "1.0.0",
    author = "Semio",
    license = "Apache-2.0",
    description = "A declared module naming a shared type",
    executable_mime = "application/wasm",
    scope = crate::WEATHER
)]
pub mod thermometer {
    use super::{Reading, Scale};

    #[export(id = "5a3e0000-0000-4000-8000-0000000000d1")]
    pub fn read() -> Reading {
        Reading {
            value: 20.0,
            scale: Scale::Celsius,
        }
    }
}

#[arora_module::module(
    id = "5a3e0000-0000-4000-8000-0000000000e0",
    name = "logger",
    version = "1.0.0",
    author = "Semio",
    license = "Apache-2.0",
    description = "Another declared module naming the shared type",
    executable_mime = "application/wasm",
    scope = crate::LOGS
)]
pub mod logger {
    use super::Reading;

    #[export(id = "5a3e0000-0000-4000-8000-0000000000e1")]
    pub fn history() -> Vec<Reading> {
        Vec::new()
    }
}

/// A type stating no scope, named only by a module declaring none.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "5a3e0000-0000-4000-8000-000000000006")]
pub struct Level {
    #[arora(id = "5a3e0000-0000-4000-8000-000000000061")]
    value: f32,
}

/// A module declared without a scope, as an operator installs one by hand.
#[arora_module::module(
    id = "5a3e0000-0000-4000-8000-0000000000f0",
    name = "leveler",
    version = "0.1.0",
    author = "Semio",
    license = "Apache-2.0",
    description = "A declared module without a scope",
    executable_mime = "application/wasm"
)]
pub mod leveler {
    use super::Level;

    #[export(id = "5a3e0000-0000-4000-8000-0000000000f1")]
    pub fn level() -> Level {
        Level { value: 1.0 }
    }
}

fn version(text: &str) -> Version {
    Version::parse(text).expect("a version")
}

fn reference<T: AroraType>() -> FrozenReference {
    FrozenReference {
        id: T::arora_type_id(),
        version: T::arora_type_version(),
    }
}

/// A registry holding the `samples` scope's folder and nothing else.
async fn with_scope() -> LocalRegistry {
    let mut registry = LocalRegistry::new();
    registry
        .add_folder(SAMPLES.id, SAMPLES.record())
        .await
        .expect("the scope's folder is added");
    registry
}

/// The records a declared module carries — its module record, and those of
/// the types its functions name — land in the scope its declaration states,
/// the types stating no scope there too, with the scope's folder; the
/// registry then holds everything the module record depends on, and finds
/// it under its exact id and version.
#[tokio::test]
async fn a_declared_module_s_records_land_in_its_scope() {
    let mut registry = LocalRegistry::new();
    registry
        .add_declared_module::<sampler::Module>(version("0.3.0"))
        .await
        .expect("the declaration's records are added");

    let module = registry
        .module_at(&sampler::ids::MODULE, &version("0.3.0"))
        .expect("the module record is held at the header's version");
    assert_eq!(*module, sampler::record(SAMPLES.id));
    assert!(registry
        .module_at(&sampler::ids::MODULE, &version("0.3.1"))
        .is_none());
    assert!(
        registry
            .missing_dependencies(&FrozenReference {
                id: sampler::ids::MODULE,
                version: version("0.3.0").into(),
            })
            .is_empty(),
        "every record the module record depends on is held"
    );
    let Some(TypeDefinitionFrozen::Structure(sample)) =
        registry.type_at(&Sample::arora_type_id(), &version("1.1.0"))
    else {
        panic!("Sample's record is held at its version");
    };
    assert_eq!(sample.parent, SAMPLES.id);
    let Some(TypeDefinitionFrozen::Enumeration(mode)) =
        registry.type_at(&Mode::arora_type_id(), &version("1.0.0"))
    else {
        panic!("Mode's record is held at its version");
    };
    assert_eq!(mode.parent, SAMPLES.id);
    assert_eq!(
        registry.resolve_id(&SAMPLES.id).await.unwrap(),
        "samples",
        "the scope's folder is named by its handle"
    );
    assert_eq!(
        registry.resolve_id(&sampler::ids::MODULE).await.unwrap(),
        "samples.sampler"
    );
    assert_eq!(
        registry.resolve_id(&Sample::arora_type_id()).await.unwrap(),
        "samples.Sample"
    );

    // The same records again are the ones it holds.
    registry
        .add_declared_module::<sampler::Module>(version("0.3.0"))
        .await
        .expect("adding the same records again keeps them");
}

/// Two modules of different scopes sharing a type that states its scope add
/// together: the shared type, and the type it names that states none, are
/// one record each in the stated scope, added with its folder.
#[tokio::test]
async fn two_modules_of_different_scopes_share_a_type_that_states_its_scope() {
    let mut registry = LocalRegistry::new();
    registry
        .add_declared_module::<thermometer::Module>(version("1.0.0"))
        .await
        .expect("the first module's records are added");
    registry
        .add_declared_module::<logger::Module>(version("1.0.0"))
        .await
        .expect("the second module's records add beside the first's");

    let Some(TypeDefinitionFrozen::Structure(reading)) =
        registry.type_at(&Reading::arora_type_id(), &version("1.0.0"))
    else {
        panic!("Reading's record is held");
    };
    assert_eq!(reading.parent, SENSORS.id);
    let Some(TypeDefinitionFrozen::Enumeration(scale)) =
        registry.type_at(&Scale::arora_type_id(), &version("1.0.0"))
    else {
        panic!("Scale's record is held");
    };
    assert_eq!(scale.parent, SENSORS.id);
    assert_eq!(
        registry
            .resolve_id(&Reading::arora_type_id())
            .await
            .unwrap(),
        "sensors.Reading"
    );
    for (module, path) in [
        (thermometer::ids::MODULE, "weather.thermometer"),
        (logger::ids::MODULE, "logs.logger"),
    ] {
        assert_eq!(registry.resolve_id(&module).await.unwrap(), path);
        assert!(registry
            .missing_dependencies(&FrozenReference {
                id: module,
                version: version("1.0.0").into(),
            })
            .is_empty());
    }
}

/// A type stating no scope lives in the scope of each module naming it, so
/// two modules of different scopes sharing it carry two different records
/// under one id and version: the second is refused, naming the type and the
/// module. Within one scope, they add together.
#[tokio::test]
async fn a_type_stating_no_scope_is_shared_only_within_one_scope() {
    let mut registry = LocalRegistry::new();
    registry
        .add_declared_module::<sampler::Module>(version("0.3.0"))
        .await
        .expect("the first module's records are added");
    let error = registry
        .add_declared_module::<resampler::Module>(version("0.1.0"))
        .await
        .expect_err("Sample is held in another scope");
    let message = error.to_string();
    assert!(
        message.contains(&format!("type 'Sample' ({})", reference::<Sample>())),
        "{message}"
    );
    assert!(message.contains("module 'resampler'"), "{message}");
    assert!(message.contains("states its own scope"), "{message}");
    assert!(registry
        .module_at(&resampler::ids::MODULE, &version("0.1.0"))
        .is_none());

    registry
        .add_declared_module::<upsampler::Module>(version("0.1.0"))
        .await
        .expect("from the same scope, the records are the same");
}

/// A name is unique within its scope: another record under a name a record
/// holds in the same scope is refused, at its version or another, and a
/// declared module naming a type whose name is taken fails, naming the path.
/// The same name in another scope, or at the root, is another path.
#[tokio::test]
async fn a_name_is_unique_within_its_scope() {
    let mut registry = with_scope().await;
    let impostor = |parent: Uuid| StructureFrozen {
        parent,
        name: "Mode".to_string(),
        fields: Default::default(),
    };
    let impostor_id = Uuid::from_u128(0x0bf1);
    registry
        .add_structure(impostor_id, version("1.0.0"), impostor(SAMPLES.id))
        .await
        .expect("the first record named Mode in the scope is added");
    for tag in ["1.0.0", "2.0.0"] {
        let error = registry
            .add_structure(Uuid::from_u128(0x0bf2), version(tag), impostor(SAMPLES.id))
            .await
            .expect_err("a name is unique within its scope");
        assert!(
            matches!(&error, RegistryError::DuplicateSelector { selector }
                if selector.to_string().contains("samples.Mode")),
            "{error}"
        );
    }

    let error = registry
        .add_declared_module::<sampler::Module>(version("0.3.0"))
        .await
        .expect_err("Mode's name is taken in the samples scope");
    let message = error.to_string();
    assert!(message.contains("type 'Mode'"), "{message}");
    assert!(message.contains("'samples.Mode'"), "{message}");
    assert!(
        message.contains("a name is unique within its scope"),
        "{message}"
    );
    assert!(registry
        .module_at(&sampler::ids::MODULE, &version("0.3.0"))
        .is_none());

    registry
        .add_folder(STUDIO.id, STUDIO.record())
        .await
        .expect("another scope is added");
    registry
        .add_structure(
            Uuid::from_u128(0x0bf3),
            version("1.0.0"),
            impostor(STUDIO.id),
        )
        .await
        .expect("the same name in another scope is another path");
    registry
        .add_structure(Uuid::from_u128(0x0bf4), version("1.0.0"), impostor(ROOT_ID))
        .await
        .expect("the same name at the root is another path");
}

/// A module declared without a scope, and the types it names that state
/// none, live at the root: a local registry holds them, writes them and
/// reads them back.
#[tokio::test]
async fn unscoped_records_live_at_the_root() {
    let mut registry = LocalRegistry::new();
    registry
        .add_declared_module::<leveler::Module>(version("0.1.0"))
        .await
        .expect("an unscoped module's records are added");
    assert_eq!(
        registry.resolve_id(&leveler::ids::MODULE).await.unwrap(),
        "leveler"
    );
    let Some(TypeDefinitionFrozen::Structure(level)) =
        registry.type_at(&Level::arora_type_id(), &version("1.0.0"))
    else {
        panic!("Level's record is held");
    };
    assert_eq!(level.parent, ROOT_ID);
    assert_eq!(
        registry.resolve_id(&Level::arora_type_id()).await.unwrap(),
        "Level"
    );

    let written = records_as_yaml(&registry).expect("the records serialize");
    assert!(
        written
            .iter()
            .all(|(kind, _, _)| *kind != RecordKind::Folder),
        "no folder is written for records at the root"
    );
    let mut read = LocalRegistry::new();
    load_records(
        written
            .iter()
            .map(|(kind, stem, yaml)| (*kind, stem.as_str(), yaml.clone())),
        &mut read,
    )
    .await
    .expect("the records load");
    assert_eq!(records_as_yaml(&read).unwrap(), written);
}

/// A module naming a type with no record of its own fails, naming the type,
/// and its module record is not added.
#[tokio::test]
async fn a_declared_module_naming_a_type_without_a_record_fails() {
    let mut registry = LocalRegistry::new();
    let error = registry
        .add_declared_module::<windowed::Module>(version("0.1.0"))
        .await
        .expect_err("Window has no record");
    let message = error.to_string();
    assert!(
        message.contains(&reference::<Window>().to_string()),
        "{message}"
    );
    assert!(message.contains("module 'windowed'"), "{message}");
    assert!(registry
        .module_at(&windowed::ids::MODULE, &version("0.1.0"))
        .is_none());
}

/// A record already held identically under its id and version is kept; a
/// different record there is refused.
#[tokio::test]
async fn an_identical_record_is_kept_and_a_different_one_refused() {
    let mut registry = with_scope().await;
    registry
        .add_folder(SAMPLES.id, SAMPLES.record())
        .await
        .expect("the same folder again is kept");
    let Some(TypeDefinitionFrozen::Enumeration(mode)) = Mode::record(Some(SAMPLES)) else {
        panic!("Mode's record is an enumeration");
    };
    let id = Mode::arora_type_id();
    registry
        .add_enumeration(id, version("1.0.0"), mode.clone())
        .await
        .expect("the record is added");
    registry
        .add_enumeration(id, version("1.0.0"), mode.clone())
        .await
        .expect("the same record again is kept");
    let renamed = EnumerationFrozen {
        name: "Moded".to_string(),
        ..mode
    };
    let error = registry
        .add_enumeration(id, version("1.0.0"), renamed)
        .await
        .expect_err("another record under the same id and version is refused");
    assert!(
        matches!(error, RegistryError::DuplicateVersion { .. }),
        "{error}"
    );
}

/// Two versions of one record are held side by side, each found exactly; a
/// requirement resolves to the newest.
#[tokio::test]
async fn two_versions_of_a_record_are_held_side_by_side() {
    let mut registry = with_scope().await;
    let Some(TypeDefinitionFrozen::Structure(sample)) = Sample::record(Some(SAMPLES)) else {
        panic!("Sample's record is a structure");
    };
    let id = Sample::arora_type_id();
    let mut newer = sample.clone();
    newer.fields.insert(
        Uuid::from_u128(0x5a3e_0013),
        StructureField {
            name: "weight".to_string(),
            ty: FrozenTy::from(PrimitiveKind::F32),
        },
    );
    registry
        .add_structure(id, version("1.1.0"), sample.clone())
        .await
        .expect("1.1.0 is added");
    registry
        .add_structure(id, version("1.2.0"), newer.clone())
        .await
        .expect("1.2.0 is added beside it");
    assert_eq!(
        registry.type_at(&id, &version("1.1.0")),
        Some(TypeDefinitionFrozen::Structure(sample))
    );
    assert_eq!(
        registry.type_at(&id, &version("1.2.0")),
        Some(TypeDefinitionFrozen::Structure(newer))
    );
    assert_eq!(
        registry
            .resolve_tag(
                &arora_types::record::Selector::Path("samples.Sample".to_string()),
                &VersionReq::STAR
            )
            .await
            .unwrap(),
        version("1.2.0")
    );
}

/// What a record depends on and the registry does not hold is reported,
/// transitively; a well-known type is never missing.
#[tokio::test]
async fn missing_dependencies_are_found_through_every_record() {
    let mut registry = with_scope().await;
    let Some(TypeDefinitionFrozen::Structure(sample)) = Sample::record(Some(SAMPLES)) else {
        panic!("Sample's record is a structure");
    };
    registry
        .add_structure(Sample::arora_type_id(), version("1.1.0"), sample)
        .await
        .expect("Sample is added without Mode");
    assert_eq!(
        registry.missing_dependencies(&reference::<Sample>()),
        vec![reference::<Mode>()]
    );
    assert_eq!(
        registry.missing_dependencies(&reference::<Window>()),
        vec![reference::<Window>()],
        "a record the registry does not hold is itself missing"
    );

    let dynamic = StructureFrozen {
        parent: SAMPLES.id,
        name: "Envelope".to_string(),
        fields: [(
            Uuid::from_u128(0x5a3e_0041),
            StructureField {
                name: "payload".to_string(),
                ty: FrozenTy::FrozenScalar(FrozenScalar {
                    reference: FrozenReference {
                        id: *arora_types::ty::KEY_VALUE_ID,
                        version: version("1.0.0").into(),
                    },
                }),
            },
        )]
        .into_iter()
        .collect(),
    };
    let envelope = Uuid::from_u128(0x5a3e_0004);
    registry
        .add_structure(envelope, version("1.0.0"), dynamic)
        .await
        .expect("Envelope is added");
    assert!(registry
        .missing_dependencies(&FrozenReference {
            id: envelope,
            version: version("1.0.0").into(),
        })
        .is_empty());
}

/// The records a registry holds, written as YAML and loaded into another
/// registry, are the same records — in memory and through a directory.
#[tokio::test]
async fn records_written_as_yaml_load_back_as_the_same_records() {
    let mut registry = LocalRegistry::new();
    registry
        .add_declared_module::<sampler::Module>(version("0.3.0"))
        .await
        .expect("the declaration's records are added");
    let written = records_as_yaml(&registry).expect("the records serialize");
    let kinds: Vec<RecordKind> = written.iter().map(|(kind, _, _)| *kind).collect();
    assert_eq!(
        kinds,
        vec![
            RecordKind::Folder,
            RecordKind::Enumeration,
            RecordKind::Structure,
            RecordKind::Module
        ]
    );
    assert_eq!(written[3].1, format!("{}@0.3.0", sampler::ids::MODULE));

    let mut read = LocalRegistry::new();
    load_records(
        written
            .iter()
            .map(|(kind, stem, yaml)| (*kind, stem.as_str(), yaml.clone())),
        &mut read,
    )
    .await
    .expect("the records load");
    assert_eq!(records_as_yaml(&read).unwrap(), written);

    let dir = std::env::temp_dir().join(format!("arora-registry-records-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    write_records_to_yaml_dir(&registry, &dir).expect("the records are written");
    assert!(dir
        .join("module")
        .join(format!("{}@0.3.0.yaml", sampler::ids::MODULE))
        .is_file());
    let mut from_dir = LocalRegistry::new();
    load_records_from_yaml_dir(&dir, &mut from_dir)
        .await
        .expect("the directory loads");
    assert_eq!(records_as_yaml(&from_dir).unwrap(), written);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Folders load each after its parent, whatever order they come in: a
/// structure in a folder two levels deep, its folders written child first.
#[tokio::test]
async fn nested_folders_load_parent_first() {
    let (outer, inner) = (Uuid::from_u128(0xf2), Uuid::from_u128(0xf1));
    let mut registry = LocalRegistry::new();
    registry
        .add_folder(
            outer,
            FolderPublic {
                name: "vendor".to_string(),
                parent: ROOT_ID,
            },
        )
        .await
        .expect("the outer folder is added");
    registry
        .add_folder(
            inner,
            FolderPublic {
                name: "sampler".to_string(),
                parent: outer,
            },
        )
        .await
        .expect("the inner folder is added");
    let Some(TypeDefinitionFrozen::Structure(mut sample)) = Sample::record(None) else {
        panic!("Sample's record is a structure");
    };
    sample.parent = inner;
    registry
        .add_structure(Sample::arora_type_id(), version("1.1.0"), sample)
        .await
        .expect("the structure is added in the inner folder");
    let written = records_as_yaml(&registry).expect("the records serialize");
    // The child's id sorts first.
    assert_eq!(written[0].1, inner.to_string());

    let mut read = LocalRegistry::new();
    load_records(
        written
            .iter()
            .map(|(kind, stem, yaml)| (*kind, stem.as_str(), yaml.clone())),
        &mut read,
    )
    .await
    .expect("the inner folder waits for the outer one");
    assert_eq!(
        read.resolve_id(&Sample::arora_type_id()).await.unwrap(),
        "vendor.sampler.Sample"
    );

    let orphan = [(
        RecordKind::Folder,
        "00000000-0000-0000-0000-0000000000f3",
        "name: orphan\nparent: 00000000-0000-0000-0000-0000000000f4\n".to_string(),
    )];
    let error = load_records(orphan, &mut LocalRegistry::new())
        .await
        .expect_err("a folder whose parent is nowhere is refused");
    assert!(
        matches!(error, RegistryError::UnknownParent { .. }),
        "{error}"
    );
}

/// A structure with its fields in another order is another record.
#[tokio::test]
async fn a_structure_with_its_fields_reordered_is_another_record() {
    let mut registry = with_scope().await;
    let Some(TypeDefinitionFrozen::Structure(sample)) = Sample::record(Some(SAMPLES)) else {
        panic!("Sample's record is a structure");
    };
    let mut reordered = sample.clone();
    reordered.fields.reverse();
    let id = Sample::arora_type_id();
    registry
        .add_structure(id, version("1.1.0"), sample)
        .await
        .expect("the record is added");
    let error = registry
        .add_structure(id, version("1.1.0"), reordered)
        .await
        .expect_err("the fields' order is the record's");
    assert!(
        matches!(error, RegistryError::DuplicateVersion { .. }),
        "{error}"
    );
}

/// Each kind names the subdirectory its records are written in.
#[test]
fn a_record_kind_names_its_directory() {
    for kind in [
        RecordKind::Folder,
        RecordKind::Enumeration,
        RecordKind::Structure,
        RecordKind::Module,
    ] {
        assert_eq!(RecordKind::from_directory(kind.directory()), Some(kind));
    }
    assert_eq!(RecordKind::from_directory("records.json"), None);
}

/// A primitive answers `get_type` without a record, as before.
#[tokio::test]
async fn a_primitive_needs_no_record() {
    let mut registry = LocalRegistry::new();
    let primitive = registry
        .get_type(
            &arora_types::record::Selector::Id(*arora_types::ty::F32_ID),
            &VersionReq::STAR,
        )
        .await
        .expect("a primitive resolves");
    assert_eq!(
        primitive,
        TypeDefinitionFrozen::Primitive(PrimitiveKind::F32)
    );
}

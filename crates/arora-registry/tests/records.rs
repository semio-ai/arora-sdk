//! A registry holding the records a module and its types are described by:
//! added from a Rust declaration, looked up exactly, checked for what they
//! depend on, written in the record-directory layout and read back.

use arora_registry::local::{LocalRegistry, ROOT_ID};
use arora_registry::local_yaml::{
    load_records, load_records_from_yaml_dir, records_as_yaml, write_records_to_yaml_dir,
    RecordKind,
};
use arora_registry::{
    EditableRegistry, EnumerationFrozen, FolderPublic, ReadableRegistry, RegistryError,
    StructureFrozen, TypeDefinitionFrozen,
};
use arora_types::record::folder::DeclaredFolder;
use arora_types::record::structure::frozen::StructureField;
use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
use arora_types::record::FrozenReference;
use arora_types::{AroraType, Uuid};
use semver::{Version, VersionReq};

/// What `sample` returns: a structure naming an enumeration.
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
    license = "MIT",
    description = "A declared module whose functions name records",
    executable_mime = "application/wasm"
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
    license = "MIT",
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

/// A module naming `Sample`, as `sampler` does.
#[arora_module::module(
    id = "5a3e0000-0000-4000-8000-0000000000c0",
    name = "resampler",
    version = "0.1.0",
    author = "Semio",
    license = "MIT",
    description = "A declared module sharing a type that declares no folder",
    executable_mime = "application/wasm"
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

/// The home of `Reading`, which two modules share.
const SENSORS: DeclaredFolder = DeclaredFolder {
    id: Uuid::from_u128(0x5e45_f01d),
    name: "sensors",
    parent: None,
};

/// A type two modules share, declaring its folder.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "5a3e0000-0000-4000-8000-000000000004", parent = SENSORS)]
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
    license = "MIT",
    description = "A declared module naming a shared type",
    executable_mime = "application/wasm"
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
    license = "MIT",
    description = "Another declared module naming the shared type",
    executable_mime = "application/wasm"
)]
pub mod logger {
    use super::Reading;

    #[export(id = "5a3e0000-0000-4000-8000-0000000000e1")]
    pub fn history() -> Vec<Reading> {
        Vec::new()
    }
}

const FOLDER: Uuid = Uuid::from_u128(0x5a3e_f01d);

fn version(text: &str) -> Version {
    Version::parse(text).expect("a version")
}

fn reference<T: AroraType>() -> FrozenReference {
    FrozenReference {
        id: T::arora_type_id(),
        version: T::arora_type_version(),
    }
}

/// A registry holding the `sampler` folder and nothing else.
async fn with_folder() -> LocalRegistry {
    let mut registry = LocalRegistry::new();
    registry
        .add_folder(
            FOLDER,
            FolderPublic {
                name: "sampler".to_string(),
                parent: ROOT_ID,
            },
        )
        .await
        .expect("the folder is added");
    registry
}

/// The records a declared module carries — its module record, and those of
/// the types its functions name — are added in the folder given, the types
/// declaring no folder of their own there too; the registry then holds
/// everything the module record depends on, and finds it under its exact id
/// and version.
#[tokio::test]
async fn a_declared_module_adds_its_record_and_its_types_records() {
    let mut registry = with_folder().await;
    registry
        .add_declared_module::<sampler::Module>(version("0.3.0"), FOLDER)
        .await
        .expect("the declaration's records are added");

    let module = registry
        .module_at(&sampler::ids::MODULE, &version("0.3.0"))
        .expect("the module record is held at the header's version");
    assert_eq!(*module, sampler::record(FOLDER));
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
    assert_eq!(sample.parent, FOLDER);
    let Some(TypeDefinitionFrozen::Enumeration(mode)) =
        registry.type_at(&Mode::arora_type_id(), &version("1.0.0"))
    else {
        panic!("Mode's record is held at its version");
    };
    assert_eq!(mode.parent, FOLDER);
    assert_eq!(
        registry.resolve_id(&Sample::arora_type_id()).await.unwrap(),
        "sampler.Sample"
    );

    // The same records again are the ones it holds.
    registry
        .add_declared_module::<sampler::Module>(version("0.3.0"), FOLDER)
        .await
        .expect("adding the same records again keeps them");
}

/// Two modules in different folders sharing a type that declares its folder
/// add together: the shared type, and the type it names that declares none,
/// are one record each in the declared folder, which is added with them.
#[tokio::test]
async fn two_modules_sharing_a_type_that_declares_its_folder_add_together() {
    let mut registry = LocalRegistry::new();
    for (id, name) in [(0xd0, "thermometer"), (0xe0, "logger")] {
        registry
            .add_folder(
                Uuid::from_u128(id),
                FolderPublic {
                    name: name.to_string(),
                    parent: ROOT_ID,
                },
            )
            .await
            .expect("the module's folder is added");
    }
    registry
        .add_declared_module::<thermometer::Module>(version("1.0.0"), Uuid::from_u128(0xd0))
        .await
        .expect("the first module's records are added");
    registry
        .add_declared_module::<logger::Module>(version("1.0.0"), Uuid::from_u128(0xe0))
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
    for module in [thermometer::ids::MODULE, logger::ids::MODULE] {
        assert!(registry
            .missing_dependencies(&FrozenReference {
                id: module,
                version: version("1.0.0").into(),
            })
            .is_empty());
    }
}

/// A type declaring no folder lives in the folder of each module naming it,
/// so two modules in different folders sharing it carry two different
/// records under one id and version: the second is refused, naming the type.
/// From one folder, they add together.
#[tokio::test]
async fn a_type_declaring_no_folder_is_shared_only_within_one_folder() {
    let mut registry = with_folder().await;
    registry
        .add_folder(
            Uuid::from_u128(0xc0),
            FolderPublic {
                name: "resampler".to_string(),
                parent: ROOT_ID,
            },
        )
        .await
        .expect("the folder is added");
    registry
        .add_declared_module::<sampler::Module>(version("0.3.0"), FOLDER)
        .await
        .expect("the first module's records are added");
    let error = registry
        .add_declared_module::<resampler::Module>(version("0.1.0"), Uuid::from_u128(0xc0))
        .await
        .expect_err("Sample is held in another folder");
    let message = error.to_string();
    assert!(
        message.contains(&format!("type 'Sample' ({})", reference::<Sample>())),
        "{message}"
    );
    assert!(message.contains("module 'resampler'"), "{message}");
    assert!(registry
        .module_at(&resampler::ids::MODULE, &version("0.1.0"))
        .is_none());

    registry
        .add_declared_module::<resampler::Module>(version("0.1.0"), FOLDER)
        .await
        .expect("from the same folder, the records are the same");
}

/// A module naming a type with no record of its own fails, naming the type,
/// and its module record is not added.
#[tokio::test]
async fn a_declared_module_naming_a_type_without_a_record_fails() {
    let mut registry = with_folder().await;
    let error = registry
        .add_declared_module::<windowed::Module>(version("0.1.0"), FOLDER)
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
    let mut registry = with_folder().await;
    registry
        .add_folder(
            FOLDER,
            FolderPublic {
                name: "sampler".to_string(),
                parent: ROOT_ID,
            },
        )
        .await
        .expect("the same folder again is kept");
    let Some(TypeDefinitionFrozen::Enumeration(mode)) = Mode::record(FOLDER) else {
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
    let mut registry = with_folder().await;
    let Some(TypeDefinitionFrozen::Structure(sample)) = Sample::record(FOLDER) else {
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
                &arora_types::record::Selector::Path("sampler.Sample".to_string()),
                &VersionReq::STAR
            )
            .await
            .unwrap(),
        version("1.2.0")
    );
    // Another record under a path a record holds is still refused.
    let other = StructureFrozen {
        parent: FOLDER,
        name: "Sample".to_string(),
        fields: Default::default(),
    };
    let error = registry
        .add_structure(Uuid::from_u128(0x0bf1), version("1.0.0"), other)
        .await
        .expect_err("a path names one record");
    assert!(
        matches!(error, RegistryError::DuplicateSelector { .. }),
        "{error}"
    );
}

/// What a record depends on and the registry does not hold is reported,
/// transitively; a well-known type is never missing.
#[tokio::test]
async fn missing_dependencies_are_found_through_every_record() {
    let mut registry = with_folder().await;
    let Some(TypeDefinitionFrozen::Structure(sample)) = Sample::record(FOLDER) else {
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
        parent: FOLDER,
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
    let mut registry = with_folder().await;
    registry
        .add_declared_module::<sampler::Module>(version("0.3.0"), FOLDER)
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
/// module in a folder two levels deep, its folders written child first.
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
    registry
        .add_declared_module::<sampler::Module>(version("0.3.0"), inner)
        .await
        .expect("the declaration's records are added");
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
    let mut registry = with_folder().await;
    let Some(TypeDefinitionFrozen::Structure(sample)) = Sample::record(FOLDER) else {
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

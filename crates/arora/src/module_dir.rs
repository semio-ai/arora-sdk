//! A module directory: the form a device carries a guest module in — the
//! module's header beside the artifact the header's executor runs.
//!
//! ```text
//! <module directory>/
//! ├── header.json      the module's low-level Header, as JSON
//! └── <artifact>       the one file with the executor's extension: `.wasm`
//!                      for `wasm`, the platform's dynamic library (`.so`,
//!                      `.dylib`, `.dll`) for `native`
//! ```
//!
//! The artifact's name is free — its extension names it — so a published
//! artifact directory (a header beside its `.wasm`) is a module directory as
//! is. A device loads every module directory under its device directory's
//! `modules/` at start ([`in_device_dir`]), and any other the command line
//! names (`--module`, [`DeviceCli::modules`](crate::DeviceCli::modules)).
//!
//! A module directory that cannot be read — a missing or malformed header, no
//! artifact or several, an executor this runtime has no artifact convention
//! for — is an error naming the module by its directory's name: a device never
//! starts without a module it was given. An entry whose name starts with `.`
//! is neither a module directory nor an artifact: OS metadata (`.DS_Store`, an
//! AppleDouble `._x.wasm`) lands beside the files it describes, and is ignored
//! both under `modules/` and inside a module directory.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use arora_types::module::low::Header;

/// The subdirectory of the device directory holding its module directories.
const MODULES_DIR: &str = "modules";
/// The header's file name in a module directory.
const HEADER_FILE: &str = "header.json";

/// A module directory, read: what
/// [`AroraBuilder::with_module`](crate::AroraBuilder::with_module) loads.
#[derive(Debug)]
pub struct ModuleFiles {
    /// The module directory the files were read from.
    pub dir: PathBuf,
    /// The module's header, from its `header.json`.
    pub header: Header,
    /// The artifact's bytes, in the format the header's executor runs.
    pub executable: Box<[u8]>,
}

/// Read every module directory under `<device_dir>/modules`, in name order. A
/// device directory without a `modules/` carries no module; anything there
/// that is not a directory, or whose name starts with `.`, is not a module
/// directory.
pub fn in_device_dir(device_dir: &Path) -> Result<Vec<ModuleFiles>> {
    let modules = device_dir.join(MODULES_DIR);
    let mut dirs = match list(&modules) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("could not list {}", modules.display())),
    };
    dirs.retain(|path| path.is_dir());
    let modules: Vec<ModuleFiles> = dirs.iter().map(|dir| read(dir)).collect::<Result<_>>()?;
    distinct(&modules)?;
    Ok(modules)
}

/// Read the module directory `dir`: its `header.json`, and the one file with
/// the extension the header's executor names. The error of a directory that
/// cannot be read names the module by the directory's name.
pub fn read(dir: &Path) -> Result<ModuleFiles> {
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string());
    read_files(dir).with_context(|| format!("module '{name}' ({})", dir.display()))
}

/// Checks that no two of `modules` carry one module id: the engine keeps the
/// first it loads and the method index the last, so a module in two
/// directories is a configuration to refuse, not to resolve.
pub(crate) fn distinct(modules: &[ModuleFiles]) -> Result<()> {
    let mut seen: HashMap<uuid::Uuid, &ModuleFiles> = HashMap::new();
    for module in modules {
        if let Some(first) = seen.insert(module.header.id, module) {
            bail!(
                "module directories {} and {} both carry module '{}' ({}): a device loads a \
                 module from one directory",
                first.dir.display(),
                module.dir.display(),
                module.header.name,
                module.header.id
            );
        }
    }
    Ok(())
}

fn read_files(dir: &Path) -> Result<ModuleFiles> {
    let header_path = dir.join(HEADER_FILE);
    let header_text = fs::read_to_string(&header_path)
        .with_context(|| format!("could not read {}", header_path.display()))?;
    let header: Header = serde_json::from_str(&header_text)
        .with_context(|| format!("{} is not a module header", header_path.display()))?;
    log::info!(
        "module directory {}: '{}' ({}, executor {})",
        dir.display(),
        header.name,
        header.id,
        header.executor.name
    );
    let executor = header.executor.name.as_str();
    let extension = artifact_extension(executor)?;
    let artifact = artifact_in(dir, executor, extension)?;
    let executable =
        fs::read(&artifact).with_context(|| format!("could not read {}", artifact.display()))?;
    Ok(ModuleFiles {
        dir: dir.to_path_buf(),
        header,
        executable: executable.into_boxed_slice(),
    })
}

/// The extension of the artifact `executor` runs: `wasm` for the WebAssembly
/// executor, the platform's dynamic-library extension for the native one.
/// These are the executors the runtime's engine carries; any other has no
/// artifact here to look for.
fn artifact_extension(executor: &str) -> Result<&'static str> {
    match executor {
        "wasm" => Ok("wasm"),
        "native" => Ok(std::env::consts::DLL_EXTENSION),
        other => bail!(
            "the header names executor '{other}', which this device does not run (wasm, native)"
        ),
    }
}

/// The one file in `dir` with `extension`.
fn artifact_in(dir: &Path, executor: &str, extension: &str) -> Result<PathBuf> {
    let mut found = list(dir).with_context(|| format!("could not list {}", dir.display()))?;
    found.retain(|path| path.is_file() && path.extension().is_some_and(|ext| ext == extension));
    match found.as_slice() {
        [artifact] => Ok(artifact.clone()),
        [] => {
            bail!("no .{extension} file beside {HEADER_FILE}: the executor '{executor}' runs one")
        }
        several => {
            let names: Vec<String> = several
                .iter()
                .filter_map(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .collect();
            bail!(
                "several .{extension} files ({}): a module directory holds one artifact",
                names.join(", ")
            )
        }
    }
}

/// The entries of `dir` whose name does not start with `.`, sorted by path.
fn list(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<_>>()?;
    entries.retain(|path| {
        !path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
    });
    entries.sort();
    Ok(entries)
}

/// Reading module directories, loading what they hold through the builder, and
/// dispatching it. `test-rust-wasm` is the guest: its header comes from its
/// Rust declaration, its `.wasm` from the `wasm32-wasip1` cdylib artifact
/// dependency Cargo builds.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Arora;
    use arora_types::call::Call;
    use arora_types::module::low::Executor;
    use arora_types::value::Value;
    use uuid::Uuid;

    const WASM: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));

    // `succeed() -> bool`, as the guest's declaration pins it.
    const SUCCEED: &str = "00cd31a8-2cf4-48e6-a957-69a55de90424";

    /// A fresh device directory under the system temp dir, unique to `name`
    /// and this process.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("arora-module-dir-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn header(executor: &str) -> Header {
        test_rust_wasm::test_rust_wasm::header(Executor {
            name: executor.to_string(),
            min_version: None,
            max_version: None,
        })
    }

    /// A module directory `<device_dir>/modules/<name>` holding `header` and
    /// each of `files`.
    fn module_dir(device_dir: &Path, name: &str, header: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = device_dir.join(MODULES_DIR).join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(HEADER_FILE), header).unwrap();
        for (file, bytes) in files {
            fs::write(dir.join(file), bytes).unwrap();
        }
        dir
    }

    fn json(header: &Header) -> String {
        serde_json::to_string_pretty(header).unwrap()
    }

    /// The modules under a device directory load into a device, and their
    /// functions dispatch through [`Arora::call`] like any module's. The
    /// artifact keeps the name it was published under.
    #[test]
    fn the_device_directory_s_modules_load_and_dispatch() {
        let device_dir = scratch("loads");
        let header = header("wasm");
        let module_id = header.id;
        module_dir(
            &device_dir,
            "sinus",
            &json(&header),
            &[("test_rust_wasm.wasm", WASM)],
        );

        let modules = in_device_dir(&device_dir).expect("the module directory reads");
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].header.id, module_id);
        assert_eq!(modules[0].dir, device_dir.join("modules").join("sinus"));

        let mut builder = Arora::builder();
        for module in modules {
            builder = builder.with_module(module.header, module.executable);
        }
        let mut arora = builder.build().expect("build a device with the module");
        let result = arora
            .call(Call {
                module_id: Some(module_id),
                id: Uuid::parse_str(SUCCEED).unwrap(),
                args: Vec::new(),
            })
            .expect("call succeed() on the loaded module");
        assert_eq!(result.ret, Value::Boolean(true));
        fs::remove_dir_all(&device_dir).unwrap();
    }

    /// A device directory with no `modules/` carries no module; a file among
    /// the module directories is not one, nor is a directory whose name
    /// starts with `.` (OS metadata).
    #[test]
    fn a_device_directory_without_modules_carries_none() {
        let device_dir = scratch("none");
        assert!(in_device_dir(&device_dir).unwrap().is_empty());
        let modules = device_dir.join(MODULES_DIR);
        fs::create_dir_all(modules.join(".git")).unwrap();
        fs::create_dir_all(modules.join(".Trashes")).unwrap();
        fs::write(modules.join(".DS_Store"), b"").unwrap();
        fs::write(modules.join("notes.txt"), b"").unwrap();
        assert!(in_device_dir(&device_dir).unwrap().is_empty());
        fs::remove_dir_all(&device_dir).unwrap();
    }

    /// OS metadata beside the artifact — an AppleDouble `._m.wasm`, a
    /// `.DS_Store` — is not an artifact: the one `.wasm` is still found.
    #[test]
    fn metadata_beside_the_artifact_is_not_an_artifact() {
        let device_dir = scratch("apple-double");
        module_dir(
            &device_dir,
            "sinus",
            &json(&header("wasm")),
            &[
                ("m.wasm", WASM),
                ("._m.wasm", b"AppleDouble"),
                (".DS_Store", b""),
            ],
        );
        let modules = in_device_dir(&device_dir).expect("the sidecar is ignored");
        assert_eq!(modules.len(), 1);
        assert_eq!(&*modules[0].executable, WASM);
        fs::remove_dir_all(&device_dir).unwrap();
    }

    /// A header that is not one fails the read, naming the module.
    #[test]
    fn a_broken_header_fails_naming_the_module() {
        let device_dir = scratch("broken-header");
        module_dir(&device_dir, "broken", "{ not a header", &[]);
        let error = in_device_dir(&device_dir).expect_err("a broken header is refused");
        let message = format!("{error:#}");
        assert!(message.contains("module 'broken'"), "{message}");
        assert!(message.contains("is not a module header"), "{message}");
        fs::remove_dir_all(&device_dir).unwrap();
    }

    /// A module directory without a header fails the read, naming the module.
    #[test]
    fn a_missing_header_fails_naming_the_module() {
        let device_dir = scratch("no-header");
        let dir = device_dir.join(MODULES_DIR).join("headless");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("module.wasm"), WASM).unwrap();
        let error = in_device_dir(&device_dir).expect_err("a module without a header is refused");
        let message = format!("{error:#}");
        assert!(message.contains("module 'headless'"), "{message}");
        assert!(message.contains("header.json"), "{message}");
        fs::remove_dir_all(&device_dir).unwrap();
    }

    /// The artifact is the one file with the executor's extension: none, or
    /// several, fails the read naming the module.
    #[test]
    fn the_artifact_is_the_one_file_with_the_executor_s_extension() {
        let device_dir = scratch("artifacts");
        let header = json(&header("wasm"));
        module_dir(
            &device_dir,
            "bare",
            &header,
            &[("readme.md", b"no wasm here")],
        );
        let error = in_device_dir(&device_dir).expect_err("no artifact is refused");
        let message = format!("{error:#}");
        assert!(message.contains("module 'bare'"), "{message}");
        assert!(message.contains("no .wasm file"), "{message}");

        let _ = fs::remove_dir_all(device_dir.join(MODULES_DIR));
        module_dir(
            &device_dir,
            "twice",
            &header,
            &[("a.wasm", WASM), ("b.wasm", WASM)],
        );
        let error = in_device_dir(&device_dir).expect_err("two artifacts are refused");
        let message = format!("{error:#}");
        assert!(message.contains("module 'twice'"), "{message}");
        assert!(
            message.contains("several .wasm files (a.wasm, b.wasm)"),
            "{message}"
        );
        fs::remove_dir_all(&device_dir).unwrap();
    }

    /// A native module's artifact is the platform's dynamic library; the read
    /// does not judge its bytes (the engine does, at load).
    #[test]
    fn a_native_module_s_artifact_is_the_platform_s_dynamic_library() {
        let device_dir = scratch("native");
        let library = format!("libthing.{}", std::env::consts::DLL_EXTENSION);
        module_dir(
            &device_dir,
            "thing",
            &json(&header("native")),
            &[(library.as_str(), b"not a library"), ("thing.wasm", WASM)],
        );
        let modules = in_device_dir(&device_dir).expect("the native module reads");
        assert_eq!(&*modules[0].executable, b"not a library");
        fs::remove_dir_all(&device_dir).unwrap();
    }

    /// An executor this device does not run has no artifact to look for.
    #[test]
    fn an_executor_the_device_does_not_run_fails_naming_it() {
        let device_dir = scratch("executor");
        module_dir(&device_dir, "py", &json(&header("python")), &[]);
        let error = in_device_dir(&device_dir).expect_err("an unknown executor is refused");
        let message = format!("{error:#}");
        assert!(message.contains("module 'py'"), "{message}");
        assert!(message.contains("executor 'python'"), "{message}");
        fs::remove_dir_all(&device_dir).unwrap();
    }

    /// One module in two directories is refused, naming both directories and
    /// the module.
    #[test]
    fn one_module_in_two_directories_is_refused() {
        let device_dir = scratch("twins");
        let header = json(&header("wasm"));
        let first = module_dir(&device_dir, "first", &header, &[("m.wasm", WASM)]);
        let second = module_dir(&device_dir, "second", &header, &[("m.wasm", WASM)]);
        let error = in_device_dir(&device_dir).expect_err("a module in two directories is refused");
        let message = format!("{error:#}");
        assert!(
            message.contains(&format!(
                "module directories {} and {} both carry module 'test-rust-wasm'",
                first.display(),
                second.display()
            )),
            "{message}"
        );
        fs::remove_dir_all(&device_dir).unwrap();
    }

    /// The modules load in the order of their directories' names, so a device
    /// starts the same way each time.
    #[test]
    fn modules_read_in_name_order() {
        let device_dir = scratch("order");
        let base = header("wasm");
        for (name, id) in [("b", 2u128), ("a", 1), ("c", 3)] {
            let mut header = base.clone();
            header.id = Uuid::from_u128(id);
            module_dir(&device_dir, name, &json(&header), &[("m.wasm", WASM)]);
        }
        let ids: Vec<u128> = in_device_dir(&device_dir)
            .unwrap()
            .iter()
            .map(|module| module.header.id.as_u128())
            .collect();
        assert_eq!(ids, vec![1, 2, 3]);
        fs::remove_dir_all(&device_dir).unwrap();
    }
}

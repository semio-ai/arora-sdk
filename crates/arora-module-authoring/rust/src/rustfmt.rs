use crate::GenerationError;
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

/// Run `rustfmt` on all Rust files found under the given path.
pub async fn apply_rustfmt<P: AsRef<Path>>(path: P) -> Result<(), GenerationError> {
    let rust_files = list_all_files(path, |sub_path| {
        sub_path.extension() == Some(OsStr::new("rs"))
    })
    .map_err(GenerationError::IoError)?;
    let rustfmt_status = tokio::process::Command::new("rustfmt")
        .args(&rust_files)
        .spawn()
        .map_err(GenerationError::IoError)?
        .wait()
        .await
        .map_err(GenerationError::IoError)?;
    if rustfmt_status.success() {
        Ok(())
    } else {
        Err(GenerationError::Generic(format!(
            "rustfmt exited with non-zero status: {:?}",
            rustfmt_status.code()
        )))
    }
}

/// Helper to list all files recursively under the given path,
/// while filtering result (files only) with a custom predicate.
pub fn list_all_files<P: AsRef<Path>, F: Fn(&Path) -> bool>(
    path: P,
    predicate: F,
) -> io::Result<Vec<PathBuf>> {
    list_all_files_ref(path, &predicate)
}

fn list_all_files_ref<P: AsRef<Path>, F: Fn(&Path) -> bool>(
    path: P,
    predicate: &F,
) -> io::Result<Vec<PathBuf>> {
    let path = path.as_ref();
    if path.is_dir() {
        let mut result = Vec::new();
        for entry in path.read_dir()? {
            let sub_result = list_all_files_ref(entry?.path(), predicate)?;
            result.extend(sub_result);
        }
        Ok(result)
    } else {
        if (*predicate)(path) {
            Ok(vec![path.to_path_buf()])
        } else {
            Ok(vec![])
        }
    }
}

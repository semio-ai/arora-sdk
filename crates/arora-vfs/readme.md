# arora-vfs

A directory tree held in memory, built up by code and written to disk in one
step. Arora's module code generators assemble the files they produce in it,
then write them out.

A [`Directory`](src/lib.rs) maps names to entries; an `Entry` is a `File`
(its bytes) or another `Directory`. The tree serializes with serde.

```rust
use arora_vfs::{Directory, File};

let mut generated = Directory::new();
generated.insert("module.yaml", File::new("id: …"))?;
generated
    .ensure_directories("src/arora_generated".as_ref())?
    .insert("lib.rs", File::new("// generated"))?;
generated.sync("out".into()).await?;
```

- `insert` and `insert_at_path` add an entry, refusing a name already taken;
  `ensure_directories` creates a path of directories as needed, and
  `get_mut_or_insert` returns an entry, creating it when absent.
- `sync` writes the tree under a path, creating directories. A file whose
  content on disk is already the same — compared by SHA-256 — is left
  untouched, so regenerating unchanged sources changes no file and triggers no
  rebuild. Files on disk the tree does not hold stay.
- `merge_with` combines two trees, merging directories present in both; a file
  present in both panics.
- A failed operation is a `VfsError`.

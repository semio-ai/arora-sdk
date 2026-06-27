use arora_module_core::{
    analyze_module_from_path, header::generate_header_file, ModuleAsset, Reader, Writer,
};
use arora_registry::ReadableRegistry;
use arora_vfs::Entry;
use clap::Parser;
use semio_record::record::Freezer;

#[derive(Debug, Parser)]
pub struct Generate {
    #[clap(short, long, name = "configuration-file")]
    pub module_file: String,
    #[clap(short, long)]
    pub language: String,
    #[clap(short, long, name = "output-directory")]
    pub output_directory: String,

    #[clap(long, name = "dry-run")]
    pub dry_run: bool,

    pub var_args: Vec<String>,
}

fn print_entry(entry: &Entry, i: usize) {
    match *entry {
        Entry::Directory(ref directory) => {
            for (name, entry) in directory.entries.iter() {
                println!("{} {}", " ".repeat(i), name);
                print_entry(entry, i + 2);
            }
        }
        Entry::File(_) => {}
    }
}

pub async fn generate<R: ReadableRegistry + Freezer>(
    cmd: Generate,
    registry: &mut R,
) -> anyhow::Result<()> {
    let assets = analyze_module_from_path(cmd.module_file, registry).await?;
    let (module_id, tag, module, executor) = match assets.last() {
        Some(ModuleAsset::Module(module_id, tag, module, executor)) => (
            module_id.to_owned(),
            tag.to_owned(),
            module.to_owned(),
            executor.to_owned(),
        ),
        _ => panic!("last module asset should be the module!"),
    };

    let mut generator_path = std::env::current_exe()?;
    generator_path.pop();
    generator_path.push(format!(
        "arora-module-{}{}",
        cmd.language,
        std::env::consts::EXE_SUFFIX
    ));

    let mut command = tokio::process::Command::new(&generator_path)
        .args(["--self-id", &module_id.to_string()])
        .args(["--self-version", &tag.to_string()])
        .args(cmd.var_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| anyhow::anyhow!("Failed to start generator {:?}", generator_path))?;

    let mut stdin = command.stdin.as_mut().unwrap();
    let mut stdout = command.stdout.as_mut().unwrap();

    let mut writer = Writer::new(&mut stdin);
    let mut reader = Reader::new(&mut stdout);

    let mut imports = Vec::new();
    for asset in assets {
        if let ModuleAsset::Import(ref import) = asset {
            imports.push(import.to_owned())
        };
        writer.write(asset).await?;
    }
    writer.end().await?;

    let vfs = reader.read::<Entry>().await?;
    assert!(reader.read::<Entry>().await?.is_none());

    let status = command.wait().await?;
    if !status.success() {
        anyhow::bail!("Generator failed with status {:?}", status);
    }

    if let Some(vfs) = vfs {
        // Now we have the vfs.
        if cmd.dry_run {
            println!("{}", cmd.output_directory);
            print_entry(&vfs, 0);
            return Ok(());
        } else {
            vfs.sync(cmd.output_directory.clone().into()).await?;
        }
    } else {
        anyhow::bail!("Failed to read virtual filesystem");
    }

    let mut module_low = std::path::PathBuf::new();
    module_low.push(cmd.output_directory);
    let header_file = generate_header_file(&module_id, &module, &imports, &executor)?;
    header_file
        .sync(module_low)
        .await
        .map_err(|err| anyhow::anyhow!("failed to write header: {}", err))?;

    Ok(())
}

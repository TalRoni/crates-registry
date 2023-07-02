use std::{fs::File, path::Path};

use anyhow::Result;
use tar::Archive;
use tempfile::TempDir;
use tracing::{debug, info};

use crate::{
    cli::PackArgs,
    rustup::{download_latest, download_pinned_rust_version},
};

pub async fn pack(pack_args: PackArgs) -> Result<()> {
    let root_registry = TempDir::new()?;
    debug!("Root registry: {}", root_registry.path().display());
    if !pack_args.rust_versions.is_empty() {
        download_pinned_rust_version(root_registry.path(), &pack_args).await?;
    } else {
        download_latest(root_registry.path(), &pack_args).await?;
    }

    info!(
        "Collect file installations to the pack file: {}",
        pack_args.pack_file.display()
    );

    let tar_file = File::create(&pack_args.pack_file)?;
    // let enc = GzEncoder::new(tar_gz, Compression::none());
    let mut tar = tar::Builder::new(tar_file);
    tar.append_dir_all(".", root_registry.path())?;

    info!("The packing finished");
    Ok(())
}

pub async fn unpack(packed_file: &Path, root_registry: &Path) -> Result<()> {
    info!(
        "Unpacking file installations...\n
        Packed file: {}\n
        Registry: {}",
        packed_file.display(),
        root_registry.display()
    );

    let tar_file = File::open(packed_file)?;
    // let enc = GzEncoder::new(tar_gz, Compression::none());
    let mut archive = Archive::new(tar_file);
    // TODO: handle history channel files if needed
    archive.unpack(root_registry)?;
    info!("The unpacking finished");
    Ok(())
}

use std::path::{Path, PathBuf};

/// Copy files from source to destination recursively.
pub fn copy_folder(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> std::io::Result<()> {
    std::fs::create_dir_all(&destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let filetype = entry.file_type()?;
        if filetype.is_dir() {
            copy_folder(entry.path(), destination.as_ref().join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), destination.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn main() {
    println!("cargo:rerun-if-changed=frontend/build");
    let frontend_dist_folder = PathBuf::from("frontend/build");
    if !frontend_dist_folder.exists() {
        eprintln!("You must build the Frontend first. please see the relevant script command in `frontend/package.json`.");
    }

    let frontend_dist_folder_out =
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("frontend_dist_folder");
    copy_folder(frontend_dist_folder, frontend_dist_folder_out).unwrap();
}

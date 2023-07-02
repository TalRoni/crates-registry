use std::process::Command;

#[cfg(target_os = "windows")]
fn build_frontend() {
    Command::new("powershell")
        .arg("-Command")
        .arg("yarn install")
        .current_dir("frontend")
        .output()
        .expect("Failed to install frontend dependencies (windows target)");
    Command::new("powershell")
        .arg("-Command")
        .arg("yarn build")
        .current_dir("frontend")
        .output()
        .expect("Failed to build the frontend (windows target)");
}

#[cfg(target_os = "linux")]
fn build_frontend() {
    Command::new("yarn")
        .arg("install")
        .current_dir("frontend")
        .output()
        .expect("Failed to install frontend dependencies (linux target)");
    Command::new("yarn")
        .arg("build")
        .current_dir("frontend")
        .output()
        .expect("Failed to build the frontend (linux target)");
}

fn main() {
    if std::env::var("SKIP_BUILDING_FRONTEND").is_ok() {
        return;
    }
    println!("cargo:rerun-if-changed=frontend/src");
    eprintln!("Require yarn.");
    build_frontend()
}

use std::{net::SocketAddr, path::PathBuf, str::FromStr};

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    /// Increase verbosity (can be supplied multiple times).
    #[arg(short, long, global = true, default_value_t = 1)]
    pub verbosity: usize,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Pack Rust installations to serve later.
    Pack(PackArgs),
    /// Print all available platforms installations to the stdout.
    PlatformsList,
    /// Unpack Rust installation before serving into root registry.
    Unpack(UnpackArgs),
    /// Serve offline crates registry.
    Serve(ServeArgs),
}

#[derive(Args)]
pub struct UnpackArgs {
    /// Path to the src compressed file (we support tar file).
    #[arg(short, long)]
    pub packed_file: PathBuf,
    /// Extract the compressed file here (Be carefull this will override some files).
    #[arg(short, long)]
    pub root_registry: PathBuf,
}

#[derive(Args)]
pub struct PackArgs {
    /// Path to the dst compressed file.
    #[arg(short, long)]
    pub(crate) pack_file: PathBuf,
    /// The rust versions for collecting all installation files seperated by comma.
    /// Valid versions could be "1.67.1", "1.54", and "nightly-2014-12-18".
    /// In emptry case, Crates-Registry will pack the latest versions of the stable release and the nightly release.
    #[arg(short, long, value_delimiter=',')]
    pub(crate) rust_versions: Vec<String>,
    /// The platforms for collecting seperated by comma.
    /// You can run `crates-registry platfroms-list` to show all available platfroms.
    /// Valid platforms could be x86_64-unknown-linux-gnu or x86_64-pc-windows-msvc.
    #[arg(long, value_delimiter=',')]
    pub(crate) platforms: Vec<String>,
    /// Number of downloads that can be ran in parallel.
    #[arg(short, long, default_value_t = 16)]
    pub(crate) threads: usize,
    /// Where to download rustup files from.
    #[arg(short, long, default_value = "https://static.rust-lang.org")]
    pub(crate) source: String,
    /// Number of download retries before giving up.
    #[arg(long, default_value_t = 5)]
    pub(crate) retries: usize,
}

#[derive(Args)]
pub struct ServeArgs {
    /// The root directory of the registry. if the path does not exists Crates-Registry will create it's
    #[arg(long)]
    pub root_registry: PathBuf,
    /// The address to serve on. By default we serve on 0.0.0.0:5000
    #[arg(short, long, value_parser = SocketAddr::from_str, default_value_t = SocketAddr::from(([0, 0, 0, 0], 5000)))]
    pub binding_addr: SocketAddr,
    /// The address of the server. By default the address is the local address: 127.0.0.1:5000
    #[arg(short, long, value_parser = SocketAddr::from_str, default_value_t = SocketAddr::from(([127, 0, 0, 1], 5000)))]
    pub server_addr: SocketAddr,
}

use anyhow::{Context, Result};

use clap::Parser;
use crates_registry::{download_platform_list, pack, serve, unpack, Cli, Commands};

use itertools::Itertools;
use tracing::subscriber::set_global_default as set_global_subscriber;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::time::SystemTime;
use tracing_subscriber::FmtSubscriber;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info")
    }

    env_logger::init();
    let cli = Cli::parse();
    let level = match cli.verbosity {
        0 => LevelFilter::WARN,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_timer(SystemTime)
        .finish();

    set_global_subscriber(subscriber).context("failed to set tracing subscriber")?;
    match cli.command {
        Commands::Serve(serve_args) => {
            serve(
                &serve_args.root_registry,
                serve_args.binding_addr,
                serve_args.server_addr,
            )
            .await?
        }
        Commands::Pack(pack_args) => pack(pack_args).await?,
        Commands::PlatformsList => {
            let platforms =
                download_platform_list("https://static.rust-lang.org", "nightly").await?;
            println!(
                "available platforms:\n - {}",
                platforms.into_iter().join("\n - ")
            )
        }
        Commands::Unpack(unpack_args) => {
            unpack(&unpack_args.packed_file, &unpack_args.root_registry).await?
        }
    };
    Ok(())
}

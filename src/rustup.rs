use crate::cli::PackArgs;
use crate::download::{
    append_to_path, copy_file_create_dir_with_sha256, download, download_string,
    download_with_sha256_file, move_if_exists, move_if_exists_with_sha256, write_file_create_dir,
    DownloadError,
};
use anyhow::{anyhow, Result};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};
use itertools::Itertools;
use reqwest::header::HeaderValue;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};
use thiserror::Error;
use tokio::task::JoinError;
use tracing::{error, info, warn};

// The allowed platforms to validate the configuration
// Note: These platforms should match the list on https://rust-lang.github.io/rustup/installation/other.html

/// Windows platforms (platforms where rustup-init has a .exe extension)
static PLATFORMS_WINDOWS: &[&str] = &[
    "i586-pc-windows-msvc",
    "i686-pc-windows-gnu",
    "i686-pc-windows-msvc",
    "x86_64-pc-windows-gnu",
    "x86_64-pc-windows-msvc",
];

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Toml error: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("Toml error: {0}")]
    Deserialize(#[from] toml::de::Error),

    #[error("Download error: {0}")]
    Download(#[from] DownloadError),

    #[error("Path prefix strip error: {0}")]
    StripPrefix(#[from] std::path::StripPrefixError),

    #[error("Failed {count} downloads")]
    FailedDownloads { count: usize },
}

#[derive(Deserialize, Debug)]
pub struct TargetUrls {
    pub url: String,
    pub hash: String,
    pub xz_url: String,
    pub xz_hash: String,
}

#[derive(Deserialize, Debug)]
pub struct Target {
    pub available: bool,

    #[serde(flatten)]
    pub target_urls: Option<TargetUrls>,
}

#[derive(Deserialize, Debug)]
pub struct Pkg {
    pub version: String,
    pub target: HashMap<String, Target>,
}

#[derive(Deserialize, Debug)]
pub struct Channel {
    #[serde(alias = "manifest-version")]
    pub manifest_version: String,
    pub date: String,
    pub pkg: HashMap<String, Pkg>,
}

#[derive(Deserialize, Debug)]
struct Release {
    version: String,
}

#[derive(Deserialize, Debug, Default)]
pub struct Platforms {
    unix: Vec<String>,
    windows: Vec<String>,
}

impl Platforms {
    // &String instead of &str is required due to vec.contains not performing proper inference
    // here. See:
    // https://stackoverflow.com/questions/48985924/why-does-a-str-not-coerce-to-a-string-when-using-veccontains
    // https://github.com/rust-lang/rust/issues/42671
    #[allow(clippy::ptr_arg)]
    pub fn contains(&self, platform: &String) -> bool {
        self.unix.contains(platform) || self.windows.contains(platform)
    }

    pub fn len(&self) -> usize {
        self.unix.len() + self.windows.len()
    }
}

impl<'a> IntoIterator for &'a Platforms {
    type Item = &'a String;

    type IntoIter = PlatformsIntoIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        PlatformsIntoIterator {
            platforms: &self,
            index: 0,
        }
    }
}

pub struct PlatformsIntoIterator<'a> {
    platforms: &'a Platforms,
    index: usize,
}

impl<'a> Iterator for PlatformsIntoIterator<'a> {
    type Item = &'a String;

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.platforms.len() - self.index;
        (len, Some(len))
    }

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.index;
        let unix_len = self.platforms.unix.len();
        self.index += 1;
        if index < unix_len {
            self.platforms.unix.get(index)
        } else {
            self.platforms.windows.get(index - unix_len)
        }
    }
}

pub async fn download_platform_list(source: &str, channel: &str) -> Result<Platforms> {
    let channel_url = format!("{source}/dist/channel-rust-{channel}.toml");
    let user_agent =
        HeaderValue::from_str(&format!("Offline Mirror/{}", env!("CARGO_PKG_VERSION")))
            .expect("Hardcoded user agent string should never fail.");
    let channel_str = download_string(&channel_url, &user_agent).await?;
    let channel_data: Channel = toml::from_str(&channel_str)?;

    let mut targets = HashSet::new();

    for (_, pkg) in channel_data.pkg {
        for (target, _) in pkg.target {
            if target == "*" {
                continue;
            }
            targets.insert(target);
        }
    }

    let mut targets: Vec<String> = targets.into_iter().collect();
    targets.sort();
    let unix = targets
        .iter()
        .filter(|x| !PLATFORMS_WINDOWS.contains(&x.as_str()))
        .map(|x| x.to_string())
        .collect();

    let windows = PLATFORMS_WINDOWS.iter().map(|x| x.to_string()).collect();
    Ok(Platforms { unix, windows })
}

pub async fn get_platforms(pack_args: &PackArgs) -> Result<Platforms> {
    let all_platforms = download_platform_list(&pack_args.source, "nightly").await?;
    Ok(if pack_args.platforms.is_empty() {
        all_platforms
    } else {
        pack_args.platforms.iter().cloned().try_fold(
            Platforms::default(),
            |mut platforms, platform| {
                if all_platforms.windows.contains(&platform) {
                    platforms.windows.push(platform);
                } else if all_platforms.unix.contains(&platform) {
                    platforms.unix.push(platform);
                } else {
                    return Err(anyhow!("Wrong platform: {platform}"));
                }
                Ok(platforms)
            },
        )?
    })
}

/// Synchronize one rustup-init file.
#[allow(clippy::too_many_arguments)]
pub async fn sync_one_init(
    client: &Client,
    path: &Path,
    source: &str,
    platform: &str,
    is_exe: bool,
    rustup_version: &str,
    retries: usize,
    user_agent: &HeaderValue,
) -> Result<(), DownloadError> {
    let local_path = path
        .join("rustup")
        .join("archive")
        .join(rustup_version)
        .join(platform)
        .join(if is_exe {
            "rustup-init.exe"
        } else {
            "rustup-init"
        });

    let archive_path = path.join("rustup/dist").join(platform).join(if is_exe {
        "rustup-init.exe"
    } else {
        "rustup-init"
    });

    let source_url = if is_exe {
        format!("{source}/rustup/dist/{platform}/rustup-init.exe")
    } else {
        format!("{source}/rustup/dist/{platform}/rustup-init")
    };

    download_with_sha256_file(client, &source_url, &local_path, retries, false, user_agent).await?;
    copy_file_create_dir_with_sha256(&local_path, &archive_path)?;

    Ok(())
}

fn registry_progress_bar(size: usize) -> ProgressBar {
    ProgressBar::new(size as u64)
        .with_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})",
            )
            .expect("template is correct")
            .progress_chars("#>-"),
        )
        .with_finish(ProgressFinish::AndLeave)
}

#[allow(clippy::too_many_arguments)]
async fn create_sync_tasks(
    platforms: &[String],
    is_exe: bool,
    rustup_version: &str,
    path: &Path,
    pack_args: &PackArgs,
    user_agent: &HeaderValue,
    pb: &ProgressBar,
) -> Vec<Result<Result<(), DownloadError>, JoinError>> {
    let client = Client::new();
    futures::stream::iter(platforms.iter())
        .map(|platform| {
            let client = client.clone();
            let rustup_version = rustup_version.to_string();
            let path = path.to_path_buf();
            let source = pack_args.source.to_string();
            let retries = pack_args.retries;
            let user_agent = user_agent.clone();
            let platform = platform.clone();
            let pb = pb.clone();

            tokio::spawn(async move {
                let out = sync_one_init(
                    &client,
                    &path,
                    &source,
                    platform.as_str(),
                    is_exe,
                    &rustup_version,
                    retries,
                    &user_agent,
                )
                .await;

                pb.inc(1);

                out
            })
        })
        .buffer_unordered(pack_args.threads)
        .collect::<Vec<Result<_, _>>>()
        .await
}

/// Synchronize all rustup-init files.
pub async fn sync_rustup_init(
    path: &Path,
    pack_args: &PackArgs,
    user_agent: &HeaderValue,
    platforms: &Platforms,
) -> Result<(), SyncError> {
    info!("Downloading rustup-init files...");
    let mut errors_occurred = 0usize;

    let client = Client::new();

    // Download rustup release file
    let release_url = format!("{}/rustup/release-stable.toml", pack_args.source);
    let release_path = path.join("rustup/release-stable.toml");
    let release_part_path = append_to_path(&release_path, ".part");

    download(
        &client,
        &release_url,
        &release_part_path,
        None,
        pack_args.retries,
        false,
        user_agent,
    )
    .await?;

    let rustup_version = get_rustup_version(&release_part_path)?;

    move_if_exists(&release_part_path, &release_path)?;

    let pb = registry_progress_bar(platforms.len());
    pb.enable_steady_tick(Duration::from_millis(10));

    let unix_tasks = create_sync_tasks(
        &platforms.unix,
        false,
        &rustup_version,
        path,
        pack_args,
        user_agent,
        &pb,
    )
    .await;

    let win_tasks = create_sync_tasks(
        &platforms.windows,
        true,
        &rustup_version,
        path,
        pack_args,
        user_agent,
        &pb,
    )
    .await;

    for res in unix_tasks.into_iter().chain(win_tasks) {
        // Unwrap the join result.
        let res = res.unwrap();

        if let Err(e) = res {
            match e {
                DownloadError::NotFound { .. } => {}
                _ => {
                    errors_occurred += 1;
                    error!("Download failed: {e:?}");
                }
            }
        }
    }

    if errors_occurred == 0 {
        Ok(())
    } else {
        Err(SyncError::FailedDownloads {
            count: errors_occurred,
        })
    }
}

/// Get the rustup file downloads, in pairs of URLs and sha256 hashes.
pub fn rustup_download_list(
    path: &Path,
    platforms: &Platforms,
) -> Result<(String, Vec<(String, String)>), SyncError> {
    let channel_str = fs::read_to_string(path).map_err(DownloadError::Io)?;
    let channel: Channel = toml::from_str(&channel_str)?;

    Ok((
        channel.date,
        channel
            .pkg
            .into_iter()
            .filter(|(pkg_name, _)| pkg_name != "rustc-dev")
            .flat_map(|(_, pkg)| {
                pkg.target
                    .into_iter()
                    .filter(
                        |(name, _)| platforms.contains(name) || name == "*", // The * platform contains rust-src, always download
                    )
                    .filter_map(|(_, target)| {
                        target.target_urls.map(|urls| {
                            (
                                urls.xz_url.split('/').collect::<Vec<&str>>()[3..].join("/"),
                                urls.xz_hash,
                            )
                        })
                    })
            })
            .collect(),
    ))
}

pub async fn sync_one_rustup_target(
    client: &Client,
    path: &Path,
    source: &str,
    url: &str,
    hash: &str,
    retries: usize,
    user_agent: &HeaderValue,
) -> Result<(), DownloadError> {
    // Chop off the source portion of the URL, to mimic the rest of the path
    //let target_url = path.join(url[source.len()..].trim_start_matches("/"));
    let target_url = format!("{source}/{url}");
    let target_path: PathBuf = std::iter::once(path.to_owned())
        .chain(url.split('/').map(PathBuf::from))
        .collect();

    download(
        client,
        &target_url,
        &target_path,
        Some(hash),
        retries,
        false,
        user_agent,
    )
    .await
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelHistoryFile {
    pub versions: HashMap<String, Vec<String>>,
}

pub fn get_channel_history(path: &Path, channel: &str) -> Result<ChannelHistoryFile, SyncError> {
    let channel_history_path = path.join(format!("mirror-{channel}-history.toml"));
    let ch_data = fs::read_to_string(channel_history_path)?;
    Ok(toml::from_str(&ch_data)?)
}

pub fn add_to_channel_history(
    path: &Path,
    channel: &str,
    date: &str,
    files: &[(String, String)],
    extra_files: &[String],
) -> Result<(), SyncError> {
    let mut channel_history = match get_channel_history(path, channel) {
        Ok(c) => c,
        Err(SyncError::Io(_)) => ChannelHistoryFile {
            versions: HashMap::new(),
        },
        Err(e) => return Err(e.into()),
    };

    let files = files.iter().map(|(f, _)| f.to_string());
    let extra_files = extra_files.iter().map(|ef| ef.to_string());

    let files = files.chain(extra_files).collect();

    channel_history.versions.insert(date.to_string(), files);

    let ch_data = toml::to_string_pretty(&channel_history)?;

    let channel_history_path = path.join(format!("mirror-{channel}-history.toml"));
    write_file_create_dir(&channel_history_path, &ch_data)?;

    Ok(())
}

/// Get the current rustup version from release-stable.toml.
pub fn get_rustup_version(path: &Path) -> Result<String, SyncError> {
    let release_data: Release = toml::from_str(&fs::read_to_string(path)?)?;
    Ok(release_data.version)
}

pub async fn sync_rustup_channel(
    path: &Path,
    pack_args: &PackArgs,
    channel: &str,
    user_agent: &HeaderValue,
    platforms: &Platforms,
) -> Result<(), SyncError> {
    info!("Downloading rustup channe {} ...", channel);
    // Download channel file
    let (channel_url, channel_path, extra_files) =
        if let Some(inner_channel) = channel.strip_prefix("nightly-") {
            let url = format!(
                "{}/dist/{inner_channel}/channel-rust-nightly.toml",
                pack_args.source
            );
            let path_chunk = format!("dist/{inner_channel}/channel-rust-nightly.toml");
            let path = path.join(&path_chunk);
            // Make sure the cleanup step doesn't delete the channel toml
            let extra_files = vec![path_chunk.clone(), format!("{path_chunk}.sha256")];
            (url, path, extra_files)
        } else {
            let url = format!("{}/dist/channel-rust-{channel}.toml", pack_args.source);
            let path = path.join(format!("dist/channel-rust-{channel}.toml"));
            (url, path, Vec::new())
        };
    let channel_part_path = append_to_path(&channel_path, ".part");
    let client = Client::new();
    download_with_sha256_file(
        &client,
        &channel_url,
        &channel_part_path,
        pack_args.retries,
        true,
        user_agent,
    )
    .await?;

    // Open toml file, find all files to download
    let (date, files) = rustup_download_list(&channel_part_path, platforms)?;
    move_if_exists_with_sha256(&channel_part_path, &channel_path)?;

    let pb = registry_progress_bar(files.len());
    pb.enable_steady_tick(Duration::from_millis(10));

    let mut errors_occurred = 0usize;

    let tasks = futures::stream::iter(files.iter())
        .map(|(url, hash)| {
            // Clone the variables that will be moved into the tokio task.
            let client = client.clone();
            let path = path.to_path_buf();
            let source = pack_args.source.to_string();
            let retries = pack_args.retries;
            let user_agent = user_agent.clone();
            let url = url.clone();
            let hash = hash.clone();
            let pb = pb.clone();

            tokio::spawn(async move {
                let out = sync_one_rustup_target(
                    &client,
                    &path,
                    &source,
                    &url,
                    &hash,
                    retries,
                    &user_agent,
                )
                .await;

                pb.inc(1);

                out
            })
        })
        .buffer_unordered(pack_args.threads)
        .collect::<Vec<_>>()
        .await;

    for res in tasks {
        // Unwrap the join result.
        let res = res.unwrap();

        if let Err(e) = res {
            match e {
                DownloadError::NotFound { .. } => {}
                _ => {
                    errors_occurred += 1;
                    error!("Download failed: {e:?}");
                }
            }
        }
    }

    if errors_occurred == 0 {
        // Write channel history file
        add_to_channel_history(path, channel, &date, &files, &extra_files)?;
        Ok(())
    } else {
        Err(SyncError::FailedDownloads {
            count: errors_occurred,
        })
    }
}

pub async fn download_pinned_rust_version(
    root_registry: &Path,
    pack_args: &PackArgs,
) -> Result<()> {
    let platforms = get_platforms(&pack_args).await?;
    let user_agent =
        HeaderValue::from_str(&format!("Offline Mirror/{}", env!("CARGO_PKG_VERSION")))?;
    info!(
        "Downloading rust `{}` installations for [{}] platforms ({})",
        &pack_args.rust_versions.join(","),
        platforms.len(),
        &platforms.into_iter().join(", ")
    );

    // Mirror rustup-init
    if let Err(e) = sync_rustup_init(root_registry, pack_args, &user_agent, &platforms).await {
        error!("Downloading rustup init files failed: {e:?}");
        error!("You will need to sync again to finish this download.");
    }

    for rust_version in &pack_args.rust_versions {
        // Mirror pinned rust versions
        if let Err(e) = sync_rustup_channel(
            root_registry,
            pack_args,
            &rust_version,
            &user_agent,
            &platforms,
        )
        .await
        {
            if let SyncError::Download(DownloadError::NotFound { .. }) = e {
                error!("{} Pinned rust version could not be found.", rust_version);
                return Err(anyhow!(
                    "Pinned rust version {rust_version} could not be found"
                ));
            } else {
                error!("Downloading pinned rust {rust_version} failed: {e:?}");
                error!("You will need to sync again to finish this download.");
            }
        }
    }

    Ok(())
}

pub async fn download_latest(root_registry: &Path, pack_args: &PackArgs) -> Result<()> {
    let platforms = get_platforms(&pack_args).await?;
    let user_agent =
        HeaderValue::from_str(&format!("Offline Mirror/{}", env!("CARGO_PKG_VERSION")))?;

    info!(
        "Downloading the latest rust installations of stable and nightly for [{}] platforms ({})",
        platforms.len(),
        &platforms.into_iter().join(", ")
    );

    // Mirror rustup-init
    if let Err(e) = sync_rustup_init(root_registry, pack_args, &user_agent, &platforms).await {
        error!("Downloading rustup init files failed: {e:?}");
        error!("You will need to sync again to finish this download.");
    }

    info!("Download latest stable");
    // Mirror stable
    if let Err(e) =
        sync_rustup_channel(root_registry, pack_args, "stable", &user_agent, &platforms).await
    {
        error!("Downloading stable release failed: {e:?}");
        warn!("You will need to sync again to finish this download.");
    }

    info!("Download latest nightly");
    // Mirror nightly
    if let Err(e) =
        sync_rustup_channel(root_registry, pack_args, "nightly", &user_agent, &platforms).await
    {
        error!("Downloading nightly release failed: {e:?}");
        warn!("You will need to sync again to finish this download.");
    }

    info!("Syncing Rustup repositories complete!");
    Ok(())
}

use anyhow::{anyhow, Result};
use bytes::Bytes;
use glob::glob;
use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use toml::Table;
use tracing::error;
use warp::hyper::Body;
use warp::path::Tail;
use warp::reply::Response;
use warp::Filter;

use crate::serve::ServerError;
use crate::unpack;

static FRONTEND: Dir<'_> = include_dir!("$OUT_DIR/frontend_dist_folder/");

fn available_platforms(root: &Path) -> Result<Vec<String>> {
    Ok(std::fs::read_dir(root.join("rustup").join("dist"))?
        .map(|entry| {
            let platform_folder = entry?;
            Ok(platform_folder.file_name().to_str().unwrap().to_owned())
        })
        .collect::<Result<Vec<_>>>()?)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
struct Versions {
    // Channel name -> Platforms
    versions: HashMap<String, Vec<String>>,
}

fn load_config(path: &Path) -> Result<Table> {
    let content = std::fs::read_to_string(path)?;
    Ok(content.parse::<Table>()?)
}

/// Extract available platforms by mirror history file.
fn extract_available_platforms_for_channel(
    config: &Table,
    version_name: &str,
) -> Option<Vec<String>> {
    Some(
        config
            .get("versions")?
            .as_table()?
            .values()
            .flat_map(|v| v.as_array().map(Clone::clone).unwrap_or_default())
            .filter_map(|p| {
                let p = p.as_str()?;
                let prefix = format!("cargo-{}-", version_name);
                let start_index = p.find(&prefix)? + prefix.len();
                Some(p[start_index..].strip_suffix("tar.xz")?.to_owned())
            })
            .collect::<Vec<String>>(),
    )
}

fn available_versions(root: &Path) -> Result<Versions> {
    let versions = glob(root.join("*.toml").to_str().unwrap())?
        .map(|conf_path| -> Result<_> {
            let conf_path: PathBuf = conf_path?;
            let conf_file = load_config(&conf_path)?;
            let file_name = conf_path.file_name().unwrap().to_str().unwrap();
            let is_nightly = file_name.contains("nightly");
            let version_name = if is_nightly {
                "nightly"
            } else {
                file_name
                    .strip_prefix("mirror-")
                    .ok_or(anyhow!("strip_prefix NoneError"))?
                    .strip_suffix("-history.toml")
                    .ok_or(anyhow!("strip_suffix NoneError"))?
            };
            let platforms: Vec<String> =
                extract_available_platforms_for_channel(&conf_file, &version_name)
                    .ok_or(anyhow!("None Error channel config"))?;
            let version_name = if is_nightly {
                let date = file_name
                    .strip_prefix("mirror-nightly-")
                    .ok_or(anyhow!("strip_prefix NoneError"))?
                    .strip_suffix("-history.toml")
                    .ok_or(anyhow!("strip_suffix NoneError"))?;
                format!("{}-{}", version_name, date)
            } else {
                version_name.to_owned()
            };

            Ok((version_name, platforms))
        })
        .collect::<Result<HashMap<String, Vec<String>>>>()?;
    Ok(Versions { versions })
}

async fn frontend_api(
    root: &Path,
) -> impl warp::Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let path_for_platforms = root.to_path_buf();
    let available_platforms = warp::get()
        .and(warp::path("api"))
        .and(warp::path("available-platforms"))
        .and_then(move || {
            let path_for_api = path_for_platforms.clone();
            async move {
                let res = available_platforms(&path_for_api)
                    .map_err(|e| warp::reject::custom(ServerError(e)))
                    .map(|platforms| warp::reply::json(&platforms));
                res
            }
        });

    let path_for_versions = root.to_path_buf();
    let versions_for_channel = warp::get()
        .and(warp::path("api"))
        .and(warp::path("versions"))
        .and_then(move || {
            let path_for_version = path_for_versions.clone();
            async move {
                available_versions(&path_for_version)
                    .map_err(|e| warp::reject::custom(ServerError(e)))
                    .map(|versions| warp::reply::json(&versions))
            }
        });
    let path_for_loading = root.to_path_buf();
    let load_pack_file = warp::put()
        .and(warp::path("api"))
        .and(warp::path("load-pack-file"))
        .and(warp::body::bytes())
        .and(warp::header::optional::<String>("Content-Type"))
        .and_then(move |data: Bytes, content_type: Option<String>| {
            // FIXME() - Stream the body to file without load the whole file in the memory.
            let path_for_loading = path_for_loading.clone();
            async move {
                if !matches!(content_type, Some(file_type) if file_type == "application/x-tar") {
                    error!("Invalid content type. support only tar files (application/x-tar)");
                    return Err(warp::reject::custom(ServerError(anyhow!(
                        "Invalid content type. support only tar files (application/x-tar)"
                    ))));
                }

                let tmp = NamedTempFile::new()
                    .map_err(|e| warp::reject::custom(ServerError(anyhow!(e))))?;
                tokio::fs::write(tmp.path(), data).await.map_err(|e| {
                    error!("error writing file: {}", e);
                    warp::reject::reject()
                })?;
                unpack(tmp.path(), &path_for_loading)
                    .await
                    .map_err(|e| warp::reject::custom(ServerError(anyhow!(e))))?;
                Ok(warp::reply())
            }
        });

    available_platforms
        .or(versions_for_channel)
        .or(load_pack_file)
}

pub async fn serve_frontend(
    root: &Path,
) -> impl warp::Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let home_page = warp::get().and(warp::path::end()).and_then(|| async {
        FRONTEND
            .get_file("index.html")
            .ok_or_else(warp::reject::not_found)
            .map(|f| warp::reply::html(f.contents()))
    });

    let static_files = warp::get()
        .and(warp::path::tail())
        .and_then(|path: Tail| async move {
            FRONTEND
                .get_file(path.as_str())
                .ok_or_else(warp::reject::not_found)
                .map(|f| Response::new(Body::from(f.contents())))
        });

    let api = frontend_api(&root).await;
    home_page.or(api).or(static_files)
}

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Error;
use anyhow::Result;

use itertools::Itertools;
use serde::Deserialize;
use serde::Serialize;
use tracing::info;

use warp::http::StatusCode;
use warp::http::Uri;
use warp::Filter;
use warp::Rejection;
use warp::reject::Reject;

use crate::index::handle_git;
use crate::index::Index;
use crate::publish::crate_file_name;
use crate::publish::crate_path;
use crate::publish::publish_crate;
use crate::serve_frontend;

#[derive(Debug)]
pub(crate) struct ServerError(pub(crate) anyhow::Error);

impl Reject for ServerError {}

/// A single error that the registry returns.
#[derive(Debug, Default, Deserialize, Serialize)]
struct RegistryError {
    detail: String,
}

/// A list of errors that the registry returns in its response.
#[derive(Debug, Default, Deserialize, Serialize)]
struct RegistryErrors {
    errors: Vec<RegistryError>,
}

impl From<Error> for RegistryErrors {
    fn from(error: Error) -> Self {
        Self {
            errors: error
                .chain()
                .map(ToString::to_string)
                .map(|err| RegistryError { detail: err })
                .collect(),
        }
    }
}

/// Convert a result back into a response.
fn response<T>(result: Result<T>) -> Result<impl warp::Reply, warp::Rejection>
where
    T: warp::Reply,
{
    match result {
        Ok(inner) => {
            info!("request status: success");
            Ok(warp::reply::with_status(inner.into_response(), StatusCode::OK))
        }
        Err(err) => {
            Err(warp::reject::custom(ServerError(err)))
        }
    }
    // // Registries always respond with OK and use the JSON error array to
    // // indicate problems.
    // let reply = warp::reply::with_status(response, StatusCode::OK);
    // Ok(reply)
}

/// Serve a registry at the given path on the given socket address.
pub async fn serve(root: &Path, binding_addr: SocketAddr, server_addr: SocketAddr) -> Result<()> {
    let frontend = serve_frontend(root).await;
    let crates_folder = Arc::new(root.join("crates"));
    let index_folder = root.join("index");
    let git_index = Arc::new(
        Index::new(&index_folder, &server_addr)
            .await
            .with_context(|| {
                format!(
                    "failed to create/instantiate crate index at {}",
                    index_folder.display()
                )
            })?,
    );

    let path_for_git = index_folder.to_path_buf();
    // Serve git client requests to /git/index
    let index = warp::path("git")
        .and(warp::path("index"))
        .and(warp::path::tail())
        .and(warp::method())
        .and(warp::header::optional::<String>("Content-Type"))
        .and(warp::addr::remote())
        .and(warp::body::stream())
        .and(warp::query::raw().or_else(|_| async { Ok::<(String,), Rejection>((String::new(),)) }))
        .and_then(
            move |path_tail, method, content_type, remote, body, query| {
                let mirror_path = path_for_git.clone();
                async move {
                    response(
                        handle_git(
                            mirror_path,
                            path_tail,
                            method,
                            content_type,
                            remote,
                            body,
                            query,
                        )
                        .await,
                    )
                }
            },
        );
    // Handle sparse index requests at /index/
    // let sparse_index = warp::path("index").and(warp::fs::dir(index_folder.clone()));

    // Serve the contents of <root>/ at /crates. This allows for directly
    // downloading the .crate files, to which we redirect from the
    // download handler below.
    let crates = warp::path("crates")
        .and(warp::fs::dir(crates_folder.to_path_buf()))
        .with(warp::trace::request());
    let download = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("crates"))
        .and(warp::path::param())
        .and(warp::path::param())
        .and(warp::path("download"))
        .map(move |name: String, version: String| {
            let crate_path = crate_path(&name).join(crate_file_name(&name, &version));
            let path = format!(
                "/crates/{}",
                crate_path
                    .components()
                    .map(|c| format!("{}", c.as_os_str().to_str().unwrap()))
                    .join("/")
            );

            // TODO: Ideally we shouldn't unwrap here. That's not that easily
            //       possible, though, because then we'd need to handle errors
            //       and we can't use the response function because it will
            //       overwrite the HTTP status even on success.
            path.parse::<Uri>().map(warp::redirect).unwrap()
        })
        .with(warp::trace::request());
    let publish = warp::put()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("crates"))
        .and(warp::path("new"))
        .and(warp::path::end())
        .and(warp::body::bytes())
        // We cap total body size to 20 MiB to have some upper bound. At the
        // time of last check, crates.io employed a limit of 10 MiB.
        .and(warp::body::content_length_limit(20 * 1024 * 1024))
        .and_then(move |body| {
            let index = git_index.clone();
            let crates_folder = crates_folder.clone();
            async move {
                response(
                    publish_crate(body, index, crates_folder.as_path())
                        .await
                        .map(|()| String::new()),
                )
            }
        })
        .with(warp::trace::request());

    // For Rust installation
    let dist_dir = warp::path::path("dist").and(warp::fs::dir(root.join("dist")));
    let rustup_dir = warp::path::path("rustup").and(warp::fs::dir(root.join("rustup")));

    let routes = frontend
        .or(crates)
        .or(download)
        .or(publish)
        .or(dist_dir)
        .or(rustup_dir)
        // .or(sparse_index)
        .or(index);
    // Despite the claim that this function "Returns [...] a Future that
    // can be executed on any runtime." not even the call itself can
    // happen outside of a tokio runtime. Boy.
    warp::serve(routes).run(binding_addr).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::to_string;

    #[test]
    fn registry_error_encoding() {
        let expected = r#"{"errors":[{"detail":"error message text"}]}"#;
        let errors = RegistryErrors {
            errors: vec![RegistryError {
                detail: "error message text".to_string(),
            }],
        };

        assert_eq!(to_string(&errors).unwrap(), expected);
    }
}

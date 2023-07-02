use anyhow::ensure;
use anyhow::Context as _;
use anyhow::Result;
use bytes::BytesMut;
use futures::Stream;
use futures::StreamExt;
use itertools::process_results;
use itertools::Itertools;
use serde_json::from_str;
use serde_json::to_string;
use smolset::SmolSet;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs::create_dir_all;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::ChildStdout;
use tokio::process::Command;
use tracing::warn;
use warp::hyper::body::Sender;
use warp::hyper::Body;

use git2::{Config as GitConfig, Repository, Signature};

use serde::Deserialize;
use serde::Serialize;
use serde_json::from_reader;
use serde_json::to_writer_pretty;
use tokio::sync::Mutex;
use warp::http;
use warp::path::Tail;

#[derive(Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct Dep {
    /// Name of the dependency. If the dependency is renamed from the
    /// original package name, this is the new name. The original package
    /// name is stored in the `package` field.
    pub name: String,
    /// The semver requirement for this dependency.
    /// This must be a valid version requirement defined at
    /// https://github.com/steveklabnik/semver#requirements.
    pub req: String,
    /// Array of features (as strings) enabled for this dependency.
    pub features: Vec<String>,
    /// Boolean of whether or not this is an optional dependency.
    pub optional: bool,
    /// Boolean of whether or not default features are enabled.
    pub default_features: bool,
    /// The target platform for the dependency. null if not a target
    /// dependency. Otherwise, a string such as "cfg(windows)".
    pub target: Option<String>,
    /// The dependency kind.
    /// Note: this is a required field, but a small number of entries
    /// exist in the crates.io index with either a missing or null `kind`
    /// field due to implementation bugs.
    pub kind: Option<String>,
    /// The URL of the index of the registry where this dependency is from
    /// as a string. If not specified or null, it is assumed the
    /// dependency is in the current registry.
    pub registry: Option<String>,
    /// If the dependency is renamed, this is a string of the actual
    /// package name. If not specified or null, this dependency is not
    /// renamed.
    pub package: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct Entry {
    /// The name of the package.
    /// This must only contain alphanumeric, '-', or '_' characters.
    pub name: String,
    /// The version of the package this row is describing. This must be a
    /// valid version number according to the Semantic Versioning 2.0.0
    /// spec at https://semver.org/.
    pub vers: String,
    /// Array of direct dependencies of the package.
    pub deps: Vec<Dep>,
    /// A SHA-256 checksum of the '.crate' file.
    pub cksum: String,
    /// Set of features defined for the package. Each feature maps to an
    /// array of features or dependencies it enables.
    pub features: BTreeMap<String, Vec<String>>,
    /// Boolean of whether or not this version has been yanked.
    pub yanked: bool,
    /// The `links` string value from the package's manifest, or null if
    /// not specified. This field is optional and defaults to null.
    pub links: Option<String>,
}

pub(crate) struct Entries(SmolSet<[Entry; 10]>);

impl Deref for Entries {
    type Target = SmolSet<[Entry; 10]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Entries {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TryFrom<String> for Entries {
    type Error = serde_json::Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Ok(Self(
            value
                .lines()
                .map(|entry| from_str::<Entry>(entry))
                .collect::<Result<SmolSet<[Entry; 10]>, Self::Error>>()?,
        ))
    }
}

impl TryInto<String> for Entries {
    type Error = serde_json::Error;

    fn try_into(self) -> std::result::Result<String, Self::Error> {
        Ok(process_results(
            self.0.into_iter().map(|entry| to_string(&entry)),
            |mut ser_entries| ser_entries.join("\n"),
        )?)
    }
}

/// An object representing a config.json file inside the index.
#[derive(Debug, Default, Deserialize, Serialize)]
struct Config {
    dl: String,
    api: Option<String>,
}

/// A struct representing a crate index.
pub struct Index {
    /// The root directory of the index.
    root: PathBuf,
    /// The git repository inside the index.
    repository: Mutex<Repository>,
}

impl Index {
    // Create new index if there is already an index in the root the method just open it
    pub async fn new<P>(root: P, addr: &SocketAddr) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        let root: PathBuf = root.into();
        {
            let mut config = GitConfig::open_default()?;
            if let Err(err) = config.set_str("safe.directory", &format!("{}", root.display())) {
                warn!(
                    "Can't update the safe.directory in the gitconfig: error: {}",
                    err
                );
            }
        }

        let repository = match Repository::open(&root) {
            Ok(r) => r,
            Err(e) => {
                warn!(
                    "Can't open the git repository at {} try to init [{:?}]",
                    root.display(),
                    e
                );
                create_dir_all(&root)
                    .with_context(|| format!("failed to create directory {}", root.display()))?;
                Repository::init(&root).with_context(|| {
                    format!("failed to initialize git repository {}", root.display())
                })?
            }
        };

        let mut index = Index {
            root,
            repository: Mutex::new(repository),
        };
        index.ensure_has_commit().await?;
        index.ensure_config(addr).await?;
        index.update_server_info()?;

        Ok(index)
    }

    pub async fn add_and_commit(
        &self,
        files: impl IntoIterator<Item = impl AsRef<Path>>,
        message: &str,
    ) -> Result<()> {
        let repository = self.repository.lock().await;
        let refname = "HEAD";
        let signature = Signature::now("CrateRegistry", "crates@registry")?;

        let mut index = repository
            .index()
            .context("failed to retrieve git repository index")?;
        for file in files {
            let file: &Path = file.as_ref();
            let relative_path = if !file.is_relative() {
                file.strip_prefix(&self.root).with_context(|| {
                    format!(
                        "failed to make {} relative to {}",
                        file.display(),
                        self.root.display()
                    )
                })?
            } else {
                file
            };
            index
                .add_path(relative_path)
                .context("failed to add file to git index")?;
            index
                .write()
                .context("failed to write git repository index")?;
        }

        let tree_id = index
            .write_tree()
            .context("failed to write git repository index tree")?;
        let tree = repository
            .find_tree(tree_id)
            .context("failed to find tree object in git repository")?;

        let empty = repository
            .is_empty()
            .context("unable to check git repository empty status")?;

        if empty {
            repository.commit(Some(refname), &signature, &signature, message, &tree, &[])
        } else {
            let oid = repository
                .refname_to_id(refname)
                .context(format!("failed to map {refname} to git id"))?;
            let parent = repository
                .find_commit(oid)
                .context(format!("failed to find {refname} commit"))?;

            repository.commit(
                Some(refname),
                &signature,
                &signature,
                message,
                &tree,
                &[&parent],
            )
        }
        .context("failed to create git commit")?;

        self.update_server_info()?;
        Ok(())
    }

    /// Update information necessary for serving the repository in "dumb"
    /// mode.
    fn update_server_info(&self) -> Result<()> {
        // Neither the git2 crate nor libgit2 itself seem to provide similar
        // functionality, so we have to fall back to just running the
        // command.
        let status = std::process::Command::new("git")
            .current_dir(&self.root)
            .arg("update-server-info")
            .status()
            .context("failed to run git update-server-info")?;

        ensure!(status.success(), "git update-server-info failed");
        Ok(())
    }

    /// Ensure that an initial git commit exists.
    async fn ensure_has_commit(&mut self) -> Result<()> {
        let empty = self
            .repository
            .lock()
            .await
            .is_empty()
            .context("unable to check git repository empty status")?;

        if empty {
            self.add_and_commit(
                std::iter::empty::<PathBuf>(),
                "Create new repository for cargo registry",
            )
            .await
            .context("failed to create initial git commit")?;
        }
        Ok(())
    }

    /// Ensure that a valid `config.json` exists and that it is up-to-date.
    async fn ensure_config(&mut self, addr: &SocketAddr) -> Result<()> {
        let path = self.root.join("config.json");
        let result = OpenOptions::new().read(true).write(true).open(&path);
        match result {
            Ok(file) => {
                let mut config =
                    from_reader::<_, Config>(&file).context("failed to parse config.json")?;
                let dl = format!(
                    "http://{}/api/v1/crates/{{crate}}/{{version}}/download",
                    addr
                );
                let api = format!("http://{}", addr);
                if config.dl != dl || config.api.as_ref() != Some(&api) {
                    config.dl = dl;
                    config.api = Some(api);

                    let file = OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(&path)
                        .context("failed to reopen config.json")?;
                    to_writer_pretty(&file, &config).context("failed to update config.json")?;

                    self.add_and_commit(vec!["config.json"], "Update config.json")
                        .await
                        .context("failed to stage and commit config.json")?;
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let file = File::create(&path).context("failed to create config.json")?;
                let config = Config {
                    dl: format!(
                        "http://{}/api/v1/crates/{{crate}}/{{version}}/download",
                        addr
                    ),
                    api: Some(format!("http://{}", addr)),
                };
                to_writer_pretty(&file, &config).context("failed to write config.json")?;

                self.add_and_commit(vec!["config.json"], "Add initial config.json")
                    .await
                    .context("failed to stage and commit config.json")?;
            }
            Err(err) => return Err(err).context("failed to open/create config.json"),
        }
        Ok(())
    }

    /// Retrieve the path to the index' root directory.
    #[inline]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Handle a request from a git client.
pub async fn handle_git<S, B>(
    mirror_path: PathBuf,
    path_tail: Tail,
    method: http::Method,
    content_type: Option<String>,
    remote: Option<SocketAddr>,
    mut body: S,
    query: String,
) -> Result<http::Response<Body>>
where
    S: Stream<Item = Result<B, warp::Error>> + Send + Unpin + 'static,
    B: bytes::Buf + Sized,
{
    let remote = remote
        .map(|r| r.ip().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());

    // Run "git http-backend"
    let mut cmd = Command::new("git");
    cmd.arg("http-backend");

    // Clear environment variables, and set needed variables
    // See: https://git-scm.com/docs/git-http-backend
    cmd.env_clear();
    cmd.env("GIT_PROJECT_ROOT", mirror_path);
    cmd.env("PATH_INFO", format!("/{}", path_tail.as_str()));

    cmd.env("REQUEST_METHOD", method.as_str());
    cmd.env("QUERY_STRING", query);
    cmd.env("REMOTE_USER", "");
    cmd.env("REMOTE_ADDR", remote);
    if let Some(content_type) = content_type {
        cmd.env("CONTENT_TYPE", content_type);
    }
    cmd.env("GIT_HTTP_EXPORT_ALL", "true");
    cmd.stderr(Stdio::inherit());
    cmd.stdout(Stdio::piped());
    cmd.stdin(Stdio::piped());

    let p = cmd.spawn()?;

    // Handle sending git client body to http-backend, if any
    let mut git_input = p.stdin.expect("Process should always have stdin");
    while let Some(Ok(mut buf)) = body.next().await {
        git_input.write_all_buf(&mut buf).await?;
    }

    // Collect headers from git CGI output
    let mut git_output = BufReader::new(p.stdout.expect("Process should always have stdout"));
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        git_output.read_line(&mut line).await?;

        let line = line.trim_end();
        if line.is_empty() {
            break;
        }

        if let Some((key, value)) = line.split_once(": ") {
            headers.insert(key.to_string(), value.to_string());
        }
    }

    // Add headers to response (except for Status, which is the "200 OK" line)
    let mut resp = http::Response::builder();
    for (key, val) in headers {
        if key == "Status" {
            resp = resp.status(&val.as_bytes()[..3]);
        } else {
            resp = resp.header(&key, val);
        }
    }

    // Create channel, so data can be streamed without being fully loaded
    // into memory. Requires a separate future to be spawned.
    let (sender, body) = Body::channel();
    tokio::spawn(send_git(sender, git_output));

    let resp = resp.body(body)?;
    Ok(resp)
}

/// Send data from git CGI process to hyper Sender, until there is no more
/// data left.
async fn send_git(
    mut sender: Sender,
    mut git_output: BufReader<ChildStdout>,
) -> Result<(), anyhow::Error> {
    loop {
        let mut bytes_out = BytesMut::new();
        git_output.read_buf(&mut bytes_out).await?;
        if bytes_out.is_empty() {
            return Ok(());
        }
        sender.send_data(bytes_out.freeze()).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write as _;
    use std::str::FromStr;

    use git2::RepositoryState;
    use git2::StatusOptions;
    use git2::StatusShow;

    use tempfile::tempdir;

    #[tokio::test]
    async fn empty_index_repository() {
        let root = tempdir().unwrap();
        let addr = SocketAddr::from_str("192.168.0.1:9999").unwrap();
        let index = Index::new(root.as_ref(), &addr).await.unwrap();
        let repository = index.repository.lock().await;
        assert_eq!(repository.state(), RepositoryState::Clean);
        assert!(repository.head().is_ok());

        let file = index.root.join("config.json");
        let config = File::open(file).unwrap();
        let config = from_reader::<_, Config>(&config).unwrap();

        assert_eq!(
            config.dl,
            "http://192.168.0.1:9999/api/v1/crates/{crate}/{version}/download"
        );
        assert_eq!(config.api, Some("http://192.168.0.1:9999".to_string()));
    }

    #[tokio::test]
    async fn prepopulated_index_repository() {
        let root = tempdir().unwrap();
        let mut file = File::create(root.as_ref().join("config.json")).unwrap();
        // We always assume some valid JSON in the config.
        file.write_all(br#"{"dl":"foobar"}"#).unwrap();

        let addr = SocketAddr::from_str("254.0.0.0:1").unwrap();
        let index = Index::new(root.as_ref(), &addr).await.unwrap();
        let repository = index.repository.lock().await;

        assert_eq!(repository.state(), RepositoryState::Clean);
        assert!(repository.head().is_ok());

        let file = index.root.join("config.json");
        let config = File::open(file).unwrap();
        let config = from_reader::<_, Config>(&config).unwrap();

        assert_eq!(
            config.dl,
            "http://254.0.0.0:1/api/v1/crates/{crate}/{version}/download"
        );
        assert_eq!(config.api, Some("http://254.0.0.0:1".to_string()));
    }

    /// Test that we can create an `Index` in the same registry directory
    /// multiple times without problems.
    #[tokio::test]
    async fn recreate_index() {
        let root = tempdir().unwrap();
        let addr = "127.0.0.1:0".parse().unwrap();

        {
            let _index = Index::new(root.path(), &addr).await.unwrap();
        }

        {
            let _index = Index::new(root.path(), &addr).await.unwrap();
        }
    }

    /// Check that the Git repository contained in our index has no
    /// untracked files.
    #[tokio::test]
    async fn no_untracked_files() {
        let root = tempdir().unwrap();
        let addr = "127.0.0.1:0".parse().unwrap();
        let index = Index::new(root.path(), &addr).await.unwrap();
        let repository = index.repository.lock().await;

        // The repository should be clean.
        assert_eq!(repository.state(), RepositoryState::Clean);

        let mut options = StatusOptions::new();
        options
            .show(StatusShow::IndexAndWorkdir)
            .include_untracked(true)
            .include_ignored(true)
            .include_unmodified(false);

        let statuses = repository.statuses(Some(&mut options)).unwrap();
        assert_eq!(statuses.len(), 0);
    }
}

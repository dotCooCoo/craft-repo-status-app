//! Everything Craft Status knows that isn't network or UI: the per-repo
//! statistics model, release/download accounting, the urgency heuristic for
//! open issues, the config file, the on-disk cache and window geometry.

pub mod cache;
pub mod config;
pub mod model;
pub mod releases;
pub mod urgency;

pub use cache::Cache;
pub use config::{Config, WindowState};
pub use model::{
    Activity, RepoEntry, RepoStats, Schedule, Snapshot, TokenStatus, UrgentIssue, ALL_TIME,
    SINCE_RELEASE, WINDOWS,
};
pub use releases::{summarize_releases, Asset, BuildLag, LagBasis, Release, ReleaseSummary};
pub use urgency::{rank_issues, score_issue, IssueInput, Scored};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no home directory")]
    NoHome,
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("config: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("config: {0}")]
    TomlEdit(#[from] toml_edit::TomlError),
    #[error("config: {0}")]
    TomlSer(#[from] toml::ser::Error),
    /// Values a save refused, one `field: problem` each.
    #[error("{}", .0.join("; "))]
    Invalid(Vec<String>),
    #[error("cache: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Write via a temp file + rename so readers never see a half-written file.
/// A symlink is followed, so the file it points at is replaced, and the new
/// file gets the permissions of the one it replaces.
pub fn atomic_write(path: &std::path::Path, text: &str) -> Result<()> {
    write_replacing(path, text, false)
}

/// As [`atomic_write`], for text that holds a secret: on Unix the file is
/// readable and writable by its owner only.
pub fn atomic_write_private(path: &std::path::Path, text: &str) -> Result<()> {
    write_replacing(path, text, true)
}

fn write_replacing(path: &std::path::Path, text: &str, private: bool) -> Result<()> {
    let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let io = |source| Error::Io {
        path: path.display().to_string(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = path.with_extension("tmp");
    // The temporary file is created with its final mode, before the text
    // goes in: 0600 when private, else the mode of the file it replaces.
    let replaced = std::fs::metadata(path).ok().map(|m| m.permissions());
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = if private {
            Some(0o600)
        } else {
            replaced.as_ref().map(|p| p.mode() & 0o777)
        };
        if let Some(m) = mode {
            options.mode(m);
        }
        mode
    };
    let mut file = options.open(&tmp).map_err(io)?;
    // The umask can narrow the mode a file is created with.
    #[cfg(unix)]
    if let Some(m) = mode {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(m))
            .map_err(io)?;
    }
    #[cfg(not(unix))]
    if let (false, Some(p)) = (private, replaced) {
        file.set_permissions(p).map_err(io)?;
    }
    std::io::Write::write_all(&mut file, text.as_bytes()).map_err(io)?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(io)
}

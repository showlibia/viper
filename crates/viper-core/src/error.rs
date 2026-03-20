use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("home directory is unavailable")]
    HomeUnavailable,
    #[error("target prefix is required: pass --prefix or --name")]
    MissingTargetPrefix,
    #[error("cannot set both --prefix and --name")]
    ConflictingTargetOptions,
    #[error("prefix '{0}' does not exist")]
    PrefixNotFound(String),
    #[error("prefix '{0}' is not a managed environment")]
    NotManagedPrefix(String),
    #[error("package specification is empty")]
    EmptySpec,
    #[error("unsupported environment file format")]
    UnsupportedEnvironmentFile,
    #[error("invalid environment file: {0}")]
    InvalidEnvironmentFile(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("invalid repodata: {0}")]
    InvalidRepodata(String),
    #[error("offline mode requires a cached repodata index (not implemented yet)")]
    OfflineRepodataUnavailable,
    #[error("config key '{0}' is not supported")]
    UnsupportedConfigKey(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

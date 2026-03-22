use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct CliGlobalOptions {
    pub root_prefix: Option<PathBuf>,
    pub prefix: Option<PathBuf>,
    pub name: Option<String>,
    pub channels: Vec<String>,
    pub json: bool,
    pub yes: bool,
    pub dry_run: bool,
    pub no_rc: bool,
    pub offline: bool,
    pub repodata_ttl: Option<usize>,
    pub verbose: u8,
}

#[derive(Debug, Clone)]
pub enum CliOperation {
    Create {
        specs: Vec<String>,
        files: Vec<PathBuf>,
    },
    Install {
        specs: Vec<String>,
        files: Vec<PathBuf>,
    },
    Remove {
        specs: Vec<String>,
        all: bool,
    },
    List(ListOptions),
    Info,
    Config(CliConfigCommand),
}

#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    pub regex: Option<String>,
    pub full_name: bool,
    pub no_pip: bool,
    pub reverse: bool,
    pub explicit: bool,
    pub md5: bool,
    pub sha256: bool,
    pub canonical: bool,
    pub export: bool,
    pub revisions: bool,
}

#[derive(Debug, Clone)]
pub enum CliConfigCommand {
    List,
    Get { key: String },
    Set { key: String, value: String },
}

#[derive(Debug, Clone)]
pub struct OperationRequest {
    pub globals: CliGlobalOptions,
    pub op: CliOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRecord {
    pub name: String,
    #[serde(default = "default_unknown")]
    pub version: String,
    #[serde(default = "default_build")]
    pub build_string: String,
    #[serde(default = "default_channel")]
    pub channel: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub url: String,
    pub spec: String,
    #[serde(default = "default_package_source")]
    pub source: String,
    #[serde(default)]
    pub depends: Vec<String>,
    pub installed_at: String,
    #[serde(default = "default_platform")]
    pub platform: String,
}

fn default_package_source() -> String {
    "conda".to_string()
}

fn default_unknown() -> String {
    "unknown".to_string()
}

fn default_build() -> String {
    "0".to_string()
}

fn default_channel() -> String {
    "conda-forge".to_string()
}

fn default_base_url() -> String {
    "https://conda.anaconda.org/conda-forge".to_string()
}

fn default_platform() -> String {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-64".to_string(),
        ("linux", "aarch64") => "linux-aarch64".to_string(),
        ("macos", "x86_64") => "osx-64".to_string(),
        ("macos", "aarch64") => "osx-arm64".to_string(),
        ("windows", "x86_64") => "win-64".to_string(),
        (os, arch) => format!("{os}-{arch}"),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationResult {
    pub success: bool,
    pub message: String,
    pub data: Value,
    pub warnings: Vec<String>,
}

impl OperationResult {
    pub fn ok(message: impl Into<String>, data: Value) -> Self {
        Self {
            success: true,
            message: message.into(),
            data,
            warnings: Vec::new(),
        }
    }

    pub fn fail(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: Value::Null,
            warnings: Vec::new(),
        }
    }
}

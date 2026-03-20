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
    pub verbose: u8,
}

#[derive(Debug, Clone)]
pub enum CliOperation {
    Create {
        specs: Vec<String>,
        file: Option<PathBuf>,
    },
    Install {
        specs: Vec<String>,
        file: Option<PathBuf>,
    },
    Remove {
        specs: Vec<String>,
        all: bool,
    },
    List,
    Info,
    Config(CliConfigCommand),
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
    pub spec: String,
    #[serde(default = "default_package_source")]
    pub source: String,
    pub installed_at: String,
}

fn default_package_source() -> String {
    "conda".to_string()
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

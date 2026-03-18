use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::spec::package_name_from_spec;
use crate::types::PackageRecord;

const STATE_FILE: &str = "viper-state.json";

#[derive(Debug, Serialize, Deserialize)]
pub struct EnvironmentState {
    pub packages: Vec<PackageRecord>,
}

impl EnvironmentState {
    pub fn empty() -> Self {
        Self {
            packages: Vec::new(),
        }
    }

    pub fn load(prefix: &Path) -> Result<Self, CoreError> {
        let path = state_path(prefix);
        if !path.exists() {
            return Ok(Self::empty());
        }
        let raw = fs::read_to_string(path)?;
        let state = serde_json::from_str(&raw)?;
        Ok(state)
    }

    pub fn save(&self, prefix: &Path) -> Result<(), CoreError> {
        let path = state_path(prefix);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(path, raw)?;
        Ok(())
    }

    pub fn install_specs(&mut self, specs: &[String]) -> Result<usize, CoreError> {
        let mut changed = 0usize;
        for spec in specs {
            let name = package_name_from_spec(spec)?;
            let now = Utc::now().to_rfc3339();

            if let Some(existing) = self.packages.iter_mut().find(|p| p.name == name) {
                existing.spec = spec.clone();
                existing.installed_at = now;
            } else {
                self.packages.push(PackageRecord {
                    name,
                    spec: spec.clone(),
                    installed_at: now,
                });
                changed += 1;
            }
        }
        self.packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(changed)
    }

    pub fn remove_specs(&mut self, specs: &[String]) -> Result<usize, CoreError> {
        let mut names = Vec::with_capacity(specs.len());
        for spec in specs {
            names.push(package_name_from_spec(spec)?);
        }

        let before = self.packages.len();
        self.packages
            .retain(|p| !names.iter().any(|n| n == &p.name));
        Ok(before.saturating_sub(self.packages.len()))
    }
}

pub fn state_path(prefix: &Path) -> PathBuf {
    prefix.join("conda-meta").join(STATE_FILE)
}

pub fn ensure_prefix_layout(prefix: &Path) -> Result<(), CoreError> {
    fs::create_dir_all(prefix.join("conda-meta"))?;
    fs::create_dir_all(prefix.join("pkgs"))?;
    fs::create_dir_all(prefix.join("bin"))?;
    Ok(())
}

pub fn is_managed_prefix(prefix: &Path) -> bool {
    prefix.join("conda-meta").exists()
}

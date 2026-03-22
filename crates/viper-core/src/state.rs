use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::error::CoreError;
use crate::spec::package_name_from_spec;
use crate::transaction::PlannedLink;
use crate::types::PackageRecord;

const HISTORY_FILE: &str = "history";

#[derive(Debug)]
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
        let meta_dir = conda_meta_dir(prefix);
        if !meta_dir.exists() {
            return Ok(Self::empty());
        }

        let mut packages = Vec::new();
        for entry in fs::read_dir(&meta_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.file_name().and_then(|v| v.to_str()) == Some(HISTORY_FILE) {
                continue;
            }
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read_to_string(&path)?;
            let record: PackageRecord = serde_json::from_str(&raw)?;
            packages.push(record);
        }

        packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { packages })
    }

    pub fn persist(&self, prefix: &Path) -> Result<(), CoreError> {
        let meta_dir = conda_meta_dir(prefix);
        fs::create_dir_all(&meta_dir)?;

        for entry in fs::read_dir(&meta_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.file_name().and_then(|v| v.to_str()) == Some(HISTORY_FILE) {
                continue;
            }
            if path.extension().and_then(|v| v.to_str()) == Some("json") {
                fs::remove_file(path)?;
            }
        }

        for record in &self.packages {
            let file_name = package_record_filename(record);
            let raw = serde_json::to_string_pretty(record)?;
            fs::write(meta_dir.join(file_name), raw)?;
        }
        Ok(())
    }

    pub fn install_conda_links(
        &mut self,
        links: &[PlannedLink],
        requested_specs: &[String],
        platform: &str,
    ) -> Result<usize, CoreError> {
        let mut changed = 0usize;
        let mut requested_by_name = HashMap::new();
        for spec in requested_specs {
            if let Ok(name) = package_name_from_spec(spec) {
                requested_by_name.insert(name, spec.clone());
            }
        }

        for link in links {
            let now = Utc::now().to_rfc3339();
            let fallback_spec = format!("{}={}", link.name, link.version);
            let spec = requested_by_name
                .get(&link.name)
                .cloned()
                .unwrap_or(fallback_spec);

            let base_url = link
                .url
                .rsplit_once('/')
                .and_then(|(prefix, _)| prefix.rsplit_once('/').map(|(base, _)| base.to_string()))
                .unwrap_or_else(|| link.channel.clone());

            if let Some(existing) = self
                .packages
                .iter_mut()
                .find(|p| p.name == link.name && p.source == "conda")
            {
                let was_same = existing.version == link.version
                    && existing.build_string == link.build
                    && existing.channel == link.channel;
                existing.version = link.version.clone();
                existing.build_string = link.build.clone();
                existing.channel = link.channel.clone();
                existing.base_url = base_url;
                existing.url = link.url.clone();
                existing.spec = spec;
                existing.source = "conda".to_string();
                existing.depends = Vec::new();
                existing.platform = platform.to_string();
                existing.installed_at = now;
                if !was_same {
                    changed += 1;
                }
            } else {
                self.packages.push(PackageRecord {
                    name: link.name.clone(),
                    version: link.version.clone(),
                    build_string: link.build.clone(),
                    channel: link.channel.clone(),
                    base_url,
                    url: link.url.clone(),
                    spec,
                    source: "conda".to_string(),
                    depends: Vec::new(),
                    installed_at: now,
                    platform: platform.to_string(),
                });
                changed += 1;
            }
        }

        self.packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(changed)
    }

    pub fn install_pip_specs(
        &mut self,
        specs: &[String],
        platform: &str,
    ) -> Result<usize, CoreError> {
        let mut changed = 0usize;
        for spec in specs {
            let name = package_name_from_spec(spec)?;
            let now = Utc::now().to_rfc3339();
            let parsed_version = spec
                .split_once("==")
                .map(|(_, v)| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "unknown".to_string());

            if let Some(existing) = self
                .packages
                .iter_mut()
                .find(|p| p.name == name && p.source == "pip")
            {
                existing.spec = spec.clone();
                existing.version = parsed_version;
                existing.build_string = "pypi_0".to_string();
                existing.channel = "pypi".to_string();
                existing.base_url = "https://pypi.org".to_string();
                existing.url = format!("https://pypi.org/project/{name}/");
                existing.installed_at = now;
                existing.platform = platform.to_string();
            } else {
                self.packages.push(PackageRecord {
                    name: name.clone(),
                    version: parsed_version,
                    build_string: "pypi_0".to_string(),
                    channel: "pypi".to_string(),
                    base_url: "https://pypi.org".to_string(),
                    url: format!("https://pypi.org/project/{name}/"),
                    spec: spec.clone(),
                    source: "pip".to_string(),
                    depends: Vec::new(),
                    installed_at: now,
                    platform: platform.to_string(),
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

    pub fn conda_locked_specs(&self) -> Vec<String> {
        self.packages
            .iter()
            .filter(|pkg| pkg.source == "conda")
            .map(|pkg| format!("{}>={}", pkg.name, pkg.version))
            .collect()
    }

    pub fn append_history(
        prefix: &Path,
        operation: &str,
        conda_links: &[PlannedLink],
        removed: &[String],
    ) -> Result<(), CoreError> {
        let history_path = history_path(prefix);
        if let Some(parent) = history_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut block = String::new();
        block.push_str(&format!("==> {} <==\n", Utc::now().to_rfc3339()));
        block.push_str(&format!("operation: {operation}\n"));
        for link in conda_links {
            block.push_str(&format!(
                "+ {}-{}-{}\n",
                link.name, link.version, link.build
            ));
        }
        for name in removed {
            block.push_str(&format!("- {name}\n"));
        }
        block.push('\n');

        let mut existing = fs::read_to_string(&history_path).unwrap_or_default();
        existing.push_str(&block);
        fs::write(history_path, existing)?;
        Ok(())
    }

    pub fn revisions(prefix: &Path) -> Result<Vec<String>, CoreError> {
        let history = fs::read_to_string(history_path(prefix))?;
        let revisions = history
            .lines()
            .filter(|line| line.starts_with("==> ") && line.ends_with(" <=="))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        Ok(revisions)
    }
}

fn package_record_filename(record: &PackageRecord) -> String {
    if record.source == "conda" {
        return format!(
            "{}-{}-{}.json",
            record.name, record.version, record.build_string
        );
    }
    format!("pypi-{}.json", record.name)
}

fn conda_meta_dir(prefix: &Path) -> PathBuf {
    prefix.join("conda-meta")
}

fn history_path(prefix: &Path) -> PathBuf {
    conda_meta_dir(prefix).join(HISTORY_FILE)
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

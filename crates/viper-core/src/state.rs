use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use serde::Deserialize;

use crate::error::CoreError;
use crate::spec::{package_name_from_spec, parse_explicit_url};
use crate::transaction::{PlannedLink, PlannedUnlink};
use crate::types::PackageRecord;

const HISTORY_FILE: &str = "history";

#[derive(Debug, Clone)]
pub struct EnvironmentState {
    pub packages: Vec<PackageRecord>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RevisionRecord {
    pub rev: usize,
    pub date: String,
    pub install: Vec<String>,
    pub remove: Vec<String>,
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

    pub fn load_with_pip_discovery(
        prefix: &Path,
        include_pip_site_packages: bool,
    ) -> Result<Self, CoreError> {
        let mut state = Self::load(prefix)?;
        if include_pip_site_packages {
            state.merge_pip_site_packages(prefix)?;
        }
        Ok(state)
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

            let base_url = if link.base_url.is_empty() {
                link.url
                    .rsplit_once('/')
                    .and_then(|(prefix, _)| {
                        prefix.rsplit_once('/').map(|(base, _)| base.to_string())
                    })
                    .unwrap_or_else(|| link.channel.clone())
            } else {
                link.base_url.clone()
            };

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
                existing.md5 = link.md5.clone();
                existing.sha256 = link.sha256.clone();
                existing.build_number = link.build_number;
                existing.dist_name = if link.dist_name.is_empty() {
                    format!("{}-{}-{}", link.name, link.version, link.build)
                } else {
                    link.dist_name.clone()
                };
                existing.spec = spec;
                existing.source = "conda".to_string();
                existing.depends = link.depends.clone();
                existing.platform = if link.platform.is_empty() {
                    platform.to_string()
                } else {
                    link.platform.clone()
                };
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
                    md5: link.md5.clone(),
                    sha256: link.sha256.clone(),
                    build_number: link.build_number,
                    dist_name: if link.dist_name.is_empty() {
                        format!("{}-{}-{}", link.name, link.version, link.build)
                    } else {
                        link.dist_name.clone()
                    },
                    spec,
                    source: "conda".to_string(),
                    depends: link.depends.clone(),
                    installed_at: now,
                    platform: if link.platform.is_empty() {
                        platform.to_string()
                    } else {
                        link.platform.clone()
                    },
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
                existing.md5 = None;
                existing.sha256 = None;
                existing.build_number = 0;
                existing.dist_name = spec.clone();
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
                    md5: None,
                    sha256: None,
                    build_number: 0,
                    dist_name: spec.clone(),
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

    pub fn remove_specs(
        &mut self,
        specs: &[String],
        prune_dependencies: bool,
        keep_requested: &HashSet<String>,
    ) -> Result<Vec<PlannedUnlink>, CoreError> {
        let mut names = HashSet::with_capacity(specs.len());
        for spec in specs {
            names.insert(package_name_from_spec(spec)?);
        }

        for name in &names {
            let exists = self.packages.iter().any(|pkg| pkg.name == *name);
            if !exists {
                return Err(CoreError::PackageNotInstalled(name.clone()));
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for pkg in &self.packages {
                if names.contains(&pkg.name) {
                    continue;
                }
                let depends_on_removed = pkg
                    .depends
                    .iter()
                    .filter_map(|dep| package_name_from_spec(dep).ok())
                    .any(|dep_name| names.contains(&dep_name));
                if depends_on_removed && names.insert(pkg.name.clone()) {
                    changed = true;
                }
            }
        }

        if prune_dependencies {
            let mut prune_queue = Vec::new();
            for pkg in &self.packages {
                if !names.contains(&pkg.name) {
                    continue;
                }
                for dep in pkg
                    .depends
                    .iter()
                    .filter_map(|dep| package_name_from_spec(dep).ok())
                {
                    prune_queue.push(dep);
                }
            }
            while let Some(candidate) = prune_queue.pop() {
                if names.contains(&candidate) {
                    continue;
                }
                if keep_requested.contains(&candidate) {
                    continue;
                }

                let still_required = self
                    .packages
                    .iter()
                    .filter(|pkg| !names.contains(&pkg.name))
                    .any(|pkg| {
                        pkg.depends
                            .iter()
                            .filter_map(|dep| package_name_from_spec(dep).ok())
                            .any(|dep| dep == candidate)
                    });
                if still_required {
                    continue;
                }

                if let Some(pkg) = self
                    .packages
                    .iter()
                    .find(|pkg| pkg.name == candidate && pkg.source == "conda")
                {
                    names.insert(pkg.name.clone());
                    for dep in pkg
                        .depends
                        .iter()
                        .filter_map(|dep| package_name_from_spec(dep).ok())
                    {
                        prune_queue.push(dep);
                    }
                }
            }
        }

        let mut removed = self
            .packages
            .iter()
            .filter(|pkg| names.contains(&pkg.name))
            .map(|pkg| PlannedUnlink {
                name: pkg.name.clone(),
                version: pkg.version.clone(),
                build: pkg.build_string.clone(),
                dist_name: pkg.dist_name.clone(),
                source: pkg.source.clone(),
            })
            .collect::<Vec<_>>();
        removed.sort_by(|a, b| a.name.cmp(&b.name));

        self.packages.retain(|p| !names.contains(&p.name));
        Ok(removed)
    }

    pub fn requested_specs_map(prefix: &Path) -> Result<HashMap<String, String>, CoreError> {
        let history = match fs::read_to_string(history_path(prefix)) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(CoreError::Io(err)),
        };
        let mut requested = HashMap::new();

        for line in history.lines() {
            let Some(comment) = line.strip_prefix("# ") else {
                continue;
            };
            let Some((action, raw_specs)) = comment.split_once(" specs: ") else {
                continue;
            };
            let specs = serde_json::from_str::<Vec<String>>(raw_specs).map_err(|err| {
                CoreError::InvalidEnvironmentFile(format!(
                    "invalid history specs entry for action '{}': {}",
                    action.trim(),
                    err
                ))
            })?;
            match action.trim() {
                "create" | "install" | "update" => {
                    for spec in specs {
                        if let Some(name) = requested_name_from_history_spec(&spec) {
                            requested.insert(name, spec);
                        }
                    }
                }
                "remove" | "uninstall" => {
                    for spec in specs {
                        if let Some(name) = requested_name_from_history_spec(&spec) {
                            requested.remove(&name);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(requested)
    }

    pub fn force_remove_specs(
        &mut self,
        specs: &[String],
    ) -> Result<Vec<PlannedUnlink>, CoreError> {
        let mut names = HashSet::with_capacity(specs.len());
        for spec in specs {
            names.insert(package_name_from_spec(spec)?);
        }
        for name in &names {
            let exists = self.packages.iter().any(|pkg| pkg.name == *name);
            if !exists {
                return Err(CoreError::PackageNotInstalled(name.clone()));
            }
        }

        let mut removed = self
            .packages
            .iter()
            .filter(|pkg| names.contains(&pkg.name))
            .map(|pkg| PlannedUnlink {
                name: pkg.name.clone(),
                version: pkg.version.clone(),
                build: pkg.build_string.clone(),
                dist_name: pkg.dist_name.clone(),
                source: pkg.source.clone(),
            })
            .collect::<Vec<_>>();
        removed.sort_by(|a, b| a.name.cmp(&b.name));
        self.packages.retain(|pkg| !names.contains(&pkg.name));
        Ok(removed)
    }

    pub fn remove_conda_unlinks(&mut self, unlinks: &[PlannedUnlink]) -> usize {
        let names = unlinks
            .iter()
            .map(|item| item.name.clone())
            .collect::<Vec<_>>();
        let before = self.packages.len();
        self.packages
            .retain(|pkg| !names.iter().any(|name| name == &pkg.name));
        before.saturating_sub(self.packages.len())
    }

    pub fn conda_packages(&self) -> Vec<PackageRecord> {
        self.packages
            .iter()
            .filter(|pkg| pkg.source == "conda")
            .cloned()
            .collect()
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
        requested_specs: &[String],
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
        block.push_str(&format!(
            "# {operation} specs: {}\n",
            serde_json::to_string(requested_specs)?
        ));
        for link in conda_links {
            let dist = if link.dist_name.is_empty() {
                format!("{}-{}-{}", link.name, link.version, link.build)
            } else {
                link.dist_name.clone()
            };
            block.push_str(&format!("+ {dist}\n"));
        }
        for name in removed {
            block.push_str(&format!("- {name}\n"));
        }
        block.push('\n');

        let mut existing = match fs::read_to_string(&history_path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(CoreError::Io(err)),
        };
        existing.push_str(&block);
        fs::write(history_path, existing)?;
        Ok(())
    }

    pub fn revisions(prefix: &Path) -> Result<Vec<RevisionRecord>, CoreError> {
        let history = fs::read_to_string(history_path(prefix))?;
        let mut revisions = Vec::new();
        let mut date = String::new();
        let mut install = Vec::new();
        let mut remove = Vec::new();
        let mut has_header = false;

        for line in history.lines() {
            if line.starts_with("==> ") && line.ends_with(" <==") {
                if has_header && (!install.is_empty() || !remove.is_empty()) {
                    revisions.push(RevisionRecord {
                        rev: revisions.len(),
                        date: date.clone(),
                        install: install.clone(),
                        remove: remove.clone(),
                    });
                }
                date = line
                    .trim_start_matches("==> ")
                    .trim_end_matches(" <==")
                    .to_string();
                install.clear();
                remove.clear();
                has_header = true;
                continue;
            }

            if let Some(dist) = line.strip_prefix("+ ") {
                install.push(dist.to_string());
                continue;
            }
            if let Some(dist) = line.strip_prefix("- ") {
                remove.push(dist.to_string());
            }
        }

        if has_header && (!install.is_empty() || !remove.is_empty()) {
            revisions.push(RevisionRecord {
                rev: revisions.len(),
                date,
                install,
                remove,
            });
        }

        Ok(revisions)
    }
}

#[derive(Debug, Deserialize)]
struct PipInspectReport {
    #[serde(default)]
    installed: Vec<PipInspectItem>,
    #[serde(default)]
    environment: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct PipInspectItem {
    #[serde(default)]
    metadata: PipInspectMetadata,
    installer: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PipInspectMetadata {
    name: Option<String>,
    version: Option<String>,
}

#[derive(Debug)]
struct DiscoveredPipPackage {
    name: String,
    version: String,
    platform: String,
}

impl EnvironmentState {
    fn merge_pip_site_packages(&mut self, prefix: &Path) -> Result<(), CoreError> {
        if !self
            .packages
            .iter()
            .any(|pkg| pkg.source == "conda" && pkg.name == "pip")
        {
            return Ok(());
        }

        for pkg in discover_pip_site_packages(prefix)? {
            let name = pkg.name;
            if self.packages.iter().any(|pkg| pkg.name == name) {
                continue;
            }
            self.packages.push(PackageRecord {
                name: name.clone(),
                version: pkg.version.clone(),
                build_string: "pypi_0".to_string(),
                channel: "pypi".to_string(),
                base_url: "https://pypi.org".to_string(),
                url: format!("https://pypi.org/project/{name}/"),
                md5: None,
                sha256: None,
                build_number: 0,
                dist_name: format!("{}-{}", name, pkg.version),
                spec: format!("{}=={}", name, pkg.version),
                source: "pip".to_string(),
                depends: Vec::new(),
                installed_at: Utc::now().to_rfc3339(),
                platform: pkg.platform.clone(),
            });
        }
        self.packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(())
    }
}

fn discover_pip_site_packages(prefix: &Path) -> Result<Vec<DiscoveredPipPackage>, CoreError> {
    let Some(python) = resolve_python_from_prefix_path(prefix) else {
        return Err(CoreError::PrefixState(format!(
            "cannot discover pip site-packages because no python executable was found in '{}'",
            prefix.display()
        )));
    };
    let output = match Command::new(&python)
        .args(["-m", "pip", "inspect", "--local"])
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            return Err(CoreError::PrefixState(format!(
                "failed to run '{} -m pip inspect --local': {}",
                python.display(),
                err
            )));
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CoreError::PrefixState(if stderr.is_empty() {
            format!(
                "pip inspect exited with status {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string())
            )
        } else {
            format!(
                "pip inspect exited with status {}: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string()),
                stderr
            )
        }));
    }
    let report = match serde_json::from_slice::<PipInspectReport>(&output.stdout) {
        Ok(report) => report,
        Err(err) => {
            return Err(CoreError::PrefixState(format!(
                "pip inspect returned invalid JSON: {}",
                err
            )));
        }
    };
    let platform = inspect_platform(&report.environment).unwrap_or_default();
    Ok(report
        .installed
        .into_iter()
        .filter_map(|item| {
            let installer = item
                .installer
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if installer != "pip" && installer != "uv" {
                return None;
            }
            let name = item.metadata.name?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let version = item
                .metadata
                .version
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or("unknown")
                .to_string();
            Some(DiscoveredPipPackage {
                name,
                version,
                platform: platform.clone(),
            })
        })
        .collect())
}

fn inspect_platform(environment: &serde_json::Value) -> Option<String> {
    if let (Some(sys_platform), Some(machine)) = (
        environment.get("sys_platform").and_then(|v| v.as_str()),
        environment.get("platform_machine").and_then(|v| v.as_str()),
    ) && !sys_platform.is_empty()
        && !machine.is_empty()
    {
        return Some(format!("{sys_platform}-{machine}"));
    }
    if let (Some(sys_platform), Some(machine)) = (
        environment
            .get("default_environment")
            .and_then(|v| v.get("sys_platform"))
            .and_then(|v| v.as_str()),
        environment
            .get("default_environment")
            .and_then(|v| v.get("platform_machine"))
            .and_then(|v| v.as_str()),
    ) && !sys_platform.is_empty()
        && !machine.is_empty()
    {
        return Some(format!("{sys_platform}-{machine}"));
    }
    None
}

fn resolve_python_from_prefix_path(prefix: &Path) -> Option<PathBuf> {
    let mut search_dirs = prefix_path_dirs(prefix);
    if let Some(current_path) = std::env::var_os("PATH") {
        search_dirs.extend(std::env::split_paths(&current_path));
    }
    for dir in search_dirs {
        for candidate in python_command_names() {
            let path = dir.join(candidate);
            if path.exists() && path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn prefix_path_dirs(prefix: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![prefix.clone(), prefix.join("Scripts"), prefix.join("Library").join("bin")]
    }
    #[cfg(not(windows))]
    {
        vec![prefix.join("bin")]
    }
}

fn python_command_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["python.exe", "python.bat", "python"]
    }
    #[cfg(not(windows))]
    {
        &["python"]
    }
}

fn requested_name_from_history_spec(spec: &str) -> Option<String> {
    if let Ok(info) = parse_explicit_url(spec) {
        return Some(info.name);
    }
    package_name_from_spec(spec).ok()
}

fn package_record_filename(record: &PackageRecord) -> String {
    if record.source == "conda" {
        if !record.dist_name.is_empty() {
            return format!("{}.json", record.dist_name);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_specs_map_uses_matchspec_package_name_keys() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let prefix = tmp.path().join("env");
        fs::create_dir_all(prefix.join("conda-meta")).expect("create conda-meta");
        fs::write(
            prefix.join("conda-meta").join("history"),
            r#"==> 2026-03-24T00:00:00Z <==
operation: install
# install specs: ["conda-forge::python>=3.11"]
+ python-3.12.0-0
"#,
        )
        .expect("write history");

        let requested = EnvironmentState::requested_specs_map(&prefix).expect("parse history");
        assert!(requested.contains_key("python"));
        assert!(!requested.contains_key("conda-forge"));
    }
}

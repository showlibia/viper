use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::CoreError;
use crate::state::{EnvironmentState, ensure_prefix_layout};
use crate::types::PackageRecord;

#[derive(Debug, Clone, Serialize)]
pub struct PlannedLink {
    pub name: String,
    pub version: String,
    pub build: String,
    pub build_number: i64,
    pub dist_name: String,
    pub channel: String,
    pub base_url: String,
    pub url: String,
    pub md5: Option<String>,
    pub sha256: Option<String>,
    pub depends: Vec<String>,
    pub platform: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedUnlink {
    pub name: String,
    pub version: String,
    pub build: String,
    pub dist_name: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedFetch {
    pub dist_name: String,
    pub url: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedExtract {
    pub dist_name: String,
    pub fetched_dist: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionPlan {
    pub fetch: Vec<PlannedFetch>,
    pub extract: Vec<PlannedExtract>,
    pub link: Vec<PlannedLink>,
    pub unlink: Vec<PlannedUnlink>,
}

impl TransactionPlan {
    pub fn from_solved(installed: &[PackageRecord], solved: &[PlannedLink]) -> Self {
        let installed_by_name = installed
            .iter()
            .filter(|pkg| pkg.source == "conda")
            .map(|pkg| (pkg.name.clone(), pkg))
            .collect::<HashMap<_, _>>();
        let solved_by_name = solved
            .iter()
            .map(|pkg| (pkg.name.clone(), pkg))
            .collect::<HashMap<_, _>>();

        let mut link = Vec::new();
        for solved_pkg in solved {
            let same = installed_by_name
                .get(&solved_pkg.name)
                .is_some_and(|installed_pkg| {
                    installed_pkg.version == solved_pkg.version
                        && installed_pkg.build_string == solved_pkg.build
                });
            if !same {
                link.push(solved_pkg.clone());
            }
        }

        let mut unlink = Vec::new();
        for installed_pkg in installed.iter().filter(|pkg| pkg.source == "conda") {
            let keep_same = solved_by_name
                .get(&installed_pkg.name)
                .is_some_and(|solved_pkg| {
                    installed_pkg.version == solved_pkg.version
                        && installed_pkg.build_string == solved_pkg.build
                });
            if !keep_same {
                unlink.push(PlannedUnlink {
                    name: installed_pkg.name.clone(),
                    version: installed_pkg.version.clone(),
                    build: installed_pkg.build_string.clone(),
                    dist_name: if installed_pkg.dist_name.is_empty() {
                        format!(
                            "{}-{}-{}",
                            installed_pkg.name, installed_pkg.version, installed_pkg.build_string
                        )
                    } else {
                        installed_pkg.dist_name.clone()
                    },
                    source: installed_pkg.source.clone(),
                });
            }
        }

        let fetch = link
            .iter()
            .filter(|planned| planned.source == "conda")
            .map(|planned| PlannedFetch {
                dist_name: if planned.dist_name.is_empty() {
                    format!("{}-{}-{}", planned.name, planned.version, planned.build)
                } else {
                    planned.dist_name.clone()
                },
                url: planned.url.clone(),
                source: planned.source.clone(),
            })
            .collect::<Vec<_>>();

        let extract = fetch
            .iter()
            .map(|planned| PlannedExtract {
                dist_name: planned.dist_name.clone(),
                fetched_dist: planned.dist_name.clone(),
                source: planned.source.clone(),
            })
            .collect::<Vec<_>>();

        Self {
            fetch,
            extract,
            link,
            unlink,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransactionExecutor {
    pub operation: String,
    pub requested_specs: Vec<String>,
    pub pip_specs: Vec<String>,
    pub platform: String,
    pub dry_run: bool,
    pub ensure_layout: bool,
}

#[derive(Debug, Clone)]
pub struct TransactionOutcome {
    pub state: EnvironmentState,
    pub fetched: usize,
    pub extracted: usize,
    pub linked: usize,
    pub unlinked: usize,
    pub pip_changed: usize,
}

impl TransactionExecutor {
    pub fn apply(
        &self,
        prefix: &Path,
        mut state: EnvironmentState,
        plan: &TransactionPlan,
    ) -> Result<TransactionOutcome, CoreError> {
        if self.dry_run {
            let fetched = self.preview_fetch_phase(plan);
            let extracted = self.preview_extract_phase(plan);
            let unlinked = state.remove_conda_unlinks(&plan.unlink);
            let linked =
                state.install_conda_links(&plan.link, &self.requested_specs, &self.platform)?;
            let pip_changed = state.install_pip_specs(&self.pip_specs, &self.platform)?;
            return Ok(TransactionOutcome {
                state,
                fetched,
                extracted,
                linked,
                unlinked,
                pip_changed,
            });
        }

        let snapshot = PrefixSnapshot::capture(prefix)?;
        let mut fetched = 0usize;
        let mut extracted = 0usize;
        let mut unlinked = 0usize;
        let mut linked = 0usize;
        let mut pip_changed = 0usize;
        let tx_result = (|| -> Result<(), CoreError> {
            clean_trash_files(prefix)?;
            if self.ensure_layout {
                ensure_prefix_layout(prefix)?;
            }

            fetched = self.run_fetch_phase(prefix, plan)?;
            if should_fail("after_fetch") {
                return Err(CoreError::TransactionFailed(
                    "injected failure after fetch phase".to_string(),
                ));
            }

            extracted = self.run_extract_phase(prefix, plan)?;
            if should_fail("after_extract") {
                return Err(CoreError::TransactionFailed(
                    "injected failure after extract phase".to_string(),
                ));
            }

            self.run_unlink_phase(prefix, plan)?;
            unlinked = state.remove_conda_unlinks(&plan.unlink);
            if should_fail("after_unlink") {
                return Err(CoreError::TransactionFailed(
                    "injected failure after unlink phase".to_string(),
                ));
            }

            linked =
                state.install_conda_links(&plan.link, &self.requested_specs, &self.platform)?;
            self.run_link_phase(prefix, plan)?;
            if should_fail("after_link") {
                return Err(CoreError::TransactionFailed(
                    "injected failure after link phase".to_string(),
                ));
            }

            pip_changed = state.install_pip_specs(&self.pip_specs, &self.platform)?;

            if should_fail("before_persist") {
                return Err(CoreError::TransactionFailed(
                    "injected failure before persist".to_string(),
                ));
            }

            state.persist(prefix)?;
            if should_fail("after_persist") {
                return Err(CoreError::TransactionFailed(
                    "injected failure after persist".to_string(),
                ));
            }

            let removed = plan
                .unlink
                .iter()
                .map(|item| item.dist_name.clone())
                .collect::<Vec<_>>();
            EnvironmentState::append_history(
                prefix,
                &self.operation,
                &self.requested_specs,
                &plan.link,
                &removed,
            )?;
            if should_fail("after_history") {
                return Err(CoreError::TransactionFailed(
                    "injected failure after history".to_string(),
                ));
            }
            self.cleanup_stage_artifacts(prefix)?;
            Ok(())
        })();
        if let Err(err) = tx_result {
            PrefixSnapshot::restore(prefix, &snapshot)?;
            return Err(err);
        }

        Ok(TransactionOutcome {
            state,
            fetched,
            extracted,
            linked,
            unlinked,
            pip_changed,
        })
    }

    fn preview_fetch_phase(&self, plan: &TransactionPlan) -> usize {
        plan.fetch.len()
    }

    fn preview_extract_phase(&self, plan: &TransactionPlan) -> usize {
        plan.extract.len()
    }

    fn run_fetch_phase(&self, prefix: &Path, plan: &TransactionPlan) -> Result<usize, CoreError> {
        if should_fail("before_fetch") {
            return Err(CoreError::TransactionFailed(
                "injected failure before fetch phase".to_string(),
            ));
        }
        let stage_dir = fetch_stage_dir(prefix);
        fs::create_dir_all(&stage_dir)?;
        for fetch in &plan.fetch {
            let path = stage_dir.join(format!("{}.fetched", fetch.dist_name));
            fs::write(path, &fetch.url)?;
        }
        Ok(plan.fetch.len())
    }

    fn run_extract_phase(&self, prefix: &Path, plan: &TransactionPlan) -> Result<usize, CoreError> {
        if should_fail("before_extract") {
            return Err(CoreError::TransactionFailed(
                "injected failure before extract phase".to_string(),
            ));
        }
        let fetch_dir = fetch_stage_dir(prefix);
        let extract_dir = extract_stage_dir(prefix);
        fs::create_dir_all(&extract_dir)?;
        for extract in &plan.extract {
            let fetched_path = fetch_dir.join(format!("{}.fetched", extract.fetched_dist));
            if !fetched_path.exists() {
                return Err(CoreError::TransactionFailed(format!(
                    "missing fetched artifact for '{}'",
                    extract.dist_name
                )));
            }
            let path = extract_dir.join(format!("{}.extracted", extract.dist_name));
            fs::write(path, &extract.fetched_dist)?;
        }
        Ok(plan.extract.len())
    }

    fn cleanup_stage_artifacts(&self, prefix: &Path) -> Result<(), CoreError> {
        let stage_root = phase_stage_root(prefix);
        if stage_root.exists() {
            fs::remove_dir_all(stage_root)?;
        }
        Ok(())
    }

    fn run_unlink_phase(&self, prefix: &Path, plan: &TransactionPlan) -> Result<(), CoreError> {
        for unlink in plan.unlink.iter().filter(|item| item.source == "conda") {
            let payload = package_payload_path(prefix, &unlink.name);
            if !payload.exists() {
                continue;
            }
            remove_payload_path(prefix, &payload)?;
        }
        Ok(())
    }

    fn run_link_phase(&self, prefix: &Path, plan: &TransactionPlan) -> Result<(), CoreError> {
        for link in plan.link.iter().filter(|item| item.source == "conda") {
            let payload = package_payload_path(prefix, &link.name);
            if let Some(parent) = payload.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&payload, package_payload_contents(link))?;
            set_executable_if_unix(&payload)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum PrefixSnapshot {
    MissingPrefix,
    Existing { entries: Vec<SnapshotEntry> },
}

#[derive(Debug, Clone)]
enum SnapshotEntry {
    Dir(PathBuf),
    File(PathBuf, Vec<u8>),
    Symlink(PathBuf, PathBuf, bool),
}

impl PrefixSnapshot {
    fn capture(prefix: &Path) -> Result<Self, CoreError> {
        if !prefix.exists() {
            return Ok(Self::MissingPrefix);
        }
        let mut entries = Vec::new();
        collect_entries(prefix, prefix, &mut entries)?;
        Ok(Self::Existing { entries })
    }

    fn restore(prefix: &Path, snapshot: &Self) -> Result<(), CoreError> {
        match snapshot {
            Self::MissingPrefix => {
                if prefix.exists() {
                    fs::remove_dir_all(prefix)?;
                }
                Ok(())
            }
            Self::Existing { entries } => {
                if prefix.exists() {
                    fs::remove_dir_all(prefix)?;
                }
                fs::create_dir_all(prefix)?;
                let mut dirs = entries
                    .iter()
                    .filter_map(|entry| {
                        if let SnapshotEntry::Dir(path) = entry {
                            Some(path.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                dirs.sort_by_key(|path| path.components().count());
                for rel in dirs {
                    if rel.as_os_str().is_empty() {
                        continue;
                    }
                    fs::create_dir_all(prefix.join(rel))?;
                }
                for entry in entries {
                    if let SnapshotEntry::File(rel, content) = entry {
                        let path = prefix.join(rel);
                        if let Some(parent) = path.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(path, content)?;
                    }
                    if let SnapshotEntry::Symlink(rel, target, is_dir) = entry {
                        let path = prefix.join(rel);
                        if let Some(parent) = path.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        create_symlink(&path, target, *is_dir)?;
                    }
                }
                Ok(())
            }
        }
    }
}

fn collect_entries(
    prefix: &Path,
    current: &Path,
    out: &mut Vec<SnapshotEntry>,
) -> Result<(), CoreError> {
    let rel = current
        .strip_prefix(prefix)
        .map_err(|e| CoreError::TransactionFailed(e.to_string()))?
        .to_path_buf();
    out.push(SnapshotEntry::Dir(rel));
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            let rel = path
                .strip_prefix(prefix)
                .map_err(|e| CoreError::TransactionFailed(e.to_string()))?
                .to_path_buf();
            let target = fs::read_link(&path)?;
            let is_dir = fs::metadata(&path).is_ok_and(|m| m.is_dir());
            out.push(SnapshotEntry::Symlink(rel, target, is_dir));
            continue;
        }
        if file_type.is_dir() {
            collect_entries(prefix, &path, out)?;
            continue;
        }
        if file_type.is_file() {
            let rel = path
                .strip_prefix(prefix)
                .map_err(|e| CoreError::TransactionFailed(e.to_string()))?
                .to_path_buf();
            out.push(SnapshotEntry::File(rel, fs::read(&path)?));
            continue;
        }
        return Err(CoreError::TransactionFailed(format!(
            "unsupported filesystem entry in snapshot: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlink(path: &Path, target: &Path, _is_dir: bool) -> Result<(), CoreError> {
    std::os::unix::fs::symlink(target, path)?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink(path: &Path, target: &Path, is_dir: bool) -> Result<(), CoreError> {
    if is_dir {
        std::os::windows::fs::symlink_dir(target, path)?;
    } else {
        std::os::windows::fs::symlink_file(target, path)?;
    }
    Ok(())
}

fn should_fail(stage: &str) -> bool {
    std::env::var("VIPER_TX_FAIL_POINT")
        .ok()
        .is_some_and(|configured| configured == stage)
}

fn phase_stage_root(prefix: &Path) -> PathBuf {
    prefix.join("pkgs").join(".viper-transaction")
}

fn fetch_stage_dir(prefix: &Path) -> PathBuf {
    phase_stage_root(prefix).join("fetch")
}

fn extract_stage_dir(prefix: &Path) -> PathBuf {
    phase_stage_root(prefix).join("extract")
}

fn package_payload_contents(link: &PlannedLink) -> String {
    #[cfg(not(windows))]
    {
        if link.name == "python" {
            return "#!/bin/sh\nif [ \"$1\" = \"-m\" ] && [ \"$2\" = \"pip\" ] && [ \"$3\" = \"inspect\" ] && [ \"$4\" = \"--local\" ]; then\n  printf '{\"installed\":[]}'\n  exit 0\nfi\nexit 0\n".to_string();
        }
    }
    format!("viper package payload: {}-{}", link.name, link.version)
}

fn package_payload_path(prefix: &Path, package_name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        prefix.join(format!("{package_name}.exe"))
    }
    #[cfg(not(windows))]
    {
        prefix.join("bin").join(package_name)
    }
}

fn remove_payload_path(prefix: &Path, payload: &Path) -> Result<(), CoreError> {
    #[cfg(windows)]
    {
        return remove_payload_path_windows(prefix, payload);
    }
    #[cfg(not(windows))]
    {
        let _ = prefix;
        fs::remove_file(payload)?;
        Ok(())
    }
}

#[cfg(windows)]
fn remove_payload_path_windows(prefix: &Path, payload: &Path) -> Result<(), CoreError> {
    match fs::remove_file(payload) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            let file_name = payload
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    CoreError::TransactionFailed(format!(
                        "cannot create trash name for '{}'",
                        payload.display()
                    ))
                })?;
            let trash_name = format!("{file_name}.mamba_trash");
            let trash_path = payload.with_file_name(trash_name);
            fs::rename(payload, &trash_path)?;

            let rel = trash_path
                .strip_prefix(prefix)
                .map_err(|e| CoreError::TransactionFailed(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            let trash_record = prefix.join("conda-meta").join("mamba_trash.txt");
            let mut existing = match fs::read_to_string(&trash_record) {
                Ok(raw) => raw,
                Err(read_err) if read_err.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(read_err) => return Err(CoreError::Io(read_err)),
            };
            existing.push_str(&rel);
            existing.push('\n');
            fs::write(trash_record, existing)?;
            Ok(())
        }
        Err(err) => Err(CoreError::Io(err)),
    }
}

#[cfg(not(windows))]
fn set_executable_if_unix(path: &Path) -> Result<(), CoreError> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(windows)]
fn set_executable_if_unix(_path: &Path) -> Result<(), CoreError> {
    Ok(())
}

fn clean_trash_files(prefix: &Path) -> Result<(), CoreError> {
    let trash_index = prefix.join("conda-meta").join("mamba_trash.txt");
    if !trash_index.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&trash_index)?;
    let mut survivors = Vec::new();
    for line in content.lines() {
        let rel = line.trim();
        if rel.is_empty() {
            continue;
        }
        let path = prefix.join(rel);
        if !path.exists() {
            continue;
        }
        let remove_result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        if remove_result.is_err() {
            survivors.push(rel.to_string());
        }
    }
    if survivors.is_empty() {
        fs::remove_file(trash_index)?;
    } else {
        fs::write(trash_index, format!("{}\n", survivors.join("\n")))?;
    }
    Ok(())
}

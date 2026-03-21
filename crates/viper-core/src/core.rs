use std::fs;

use serde_json::json;

use crate::config::{ConfigInput, ConfigStore, build_config};
use crate::error::CoreError;
use crate::repodata::fetch_packages;
use crate::solver::{solve_to_actions, spec_requires_full_repodata};
use crate::spec::parse_env_file;
use crate::state::{EnvironmentState, ensure_prefix_layout, is_managed_prefix};
use crate::types::{CliConfigCommand, CliOperation, OperationRequest, OperationResult};

struct NormalizedRequestInputs {
    conda_specs: Vec<String>,
    pip_specs: Vec<String>,
    yaml_name: Option<String>,
    yaml_file_stem: Option<String>,
    channels: Vec<String>,
}

pub fn execute(request: OperationRequest) -> Result<OperationResult, CoreError> {
    let globals = request.globals.clone();
    let op = request.op;
    let store = ConfigStore::from_home()?;
    let config = build_config(
        ConfigInput {
            globals: globals.clone(),
        },
        &store,
    )?;

    match op {
        CliOperation::Create { specs, file } => {
            let normalized =
                normalize_request_inputs(specs, file, &config.channels, &globals.channels)?;
            let target_prefix = resolve_create_target_prefix(
                &globals,
                &config.root_prefix,
                normalized.yaml_name.as_deref(),
                normalized.yaml_file_stem.as_deref(),
            )?;
            let repodata_filename = select_repodata_filename(&normalized.conda_specs);
            let repodata = if normalized.conda_specs.is_empty() {
                Vec::new()
            } else {
                fetch_packages(
                    &normalized.channels,
                    &current_platform_subdir(),
                    config.offline,
                    &config.root_prefix.join("pkgs").join("cache"),
                    config.local_repodata_ttl,
                    repodata_filename,
                )?
            };
            let mut link_actions = solve_to_actions(&normalized.conda_specs, &repodata);
            if has_unresolved_conda_actions(&link_actions) {
                return Err(CoreError::UnsatisfiedSpecs(unresolved_action_names(
                    &link_actions,
                )));
            }
            link_actions.extend(normalized.pip_specs.iter().map(|spec| {
                crate::transaction::PlannedLink {
                    name: crate::spec::package_name_from_spec(spec)
                        .unwrap_or_else(|_| spec.clone()),
                    version: "unknown".to_string(),
                    build: "pip".to_string(),
                    channel: "pypi".to_string(),
                    url: String::new(),
                    source: "pip".to_string(),
                }
            }));

            let mut state = EnvironmentState::empty();
            state.install_specs(&normalized.conda_specs)?;
            state.install_pip_specs(&normalized.pip_specs)?;

            if !config.dry_run {
                ensure_prefix_layout(&target_prefix)?;
                state.save(&target_prefix)?;
            }

            let result = OperationResult::ok(
                "environment created",
                json!({
                    "root_prefix": config.root_prefix,
                    "target_prefix": target_prefix,
                    "channels": normalized.channels,
                    "specs": normalized.conda_specs,
                    "pip_specs": normalized.pip_specs,
                    "actions": {
                        "link": link_actions,
                    },
                    "dry_run": config.dry_run,
                }),
            );
            Ok(result)
        }
        CliOperation::Install { specs, file } => {
            let target_prefix = config
                .target_prefix
                .clone()
                .ok_or(CoreError::MissingTargetPrefix)?;
            if !target_prefix.exists() {
                return Err(CoreError::PrefixNotFound(
                    target_prefix.display().to_string(),
                ));
            }
            if !is_managed_prefix(&target_prefix) {
                return Err(CoreError::NotManagedPrefix(
                    target_prefix.display().to_string(),
                ));
            }

            let normalized =
                normalize_request_inputs(specs, file, &config.channels, &globals.channels)?;
            let repodata_filename = select_repodata_filename(&normalized.conda_specs);
            let repodata = if normalized.conda_specs.is_empty() {
                Vec::new()
            } else {
                fetch_packages(
                    &normalized.channels,
                    &current_platform_subdir(),
                    config.offline,
                    &config.root_prefix.join("pkgs").join("cache"),
                    config.local_repodata_ttl,
                    repodata_filename,
                )?
            };
            let mut link_actions = solve_to_actions(&normalized.conda_specs, &repodata);
            if has_unresolved_conda_actions(&link_actions) {
                return Err(CoreError::UnsatisfiedSpecs(unresolved_action_names(
                    &link_actions,
                )));
            }
            link_actions.extend(normalized.pip_specs.iter().map(|spec| {
                crate::transaction::PlannedLink {
                    name: crate::spec::package_name_from_spec(spec)
                        .unwrap_or_else(|_| spec.clone()),
                    version: "unknown".to_string(),
                    build: "pip".to_string(),
                    channel: "pypi".to_string(),
                    url: String::new(),
                    source: "pip".to_string(),
                }
            }));

            let mut state = EnvironmentState::load(&target_prefix)?;
            let changed = state.install_specs(&normalized.conda_specs)?;
            let pip_changed = state.install_pip_specs(&normalized.pip_specs)?;

            if !config.dry_run {
                state.save(&target_prefix)?;
            }

            let result = OperationResult::ok(
                "packages installed",
                json!({
                    "target_prefix": target_prefix,
                    "changed": changed + pip_changed,
                    "specs": normalized.conda_specs,
                    "pip_specs": normalized.pip_specs,
                    "actions": {
                        "link": link_actions,
                    },
                    "dry_run": config.dry_run,
                }),
            );
            Ok(result)
        }
        CliOperation::Remove { specs, all } => {
            let target_prefix = config
                .target_prefix
                .clone()
                .ok_or(CoreError::MissingTargetPrefix)?;
            if !target_prefix.exists() {
                return Err(CoreError::PrefixNotFound(
                    target_prefix.display().to_string(),
                ));
            }
            if !is_managed_prefix(&target_prefix) {
                return Err(CoreError::NotManagedPrefix(
                    target_prefix.display().to_string(),
                ));
            }

            if all {
                if !config.dry_run {
                    fs::remove_dir_all(&target_prefix)?;
                }
                return Ok(OperationResult::ok(
                    "environment removed",
                    json!({
                        "target_prefix": target_prefix,
                        "removed_all": true,
                        "dry_run": config.dry_run,
                    }),
                ));
            }

            let mut state = EnvironmentState::load(&target_prefix)?;
            let removed = state.remove_specs(&specs)?;
            if !config.dry_run {
                state.save(&target_prefix)?;
            }

            Ok(OperationResult::ok(
                "packages removed",
                json!({
                    "target_prefix": target_prefix,
                    "removed": removed,
                    "specs": specs,
                    "dry_run": config.dry_run,
                }),
            ))
        }
        CliOperation::List => {
            let target_prefix = config
                .target_prefix
                .clone()
                .ok_or(CoreError::MissingTargetPrefix)?;
            if !target_prefix.exists() {
                return Err(CoreError::PrefixNotFound(
                    target_prefix.display().to_string(),
                ));
            }
            if !is_managed_prefix(&target_prefix) {
                return Err(CoreError::NotManagedPrefix(
                    target_prefix.display().to_string(),
                ));
            }

            let state = EnvironmentState::load(&target_prefix)?;
            Ok(OperationResult::ok(
                "packages listed",
                json!({
                    "target_prefix": target_prefix,
                    "packages": state.packages,
                }),
            ))
        }
        CliOperation::Info => {
            let env_exists = config
                .target_prefix
                .as_ref()
                .map(|p| p.exists())
                .unwrap_or(false);
            Ok(OperationResult::ok(
                "environment info",
                json!({
                    "root_prefix": config.root_prefix,
                    "target_prefix": config.target_prefix,
                    "channels": config.channels,
                    "channel_priority": config.channel_priority,
                    "offline": config.offline,
                    "local_repodata_ttl": config.local_repodata_ttl,
                    "json": config.json,
                    "env_exists": env_exists,
                    "platform": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                }),
            ))
        }
        CliOperation::Config(cmd) => match cmd {
            CliConfigCommand::List => Ok(OperationResult::ok(
                "config listed",
                json!({
                    "root_prefix": config.root_prefix,
                    "channels": config.channels,
                    "channel_priority": config.channel_priority,
                    "always_yes": config.always_yes,
                    "offline": config.offline,
                    "local_repodata_ttl": config.local_repodata_ttl,
                    "rc_path": store.path(),
                }),
            )),
            CliConfigCommand::Get { key } => {
                let value = match key.as_str() {
                    "root_prefix" => json!(config.root_prefix),
                    "channels" => json!(config.channels),
                    "channel_priority" => json!(config.channel_priority),
                    "always_yes" => json!(config.always_yes),
                    "offline" => json!(config.offline),
                    "local_repodata_ttl" => json!(config.local_repodata_ttl),
                    other => return Err(CoreError::UnsupportedConfigKey(other.to_string())),
                };

                Ok(OperationResult::ok(
                    "config key fetched",
                    json!({
                        "key": key,
                        "value": value,
                    }),
                ))
            }
            CliConfigCommand::Set { key, value } => {
                store.save_rc_value(&key, &value)?;
                Ok(OperationResult::ok(
                    "config key updated",
                    json!({
                        "key": key,
                        "value": value,
                        "rc_path": store.path(),
                    }),
                ))
            }
        },
    }
}

fn normalize_request_inputs(
    cli_specs: Vec<String>,
    file: Option<std::path::PathBuf>,
    base_channels: &[String],
    cli_channels: &[String],
) -> Result<NormalizedRequestInputs, CoreError> {
    let mut conda_specs = cli_specs;
    let mut pip_specs = Vec::new();
    let mut yaml_name = None;
    let mut yaml_file_stem = None;
    let mut yaml_channels = Vec::new();

    if let Some(path) = file {
        let parsed = parse_env_file(&path)?;
        conda_specs.extend(parsed.conda_specs);
        pip_specs.extend(parsed.pip_specs);
        yaml_name = parsed.name;
        yaml_file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        yaml_channels = parsed.channels;
    }

    Ok(NormalizedRequestInputs {
        conda_specs,
        pip_specs,
        yaml_name,
        yaml_file_stem,
        channels: effective_channels(base_channels, cli_channels, &yaml_channels),
    })
}

fn resolve_create_target_prefix(
    globals: &crate::types::CliGlobalOptions,
    root_prefix: &std::path::Path,
    yaml_name: Option<&str>,
    yaml_file_stem: Option<&str>,
) -> Result<std::path::PathBuf, CoreError> {
    globals
        .prefix
        .clone()
        .or_else(|| {
            globals
                .name
                .as_ref()
                .map(|name| root_prefix.join("envs").join(name))
        })
        .or_else(|| yaml_name.map(|name| root_prefix.join("envs").join(name)))
        .or_else(|| yaml_file_stem.map(|name| root_prefix.join("envs").join(name)))
        .or_else(|| std::env::var_os("CONDA_PREFIX").map(std::path::PathBuf::from))
        .ok_or(CoreError::MissingTargetPrefix)
}

fn effective_channels(
    base_channels: &[String],
    cli_channels: &[String],
    yaml_channels: &[String],
) -> Vec<String> {
    if !cli_channels.is_empty() {
        return dedup_channels(cli_channels.iter().chain(yaml_channels.iter()));
    }
    if !yaml_channels.is_empty() {
        return dedup_channels(yaml_channels.iter());
    }
    base_channels.to_vec()
}

fn dedup_channels<'a>(channels: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut out = Vec::new();
    for channel in channels {
        if !out.iter().any(|c| c == channel) {
            out.push(channel.clone());
        }
    }
    out
}

fn current_platform_subdir() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    match (os, arch) {
        ("linux", "x86_64") => "linux-64".to_string(),
        ("linux", "aarch64") => "linux-aarch64".to_string(),
        ("macos", "x86_64") => "osx-64".to_string(),
        ("macos", "aarch64") => "osx-arm64".to_string(),
        ("windows", "x86_64") => "win-64".to_string(),
        _ => format!("{os}-{arch}"),
    }
}

fn select_repodata_filename(specs: &[String]) -> &'static str {
    if specs.iter().any(|spec| spec_requires_full_repodata(spec)) {
        "repodata.json"
    } else {
        "current_repodata.json"
    }
}

fn has_unresolved_conda_actions(actions: &[crate::transaction::PlannedLink]) -> bool {
    actions
        .iter()
        .any(|a| a.source == "conda" && a.channel == "unresolved")
}

fn unresolved_action_names(actions: &[crate::transaction::PlannedLink]) -> Vec<String> {
    let mut names = Vec::new();
    for action in actions
        .iter()
        .filter(|a| a.source == "conda" && a.channel == "unresolved")
    {
        if !names.iter().any(|n| n == &action.name) {
            names.push(action.name.clone());
        }
    }
    names
}

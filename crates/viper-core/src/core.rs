use std::fs;

use serde_json::json;

use crate::config::{ConfigInput, ConfigStore, build_config};
use crate::error::CoreError;
use crate::repodata::fetch_packages;
use crate::solver::solve_to_actions;
use crate::spec::parse_env_file;
use crate::state::{EnvironmentState, ensure_prefix_layout, is_managed_prefix};
use crate::types::{CliConfigCommand, CliOperation, OperationRequest, OperationResult};

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
            let mut all_specs = specs;
            let mut pip_specs = Vec::new();
            let mut file_name = None;
            let mut file_channels = Vec::new();
            if let Some(path) = file {
                let file_specs = parse_env_file(&path)?;
                all_specs.extend(file_specs.conda_specs);
                pip_specs.extend(file_specs.pip_specs);
                file_name = file_specs.name;
                file_channels = file_specs.channels;
            }

            let target_prefix = globals
                .prefix
                .clone()
                .or_else(|| {
                    globals
                        .name
                        .as_ref()
                        .map(|name| config.root_prefix.join("envs").join(name))
                })
                .or_else(|| {
                    file_name
                        .as_ref()
                        .map(|name| config.root_prefix.join("envs").join(name))
                })
                .or_else(|| std::env::var_os("CONDA_PREFIX").map(std::path::PathBuf::from))
                .ok_or(CoreError::MissingTargetPrefix)?;
            let channels = effective_channels(&config.channels, &globals.channels, &file_channels);
            let mut warnings = Vec::new();
            let repodata =
                match fetch_packages(&channels, &current_platform_subdir(), config.offline) {
                    Ok(pkgs) => pkgs,
                    Err(err) => {
                        warnings.push(err.to_string());
                        Vec::new()
                    }
                };
            let mut link_actions = solve_to_actions(&all_specs, &repodata);
            link_actions.extend(
                pip_specs
                    .iter()
                    .map(|spec| crate::transaction::PlannedLink {
                        name: crate::spec::package_name_from_spec(spec)
                            .unwrap_or_else(|_| spec.clone()),
                        version: "unknown".to_string(),
                        build: "pip".to_string(),
                        channel: "pypi".to_string(),
                        url: String::new(),
                        source: "pip".to_string(),
                    }),
            );

            let mut state = EnvironmentState::empty();
            state.install_specs(&all_specs)?;
            state.install_pip_specs(&pip_specs)?;

            if !config.dry_run {
                ensure_prefix_layout(&target_prefix)?;
                state.save(&target_prefix)?;
            }

            let mut result = OperationResult::ok(
                "environment created",
                json!({
                    "root_prefix": config.root_prefix,
                    "target_prefix": target_prefix,
                    "channels": channels,
                    "specs": all_specs,
                    "pip_specs": pip_specs,
                    "actions": {
                        "link": link_actions,
                    },
                    "dry_run": config.dry_run,
                }),
            );
            result.warnings = warnings;
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

            let mut all_specs = specs;
            let mut pip_specs = Vec::new();
            let mut file_channels = Vec::new();
            if let Some(path) = file {
                let parsed = parse_env_file(&path)?;
                all_specs.extend(parsed.conda_specs);
                pip_specs.extend(parsed.pip_specs);
                file_channels = parsed.channels;
            }
            let channels = effective_channels(&config.channels, &globals.channels, &file_channels);
            let mut warnings = Vec::new();
            let repodata =
                match fetch_packages(&channels, &current_platform_subdir(), config.offline) {
                    Ok(pkgs) => pkgs,
                    Err(err) => {
                        warnings.push(err.to_string());
                        Vec::new()
                    }
                };
            let mut link_actions = solve_to_actions(&all_specs, &repodata);
            link_actions.extend(
                pip_specs
                    .iter()
                    .map(|spec| crate::transaction::PlannedLink {
                        name: crate::spec::package_name_from_spec(spec)
                            .unwrap_or_else(|_| spec.clone()),
                        version: "unknown".to_string(),
                        build: "pip".to_string(),
                        channel: "pypi".to_string(),
                        url: String::new(),
                        source: "pip".to_string(),
                    }),
            );

            let mut state = EnvironmentState::load(&target_prefix)?;
            let changed = state.install_specs(&all_specs)?;
            let pip_changed = state.install_pip_specs(&pip_specs)?;

            if !config.dry_run {
                state.save(&target_prefix)?;
            }

            let mut result = OperationResult::ok(
                "packages installed",
                json!({
                    "target_prefix": target_prefix,
                    "changed": changed + pip_changed,
                    "specs": all_specs,
                    "pip_specs": pip_specs,
                    "actions": {
                        "link": link_actions,
                    },
                    "dry_run": config.dry_run,
                }),
            );
            result.warnings = warnings;
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

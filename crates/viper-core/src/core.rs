use std::fs;

use serde_json::json;

use crate::config::{ConfigInput, ConfigStore, build_config};
use crate::error::CoreError;
use crate::spec::parse_env_file;
use crate::state::{EnvironmentState, ensure_prefix_layout, is_managed_prefix};
use crate::types::{CliConfigCommand, CliOperation, OperationRequest, OperationResult};

pub fn execute(request: OperationRequest) -> Result<OperationResult, CoreError> {
    let store = ConfigStore::from_home()?;
    let config = build_config(
        ConfigInput {
            globals: request.globals,
        },
        &store,
    )?;

    match request.op {
        CliOperation::Create { specs, file } => {
            let target_prefix = config
                .target_prefix
                .clone()
                .ok_or(CoreError::MissingTargetPrefix)?;
            let mut all_specs = specs;
            if let Some(path) = file {
                let file_specs = parse_env_file(&path)?;
                all_specs.extend(file_specs);
            }

            let mut state = EnvironmentState::empty();
            state.install_specs(&all_specs)?;

            if !config.dry_run {
                ensure_prefix_layout(&target_prefix)?;
                state.save(&target_prefix)?;
            }

            Ok(OperationResult::ok(
                "environment created",
                json!({
                    "root_prefix": config.root_prefix,
                    "target_prefix": target_prefix,
                    "channels": config.channels,
                    "specs": all_specs,
                    "dry_run": config.dry_run,
                }),
            ))
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
            if let Some(path) = file {
                all_specs.extend(parse_env_file(&path)?);
            }

            let mut state = EnvironmentState::load(&target_prefix)?;
            let changed = state.install_specs(&all_specs)?;

            if !config.dry_run {
                state.save(&target_prefix)?;
            }

            Ok(OperationResult::ok(
                "packages installed",
                json!({
                    "target_prefix": target_prefix,
                    "changed": changed,
                    "specs": all_specs,
                    "dry_run": config.dry_run,
                }),
            ))
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

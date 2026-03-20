use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::types::CliGlobalOptions;

#[derive(Debug, Clone)]
pub struct ConfigInput {
    pub globals: CliGlobalOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub root_prefix: PathBuf,
    pub target_prefix: Option<PathBuf>,
    pub channels: Vec<String>,
    pub channel_priority: String,
    pub always_yes: bool,
    pub offline: bool,
    pub local_repodata_ttl: usize,
    pub dry_run: bool,
    pub json: bool,
    pub verbose: u8,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct RcFile {
    root_prefix: Option<PathBuf>,
    channels: Option<Vec<String>>,
    channel_priority: Option<String>,
    always_yes: Option<bool>,
    offline: Option<bool>,
    local_repodata_ttl: Option<usize>,
}

impl ConfigStore {
    pub fn from_home() -> Result<Self, CoreError> {
        let home = dirs::home_dir().ok_or(CoreError::HomeUnavailable)?;
        Ok(Self {
            path: home.join(".viper").join("viperrc"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load_rc(&self) -> Result<RcFile, CoreError> {
        if !self.path.exists() {
            return Ok(RcFile::default());
        }
        let raw = fs::read_to_string(&self.path)?;
        let parsed: RcFile = serde_yaml::from_str(&raw)?;
        Ok(parsed)
    }

    pub fn save_rc_value(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let mut rc = self.load_rc()?;
        match key {
            "channel_priority" => rc.channel_priority = Some(value.to_string()),
            "always_yes" => rc.always_yes = Some(parse_bool(value)?),
            "offline" => rc.offline = Some(parse_bool(value)?),
            "local_repodata_ttl" => rc.local_repodata_ttl = Some(parse_usize(value)?),
            "root_prefix" => rc.root_prefix = Some(PathBuf::from(value)),
            "channels" => {
                let split = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                rc.channels = Some(split);
            }
            other => return Err(CoreError::UnsupportedConfigKey(other.to_string())),
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_yaml::to_string(&rc)?;
        fs::write(&self.path, raw)?;
        Ok(())
    }
}

fn parse_bool(value: &str) -> Result<bool, CoreError> {
    match value {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(CoreError::InvalidEnvironmentFile(format!(
            "'{value}' is not a valid bool"
        ))),
    }
}

fn parse_usize(value: &str) -> Result<usize, CoreError> {
    value
        .parse::<usize>()
        .map_err(|_| CoreError::InvalidEnvironmentFile(format!("'{value}' is not a valid integer")))
}

pub fn build_config(input: ConfigInput, store: &ConfigStore) -> Result<Config, CoreError> {
    if input.globals.prefix.is_some() && input.globals.name.is_some() {
        return Err(CoreError::ConflictingTargetOptions);
    }

    let env_root = std::env::var_os("VIPER_ROOT_PREFIX").map(PathBuf::from);
    let env_channels = std::env::var("VIPER_CHANNELS").ok().map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    });
    let env_repodata_ttl = std::env::var("VIPER_LOCAL_REPODATA_TTL")
        .ok()
        .map(|x| parse_usize(&x))
        .transpose()?;

    let rc = if input.globals.no_rc {
        RcFile::default()
    } else {
        store.load_rc()?
    };

    let home = dirs::home_dir().ok_or(CoreError::HomeUnavailable)?;
    let default_root = home.join(".viper");

    let root_prefix = input
        .globals
        .root_prefix
        .clone()
        .or(env_root)
        .or(rc.root_prefix)
        .unwrap_or(default_root);

    let channels = if !input.globals.channels.is_empty() {
        input.globals.channels.clone()
    } else if let Some(ch) = env_channels {
        ch
    } else if let Some(ch) = rc.channels {
        ch
    } else {
        vec!["conda-forge".to_string()]
    };

    let target_prefix = resolve_target_prefix(
        input.globals.prefix.clone(),
        input.globals.name.clone(),
        &root_prefix,
    );

    Ok(Config {
        root_prefix,
        target_prefix,
        channels,
        channel_priority: rc
            .channel_priority
            .unwrap_or_else(|| "flexible".to_string()),
        always_yes: input.globals.yes || rc.always_yes.unwrap_or(false),
        offline: input.globals.offline || rc.offline.unwrap_or(false),
        local_repodata_ttl: input
            .globals
            .repodata_ttl
            .or(env_repodata_ttl)
            .or(rc.local_repodata_ttl)
            .unwrap_or(1),
        dry_run: input.globals.dry_run,
        json: input.globals.json,
        verbose: input.globals.verbose,
    })
}

fn resolve_target_prefix(
    explicit_prefix: Option<PathBuf>,
    env_name: Option<String>,
    root_prefix: &Path,
) -> Option<PathBuf> {
    if let Some(prefix) = explicit_prefix {
        return Some(prefix);
    }
    if let Some(name) = env_name {
        return Some(root_prefix.join("envs").join(name));
    }
    std::env::var_os("CONDA_PREFIX").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CliGlobalOptions;

    #[test]
    fn resolve_name_to_prefix() {
        let globals = CliGlobalOptions {
            root_prefix: Some(PathBuf::from("/tmp/root")),
            prefix: None,
            name: Some("dev".to_string()),
            channels: vec![],
            json: false,
            yes: false,
            dry_run: false,
            no_rc: true,
            offline: false,
            repodata_ttl: None,
            verbose: 0,
        };
        let cfg = build_config(
            ConfigInput { globals },
            &ConfigStore {
                path: PathBuf::from("/tmp/does-not-exist"),
            },
        )
        .expect("config must build");

        assert_eq!(cfg.target_prefix, Some(PathBuf::from("/tmp/root/envs/dev")));
    }
}

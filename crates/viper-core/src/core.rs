use std::collections::{HashMap, HashSet};
use std::fs;

use regex::Regex;
use serde_json::json;

use crate::config::{ConfigInput, ConfigStore, build_config};
use crate::error::CoreError;
use crate::repodata::{RepoPackage, fetch_packages};
use crate::solver::{SolveOptions, solve_to_actions, spec_requires_full_repodata};
use crate::spec::{
    SpecFileKind, normalize_spec, package_name_from_spec, parse_match_spec, parse_spec_file,
};
use crate::state::{EnvironmentState, is_managed_prefix};
use crate::transaction::{TransactionExecutor, TransactionPlan};
use crate::types::{
    CliConfigCommand, CliOperation, ListOptions, OperationRequest, OperationResult, PackageRecord,
};

struct NormalizedRequestInputs {
    conda_specs: Vec<String>,
    pip_specs: Vec<String>,
    yaml_name: Option<String>,
    yaml_file_stem: Option<String>,
    channels: Vec<String>,
    warnings: Vec<String>,
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
        CliOperation::Create { specs, files } => {
            let normalized = normalize_request_inputs(
                specs,
                files,
                &config.channels,
                &globals.channels,
                globals.name.as_deref(),
            )?;
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
            let solve_options = SolveOptions {
                channels: normalized.channels.clone(),
                strict_channel_priority: config.channel_priority == "strict",
                installed_preferred: HashMap::new(),
                user_requested: requested_names(&normalized.conda_specs),
            };
            let solved = solve_to_actions(&normalized.conda_specs, &repodata, &solve_options)
                .map_err(CoreError::UnsatisfiedSpecs)?;
            let conda_link_actions = solved.actions;
            let conda_plan = TransactionPlan::from_solved(&[], &conda_link_actions);
            let platform = current_platform_subdir();
            let mut link_actions = conda_plan.link.clone();
            link_actions.extend(normalized.pip_specs.iter().map(|spec| {
                crate::transaction::PlannedLink {
                    name: crate::spec::package_name_from_spec(spec)
                        .unwrap_or_else(|_| spec.clone()),
                    version: "unknown".to_string(),
                    build: "pip".to_string(),
                    build_number: 0,
                    dist_name: spec.clone(),
                    channel: "pypi".to_string(),
                    base_url: "https://pypi.org".to_string(),
                    url: String::new(),
                    md5: None,
                    sha256: None,
                    depends: Vec::new(),
                    platform: platform.clone(),
                    source: "pip".to_string(),
                }
            }));

            let tx = TransactionExecutor {
                operation: "create".to_string(),
                requested_specs: normalized.conda_specs.clone(),
                pip_specs: normalized.pip_specs.clone(),
                platform: platform.clone(),
                dry_run: config.dry_run,
                ensure_layout: true,
            };
            let outcome = tx.apply(&target_prefix, EnvironmentState::empty(), &conda_plan)?;

            let mut result = OperationResult::ok(
                "environment created",
                json!({
                    "root_prefix": config.root_prefix,
                    "target_prefix": target_prefix,
                    "channels": normalized.channels,
                    "specs": normalized.conda_specs,
                    "pip_specs": normalized.pip_specs,
                    "changed": outcome.linked + outcome.unlinked + outcome.pip_changed,
                    "actions": {
                        "link": link_actions,
                    },
                    "solver_trace": if config.verbose >= 3 { Some(solved.trace) } else { None },
                    "dry_run": config.dry_run,
                }),
            );
            result.warnings = normalized.warnings;
            Ok(result)
        }
        CliOperation::Install { specs, files } => {
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

            let normalized = normalize_request_inputs(
                specs,
                files,
                &config.channels,
                &globals.channels,
                globals.name.as_deref(),
            )?;
            let state = EnvironmentState::load(&target_prefix)?;
            let mut solve_specs = state.conda_locked_specs();
            solve_specs.extend(normalized.conda_specs.clone());
            let solve_specs = dedup_specs(solve_specs);

            let repodata_filename = select_repodata_filename(&solve_specs);
            let repodata = if solve_specs.is_empty() {
                Vec::new()
            } else {
                let mut repodata = fetch_packages(
                    &normalized.channels,
                    &current_platform_subdir(),
                    config.offline,
                    &config.root_prefix.join("pkgs").join("cache"),
                    config.local_repodata_ttl,
                    repodata_filename,
                )?;
                inject_installed_candidates(&mut repodata, &state.conda_packages());
                repodata
            };
            let solve_options = SolveOptions {
                channels: normalized.channels.clone(),
                strict_channel_priority: config.channel_priority == "strict",
                installed_preferred: state
                    .conda_packages()
                    .into_iter()
                    .map(|pkg| (pkg.name, (pkg.version, pkg.build_string)))
                    .collect(),
                user_requested: requested_names(&normalized.conda_specs),
            };
            let solved = solve_to_actions(&solve_specs, &repodata, &solve_options)
                .map_err(CoreError::UnsatisfiedSpecs)?;
            let conda_link_actions = solved.actions;
            let conda_plan = TransactionPlan::from_solved(&state.packages, &conda_link_actions);
            let platform = current_platform_subdir();
            let mut link_actions = conda_plan.link.clone();
            link_actions.extend(normalized.pip_specs.iter().map(|spec| {
                crate::transaction::PlannedLink {
                    name: crate::spec::package_name_from_spec(spec)
                        .unwrap_or_else(|_| spec.clone()),
                    version: "unknown".to_string(),
                    build: "pip".to_string(),
                    build_number: 0,
                    dist_name: spec.clone(),
                    channel: "pypi".to_string(),
                    base_url: "https://pypi.org".to_string(),
                    url: String::new(),
                    md5: None,
                    sha256: None,
                    depends: Vec::new(),
                    platform: platform.clone(),
                    source: "pip".to_string(),
                }
            }));

            let tx = TransactionExecutor {
                operation: "install".to_string(),
                requested_specs: normalized.conda_specs.clone(),
                pip_specs: normalized.pip_specs.clone(),
                platform: platform.clone(),
                dry_run: config.dry_run,
                ensure_layout: false,
            };
            let outcome = tx.apply(&target_prefix, state, &conda_plan)?;

            let mut result = OperationResult::ok(
                "packages installed",
                json!({
                    "target_prefix": target_prefix,
                    "changed": outcome.linked + outcome.unlinked + outcome.pip_changed,
                    "specs": normalized.conda_specs,
                    "pip_specs": normalized.pip_specs,
                    "actions": {
                        "link": link_actions,
                        "unlink": conda_plan.unlink,
                    },
                    "solver_trace": if config.verbose >= 3 { Some(solved.trace) } else { None },
                    "dry_run": config.dry_run,
                }),
            );
            result.warnings = normalized.warnings;
            Ok(result)
        }
        CliOperation::Remove {
            specs,
            all,
            force,
            no_prune_deps,
        } => {
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

            let specs = normalize_and_validate_match_specs(specs)?;
            let state = EnvironmentState::load(&target_prefix)?;
            let mut preview = state.clone();
            let removed = if force {
                preview.force_remove_specs(&specs)?
            } else {
                let mut keep_requested = EnvironmentState::requested_specs_map(&target_prefix)?
                    .into_keys()
                    .collect::<HashSet<_>>();
                for spec in &specs {
                    let name = package_name_from_spec(spec)?;
                    keep_requested.remove(&name);
                }
                preview.remove_specs(&specs, !no_prune_deps, &keep_requested)?
            };
            let remove_plan = TransactionPlan {
                fetch: Vec::new(),
                extract: Vec::new(),
                link: Vec::new(),
                unlink: removed.clone(),
            };
            let tx = TransactionExecutor {
                operation: "remove".to_string(),
                requested_specs: specs.clone(),
                pip_specs: Vec::new(),
                platform: current_platform_subdir(),
                dry_run: config.dry_run,
                ensure_layout: false,
            };
            let _outcome = tx.apply(&target_prefix, state, &remove_plan)?;
            let removed_names = removed
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>();

            Ok(OperationResult::ok(
                "packages removed",
                json!({
                    "target_prefix": target_prefix,
                    "removed": removed.len(),
                    "removed_names": removed_names,
                    "specs": specs,
                    "actions": {
                        "unlink": removed,
                    },
                    "dry_run": config.dry_run,
                }),
            ))
        }
        CliOperation::List(list_options) => {
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

            let payload = if list_options.revisions {
                let revisions = EnvironmentState::revisions(&target_prefix).unwrap_or_default();
                json!({
                    "target_prefix": target_prefix,
                    "revisions": revisions,
                    "revisions_supported": true,
                })
            } else {
                let state = EnvironmentState::load(&target_prefix)?;
                let package_views = render_list_output(state.packages, &list_options)?;
                json!({
                    "target_prefix": target_prefix,
                    "packages": package_views,
                    "applied": {
                        "regex": list_options.regex,
                        "full_name": list_options.full_name,
                        "no_pip": list_options.no_pip,
                        "reverse": list_options.reverse,
                        "explicit": list_options.explicit,
                        "canonical": list_options.canonical,
                        "export": list_options.export,
                    },
                })
            };
            Ok(OperationResult::ok("packages listed", payload))
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
                    "envs_dirs": [config.root_prefix.join("envs")],
                    "package_cache": [config.root_prefix.join("pkgs")],
                    "user_config_files": [store.path()],
                    "base_environment": config.root_prefix,
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
                    "target_prefix": config.target_prefix,
                    "json": config.json,
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
    files: Vec<std::path::PathBuf>,
    base_channels: &[String],
    cli_channels: &[String],
    cli_name: Option<&str>,
) -> Result<NormalizedRequestInputs, CoreError> {
    let mut conda_specs = normalize_and_validate_match_specs(cli_specs)?;
    let mut pip_specs = Vec::new();
    let mut yaml_name = None;
    let mut yaml_file_stem = None;
    let mut yaml_channels = Vec::new();
    let mut warnings = Vec::new();
    let mut file_kind: Option<SpecFileKind> = None;

    for path in files {
        let parsed = parse_spec_file(&path)?;
        if let Some(kind) = file_kind {
            if kind != parsed.kind {
                return Err(CoreError::InvalidEnvironmentFile(format!(
                    "all --file inputs must have the same format, got mixed {:?} and {:?}",
                    kind, parsed.kind
                )));
            }
        } else {
            file_kind = Some(parsed.kind);
        }

        let env = parsed.env;
        conda_specs.extend(env.conda_specs);
        pip_specs.extend(env.pip_specs);
        yaml_channels.extend(env.channels);

        if let Some(name) = env.name {
            match yaml_name.as_ref() {
                Some(existing) if existing != &name => {
                    warnings.push(format!(
                        "ignoring environment name '{name}' from '{}' because '{}' is already selected",
                        path.display(),
                        existing
                    ));
                }
                None => yaml_name = Some(name),
                _ => {}
            }
        }

        if yaml_file_stem.is_none() {
            yaml_file_stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);
        }
    }

    maybe_warn_cli_name_override(cli_name, yaml_name.as_deref(), &mut warnings);
    for spec in &conda_specs {
        parse_match_spec(spec)?;
    }

    Ok(NormalizedRequestInputs {
        conda_specs,
        pip_specs,
        yaml_name,
        yaml_file_stem,
        channels: effective_channels(base_channels, cli_channels, &yaml_channels),
        warnings,
    })
}

fn normalize_and_validate_match_specs(specs: Vec<String>) -> Result<Vec<String>, CoreError> {
    specs
        .into_iter()
        .map(|spec| {
            let normalized = normalize_spec(&spec)?;
            parse_match_spec(&normalized)?;
            Ok(normalized)
        })
        .collect()
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
        return dedup_channels(
            cli_channels
                .iter()
                .chain(yaml_channels.iter())
                .chain(base_channels.iter()),
        );
    }
    if !yaml_channels.is_empty() {
        return dedup_channels(yaml_channels.iter().chain(base_channels.iter()));
    }
    base_channels.to_vec()
}

fn maybe_warn_cli_name_override(
    cli_name: Option<&str>,
    yaml_name: Option<&str>,
    warnings: &mut Vec<String>,
) {
    if let (Some(cli_name), Some(yaml_name)) = (cli_name, yaml_name)
        && cli_name != yaml_name
    {
        warnings.push(format!(
            "ignoring environment name '{yaml_name}' from env file because '--name {cli_name}' is set"
        ));
    }
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

fn render_list_output(
    mut packages: Vec<PackageRecord>,
    options: &ListOptions,
) -> Result<serde_json::Value, CoreError> {
    if options.explicit && options.md5 && options.sha256 {
        return Err(CoreError::InvalidListOptions(
            "only one of --md5 and --sha256 can be specified".to_string(),
        ));
    }

    if options.no_pip {
        packages.retain(|p| p.source != "pip");
    }

    if let Some(pattern) = options.regex.as_deref() {
        packages.retain(|p| package_name_matches(&p.name, pattern, options.full_name));
    }

    if options.reverse {
        packages.reverse();
    }

    if options.explicit {
        let lines = packages
            .iter()
            .map(|pkg| {
                let mut line = package_url(pkg);
                if options.md5 {
                    if let Some(md5) = pkg.md5.as_deref() {
                        line.push('#');
                        line.push_str(md5);
                    }
                } else if options.sha256
                    && let Some(sha256) = pkg.sha256.as_deref()
                {
                    line.push('#');
                    line.push_str(sha256);
                }
                line
            })
            .collect::<Vec<_>>();
        return Ok(json!(lines));
    }

    if options.canonical {
        let rows = packages
            .iter()
            .map(|pkg| {
                format!(
                    "{}::{}-{}-{}",
                    package_channel(pkg),
                    pkg.name,
                    package_version(pkg),
                    package_build_string(pkg)
                )
            })
            .collect::<Vec<_>>();
        return Ok(json!(rows));
    }

    if options.export {
        let rows = packages
            .iter()
            .map(|pkg| {
                format!(
                    "{}={}={}",
                    pkg.name,
                    package_version(pkg),
                    package_build_string(pkg)
                )
            })
            .collect::<Vec<_>>();
        return Ok(json!(rows));
    }

    let rows = packages
        .iter()
        .map(|pkg| {
            json!({
                "name": pkg.name,
                "spec": pkg.spec,
                "source": pkg.source,
                "installed_at": pkg.installed_at,
                "version": package_version(pkg),
                "dist_name": pkg.dist_name.clone(),
                "build_string": package_build_string(pkg),
                "build_number": pkg.build_number,
                "channel": package_channel(pkg),
                "base_url": package_base_url(pkg),
                "url": package_url(pkg),
                "md5": pkg.md5.clone(),
                "sha256": pkg.sha256.clone(),
                "depends": pkg.depends.clone(),
                "platform": pkg.platform,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!(rows))
}

fn package_name_matches(name: &str, pattern: &str, full_name: bool) -> bool {
    let resolved = if full_name {
        format!("^(?:{pattern})$")
    } else {
        pattern.to_string()
    };
    Regex::new(&resolved).is_ok_and(|re| re.is_match(name))
}

fn package_version(pkg: &PackageRecord) -> String {
    pkg.version.clone()
}

fn package_build_string(pkg: &PackageRecord) -> String {
    pkg.build_string.clone()
}

fn package_channel(pkg: &PackageRecord) -> String {
    pkg.channel.clone()
}

fn package_base_url(pkg: &PackageRecord) -> String {
    pkg.base_url.clone()
}

fn package_url(pkg: &PackageRecord) -> String {
    pkg.url.clone()
}

fn dedup_specs(specs: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for spec in specs {
        if !out.iter().any(|existing| existing == &spec) {
            out.push(spec);
        }
    }
    out
}

fn requested_names(specs: &[String]) -> HashSet<String> {
    specs
        .iter()
        .filter_map(|spec| crate::spec::package_name_from_spec(spec).ok())
        .collect()
}

fn inject_installed_candidates(repodata: &mut Vec<RepoPackage>, installed: &[PackageRecord]) {
    for pkg in installed {
        if repodata.iter().any(|candidate| {
            candidate.name == pkg.name
                && candidate.version == pkg.version
                && candidate.build == pkg.build_string
        }) {
            continue;
        }
        repodata.push(package_record_to_repo_package(pkg));
    }
}

fn package_record_to_repo_package(record: &PackageRecord) -> RepoPackage {
    let filename = record
        .url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let dist = if record.dist_name.is_empty() {
                format!("{}-{}-{}", record.name, record.version, record.build_string)
            } else {
                record.dist_name.clone()
            };
            format!("{dist}.tar.bz2")
        });
    RepoPackage {
        name: record.name.clone(),
        version: record.version.clone(),
        build: record.build_string.clone(),
        build_number: record.build_number,
        subdir: record.platform.clone(),
        filename: filename.clone(),
        depends: record.depends.clone(),
        constrains: Vec::new(),
        md5: record.md5.clone(),
        sha256: record.sha256.clone(),
        channel: record.channel.clone(),
        base_url: record.base_url.clone(),
        url: if record.url.is_empty() {
            format!(
                "{}/{}/{}",
                record.base_url.trim_end_matches('/'),
                record.platform,
                filename
            )
        } else {
            record.url.clone()
        },
    }
}

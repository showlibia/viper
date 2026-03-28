use std::collections::{HashMap, HashSet};
use std::process::Command;

use regex::Regex;
use serde_json::json;

use crate::config::{ConfigInput, ConfigStore, build_config, name_to_target_prefix};
use crate::error::CoreError;
use crate::repodata::{RepoPackage, RepodataSource, fetch_packages};
use crate::solver::{
    SolveOptions, SolveRequest, SolveResult, production_solver_engine,
    spec_requires_full_repodata,
};
use crate::spec::{
    SpecFileKind, normalize_spec, package_name_from_spec, parse_explicit_url, parse_match_spec,
    parse_spec_source,
};
use crate::state::{EnvironmentState, is_managed_prefix};
use crate::transaction::{
    PlannedExtract, PlannedFetch, PlannedLink, TransactionExecutor, TransactionPlan,
};
use crate::types::{
    CliConfigCommand, CliOperation, ListOptions, OperationRequest, OperationResult, PackageRecord,
};

struct NormalizedRequestInputs {
    conda_specs: Vec<String>,
    explicit_specs: Vec<String>,
    explicit_mode: bool,
    pip_specs: Vec<String>,
    yaml_name: Option<String>,
    channels: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileSpecKindGroup {
    Yaml,
    NonYaml,
    Lock,
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
            )?;
            if globals.print_config_only {
                let mut result = OperationResult::ok(
                    "config rendered",
                    render_normalized_request_view(
                        "create",
                        &target_prefix,
                        &normalized,
                        config.dry_run,
                    ),
                );
                result.warnings = normalized.warnings;
                return Ok(result);
            }
            let (conda_link_actions, solver_trace) = if normalized.explicit_mode {
                (
                    explicit_specs_to_links(&normalized.explicit_specs)?,
                    None::<Vec<String>>,
                )
            } else {
                let repodata_source = select_repodata_source(&normalized.conda_specs);
                let repodata = if normalized.conda_specs.is_empty() {
                    Vec::new()
                } else {
                    fetch_packages(
                        &normalized.channels,
                        &current_platform_subdir(),
                        config.offline,
                        &config.root_prefix.join("pkgs").join("cache"),
                        config.local_repodata_ttl,
                        repodata_source,
                    )?
                };
                let solve_request = build_solve_request(
                    normalized.conda_specs.clone(),
                    normalized.conda_specs.clone(),
                    repodata,
                    normalized.channels.clone(),
                    config.channel_priority == "strict",
                    HashMap::new(),
                );
                let solved = solve_with_production_entry(&solve_request)?;
                (solved.actions, Some(solved.trace))
            };
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
                requested_specs: if normalized.explicit_mode {
                    normalized.explicit_specs.clone()
                } else {
                    normalized.conda_specs.clone()
                },
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
                    "specs": if normalized.explicit_mode { normalized.explicit_specs.clone() } else { normalized.conda_specs.clone() },
                    "pip_specs": normalized.pip_specs,
                    "changed": outcome.linked + outcome.unlinked + outcome.pip_changed,
                    "actions": {
                        "fetch": conda_plan.fetch,
                        "extract": conda_plan.extract,
                        "link": link_actions,
                        "unlink": conda_plan.unlink,
                    },
                    "solver_trace": if config.verbose >= 3 { solver_trace } else { None },
                    "dry_run": config.dry_run,
                }),
            );
            result.warnings = normalized.warnings;
            Ok(result)
        }
        CliOperation::Install { specs, files } => {
            let normalized = normalize_request_inputs(
                specs,
                files,
                &config.channels,
                &globals.channels,
                globals.name.as_deref(),
            )?;
            let target_prefix = resolve_install_target_prefix(
                &globals,
                &config.root_prefix,
                config.target_prefix.clone(),
                normalized.yaml_name.as_deref(),
                globals.print_config_only,
            )?;
            if globals.print_config_only {
                let mut result = OperationResult::ok(
                    "config rendered",
                    render_normalized_request_view(
                        "install",
                        &target_prefix,
                        &normalized,
                        config.dry_run,
                    ),
                );
                result.warnings = normalized.warnings;
                return Ok(result);
            }
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
            let (conda_link_actions, solver_trace) = if normalized.explicit_mode {
                (
                    explicit_specs_to_links(&normalized.explicit_specs)?,
                    None::<Vec<String>>,
                )
            } else {
                let mut solve_specs = state.conda_locked_specs();
                solve_specs.extend(normalized.conda_specs.clone());
                let solve_specs = dedup_specs(solve_specs);

                let repodata_source = select_repodata_source(&solve_specs);
                let repodata = if solve_specs.is_empty() {
                    Vec::new()
                } else {
                    let mut repodata = fetch_packages(
                        &normalized.channels,
                        &current_platform_subdir(),
                        config.offline,
                        &config.root_prefix.join("pkgs").join("cache"),
                        config.local_repodata_ttl,
                        repodata_source,
                    )?;
                    inject_installed_candidates(&mut repodata, &state.conda_packages());
                    repodata
                };
                let solve_request = build_solve_request(
                    solve_specs,
                    normalized.conda_specs.clone(),
                    repodata,
                    normalized.channels.clone(),
                    config.channel_priority == "strict",
                    state
                        .conda_packages()
                        .into_iter()
                        .map(|pkg| (pkg.name, (pkg.version, pkg.build_string)))
                        .collect(),
                );
                let solved = solve_with_production_entry(&solve_request)?;
                (solved.actions, Some(solved.trace))
            };
            let conda_plan = if normalized.explicit_mode {
                transaction_plan_for_explicit_install(&state.packages, &conda_link_actions)
            } else {
                TransactionPlan::from_solved(&state.packages, &conda_link_actions)
            };
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
                requested_specs: if normalized.explicit_mode {
                    normalized.explicit_specs.clone()
                } else {
                    normalized.conda_specs.clone()
                },
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
                    "specs": if normalized.explicit_mode { normalized.explicit_specs.clone() } else { normalized.conda_specs.clone() },
                    "pip_specs": normalized.pip_specs,
                    "actions": {
                        "fetch": conda_plan.fetch,
                        "extract": conda_plan.extract,
                        "link": link_actions,
                        "unlink": conda_plan.unlink,
                    },
                    "solver_trace": if config.verbose >= 3 { solver_trace } else { None },
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
            let mut specs = normalize_and_validate_match_specs(specs)?;
            if globals.print_config_only {
                return Ok(OperationResult::ok(
                    "config rendered",
                    json!({
                        "operation": "remove",
                        "target_prefix": target_prefix,
                        "specs": specs,
                        "all": all,
                        "force": force,
                        "no_prune_deps": no_prune_deps,
                        "dry_run": config.dry_run,
                    }),
                ));
            }
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
            if all {
                specs = state
                    .packages
                    .iter()
                    .map(|pkg| pkg.name.clone())
                    .collect::<Vec<_>>();
                specs.sort();
                specs.dedup();
            }
            let mut preview = state.clone();
            let removed = if all || force {
                preview.force_remove_specs(&specs)?
            } else if no_prune_deps {
                let keep_requested = EnvironmentState::requested_specs_map(&target_prefix)?
                    .into_keys()
                    .collect::<HashSet<_>>();
                preview.remove_specs(&specs, false, &keep_requested)?
            } else {
                let removal_preview = preview.remove_specs(&specs, false, &HashSet::new())?;
                let removal_names = removal_preview
                    .iter()
                    .map(|item| item.name.clone())
                    .collect::<HashSet<_>>();
                let mut keep_requested = EnvironmentState::requested_specs_map(&target_prefix)?
                    .into_iter()
                    .collect::<HashMap<_, _>>();
                for name in removal_names {
                    keep_requested.remove(&name);
                }
                let solve_specs = if keep_requested.is_empty() {
                    fallback_keep_specs_without_requested_map(&state, &removal_preview)
                } else {
                    dedup_specs(keep_requested.into_values().collect::<Vec<_>>())
                };
                let preview_non_conda_unlinks = removal_preview
                    .iter()
                    .filter(|item| item.source != "conda")
                    .cloned()
                    .collect::<Vec<_>>();
                let installed_conda = state.conda_packages();
                let mut repodata = Vec::new();
                inject_installed_candidates(&mut repodata, &installed_conda);
                let solve_request = build_solve_request(
                    solve_specs.clone(),
                    solve_specs,
                    repodata,
                    config.channels.clone(),
                    config.channel_priority == "strict",
                    installed_conda
                        .into_iter()
                        .map(|pkg| (pkg.name, (pkg.version, pkg.build_string)))
                        .collect(),
                );
                let solved = solve_with_production_entry(&solve_request)?;
                let solved_plan = TransactionPlan::from_solved(&state.packages, &solved.actions);
                let mut unlink = solved_plan.unlink;
                for planned in preview_non_conda_unlinks {
                    if !unlink
                        .iter()
                        .any(|item| item.name == planned.name && item.source == planned.source)
                    {
                        unlink.push(planned);
                    }
                }
                unlink.sort_by(|a, b| a.name.cmp(&b.name));
                unlink
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
                    "removed_all": all,
                    "specs": specs,
                    "actions": {
                        "fetch": remove_plan.fetch,
                        "extract": remove_plan.extract,
                        "link": remove_plan.link,
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

            let (payload, warnings) = if list_options.revisions {
                let mut warnings = Vec::new();
                if list_options.explicit {
                    warnings.push(
                        "Option --explicit ignored because --revisions was also provided."
                            .to_string(),
                    );
                }
                if list_options.canonical {
                    warnings.push(
                        "Option --canonical ignored because --revisions was also provided."
                            .to_string(),
                    );
                }
                if list_options.export {
                    warnings.push(
                        "Option --export ignored because --revisions was also provided."
                            .to_string(),
                    );
                }
                let revisions = EnvironmentState::revisions(&target_prefix)?;
                (
                    json!({
                        "target_prefix": target_prefix,
                        "revisions": revisions,
                        "revisions_supported": true,
                    }),
                    warnings,
                )
            } else {
                let state =
                    EnvironmentState::load_with_pip_discovery(&target_prefix, !list_options.no_pip)?;
                let rendered = render_list_output(state.packages, &list_options)?;
                (
                    json!({
                        "target_prefix": target_prefix,
                        "packages": rendered.payload,
                        "applied": {
                            "regex": list_options.regex,
                            "full_name": list_options.full_name,
                            "no_pip": list_options.no_pip,
                            "reverse": list_options.reverse,
                            "explicit": list_options.explicit,
                            "canonical": list_options.canonical,
                            "export": list_options.export,
                        },
                    }),
                    rendered.warnings,
                )
            };
            let mut result = OperationResult::ok("packages listed", payload);
            result.warnings = warnings;
            Ok(result)
        }
        CliOperation::Info => {
            let info_target_prefix = config.target_prefix.as_ref().unwrap_or(&config.root_prefix);
            let env_exists = info_target_prefix.exists();
            let (environment, env_location) =
                info_environment_status(Some(info_target_prefix), &config.root_prefix);
            let has_populated_config = if globals.no_rc {
                false
            } else {
                store.has_populated_values()?
            };
            let populated_config_files = if has_populated_config {
                vec![store.path().to_path_buf()]
            } else {
                Vec::<std::path::PathBuf>::new()
            };
            let channel_urls = expanded_channel_urls(&config.channels, &current_platform_subdir());
            Ok(OperationResult::ok(
                "environment info",
                json!({
                    "libmamba version": env!("CARGO_PKG_VERSION"),
                    "mamba version": env!("CARGO_PKG_VERSION"),
                    "curl version": "unknown",
                    "libarchive version": "unknown",
                    "root_prefix": config.root_prefix,
                    "target_prefix": info_target_prefix,
                    "channels": channel_urls,
                    "channel_priority": config.channel_priority,
                    "offline": config.offline,
                    "local_repodata_ttl": config.local_repodata_ttl,
                    "json": config.json,
                    "env_exists": env_exists,
                    "environment": environment,
                    "env location": env_location,
                    "envs_dirs": [config.root_prefix.join("envs")],
                    "envs directories": [config.root_prefix.join("envs")],
                    "package_cache": [config.root_prefix.join("pkgs")],
                    "package cache": [config.root_prefix.join("pkgs")],
                    "user_config_files": [store.path()],
                    "user config files": [store.path()],
                    "populated config files": populated_config_files,
                    "virtual packages": current_virtual_packages(),
                    "base_environment": config.root_prefix,
                    "base environment": config.root_prefix,
                    "platform": current_platform_subdir(),
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
    files: Vec<String>,
    base_channels: &[String],
    cli_channels: &[String],
    cli_name: Option<&str>,
) -> Result<NormalizedRequestInputs, CoreError> {
    let cli_specs_len = cli_specs.len();
    let mut conda_specs = cli_specs;
    let mut explicit_specs = Vec::new();
    let mut explicit_mode = false;
    let mut pip_specs = Vec::new();
    let mut yaml_name = None;
    let mut yaml_channels = Vec::new();
    let mut warnings = Vec::new();
    let mut file_kind_group: Option<FileSpecKindGroup> = None;
    let mut parsed_files = Vec::new();

    for source in files {
        let parsed = parse_spec_source(&source)?;
        let current_group = match parsed.kind {
            SpecFileKind::Yaml => FileSpecKindGroup::Yaml,
            SpecFileKind::Lock => FileSpecKindGroup::Lock,
            SpecFileKind::Classic | SpecFileKind::Explicit => FileSpecKindGroup::NonYaml,
        };
        if let Some(expected) = file_kind_group {
            if expected != current_group {
                return Err(CoreError::InvalidEnvironmentFile(format!(
                    "all --file inputs must have the same format group (YAML, lockfile, or non-YAML), got mixed types at '{}'",
                    source
                )));
            }
        } else {
            file_kind_group = Some(current_group);
        }
        parsed_files.push((source, parsed));
    }

    for (source, parsed) in parsed_files {
        if parsed.kind == SpecFileKind::Explicit {
            explicit_mode = true;
            explicit_specs = parsed.env.conda_specs;
            conda_specs.clear();
            pip_specs.clear();
            warnings.push(format!(
                "explicit spec file '{}' switches request into explicit mode",
                source
            ));
            break;
        }
        if parsed.kind == SpecFileKind::Lock {
            explicit_mode = true;
            explicit_specs = parsed.env.conda_specs;
            conda_specs.clear();
            pip_specs = parsed.env.pip_specs;
            warnings.push(format!(
                "lockfile '{}' switches request into locked explicit mode",
                source
            ));
            continue;
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
                        source,
                        existing
                    ));
                }
                None => yaml_name = Some(name),
                _ => {}
            }
        }
    }

    maybe_warn_cli_name_override(cli_name, yaml_name.as_deref(), &mut warnings);
    if !explicit_mode {
        for spec in &conda_specs {
            parse_match_spec(spec)?;
        }
    }

    if !explicit_mode && cli_specs_len > 0 {
        let mut validated_cli = normalize_and_validate_match_specs(
            conda_specs.iter().take(cli_specs_len).cloned().collect(),
        )?;
        validated_cli.extend(conda_specs.into_iter().skip(cli_specs_len));
        conda_specs = validated_cli;
    }

    Ok(NormalizedRequestInputs {
        conda_specs,
        explicit_specs,
        explicit_mode,
        pip_specs,
        yaml_name,
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

fn explicit_specs_to_links(specs: &[String]) -> Result<Vec<PlannedLink>, CoreError> {
    specs
        .iter()
        .map(|spec| explicit_spec_to_link(spec))
        .collect::<Result<Vec<_>, _>>()
}

fn explicit_spec_to_link(spec: &str) -> Result<PlannedLink, CoreError> {
    let info = parse_explicit_url(spec)?;
    let (md5, sha256) = if info.fragment.as_ref().is_some_and(|hash| hash.len() == 32) {
        (info.fragment.clone(), None)
    } else if info.fragment.as_ref().is_some_and(|hash| hash.len() == 64) {
        (None, info.fragment.clone())
    } else {
        (None, None)
    };

    Ok(PlannedLink {
        name: info.name,
        version: info.version,
        build: info.build,
        build_number: 0,
        dist_name: info.dist,
        channel: info.base_url.clone(),
        base_url: info.base_url,
        url: info.url_no_fragment,
        md5,
        sha256,
        depends: Vec::new(),
        platform: info.subdir,
        source: "conda".to_string(),
    })
}

fn transaction_plan_for_explicit_install(
    installed: &[PackageRecord],
    explicit_links: &[PlannedLink],
) -> TransactionPlan {
    let target_names = explicit_links
        .iter()
        .map(|link| link.name.clone())
        .collect::<HashSet<_>>();

    let mut unlink = Vec::new();
    for installed_pkg in installed.iter().filter(|pkg| pkg.source == "conda") {
        if !target_names.contains(&installed_pkg.name) {
            continue;
        }
        let keep_same = explicit_links.iter().any(|link| {
            link.name == installed_pkg.name
                && link.version == installed_pkg.version
                && link.build == installed_pkg.build_string
        });
        if !keep_same {
            unlink.push(crate::transaction::PlannedUnlink {
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

    let mut link = Vec::new();
    for solved_pkg in explicit_links {
        let same = installed
            .iter()
            .filter(|pkg| pkg.source == "conda")
            .find(|pkg| pkg.name == solved_pkg.name)
            .is_some_and(|installed_pkg| {
                installed_pkg.version == solved_pkg.version
                    && installed_pkg.build_string == solved_pkg.build
            });
        if !same {
            link.push(solved_pkg.clone());
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

    TransactionPlan {
        fetch,
        extract,
        link,
        unlink,
    }
}

fn render_normalized_request_view(
    operation: &str,
    target_prefix: &std::path::Path,
    normalized: &NormalizedRequestInputs,
    dry_run: bool,
) -> serde_json::Value {
    json!({
        "operation": operation,
        "target_prefix": target_prefix,
        "dry_run": dry_run,
        "explicit_mode": normalized.explicit_mode,
        "specs": if normalized.explicit_mode { normalized.explicit_specs.clone() } else { normalized.conda_specs.clone() },
        "pip_specs": normalized.pip_specs,
        "channels": normalized.channels,
        "yaml_name": normalized.yaml_name,
    })
}

fn resolve_create_target_prefix(
    globals: &crate::types::CliGlobalOptions,
    root_prefix: &std::path::Path,
    yaml_name: Option<&str>,
) -> Result<std::path::PathBuf, CoreError> {
    globals
        .prefix
        .clone()
        .or_else(|| {
            globals
                .name
                .as_ref()
                .map(|name| name_to_target_prefix(root_prefix, name))
        })
        .or_else(|| yaml_name.map(|name| name_to_target_prefix(root_prefix, name)))
        .or_else(|| {
            std::env::var_os("VIPER_TARGET_PREFIX")
                .or_else(|| std::env::var_os("CONDA_PREFIX"))
                .map(std::path::PathBuf::from)
        })
        .ok_or(CoreError::MissingTargetPrefix)
}

fn resolve_install_target_prefix(
    globals: &crate::types::CliGlobalOptions,
    root_prefix: &std::path::Path,
    config_target_prefix: Option<std::path::PathBuf>,
    yaml_name: Option<&str>,
    print_config_only: bool,
) -> Result<std::path::PathBuf, CoreError> {
    globals
        .prefix
        .clone()
        .or_else(|| {
            globals
                .name
                .as_ref()
                .map(|name| name_to_target_prefix(root_prefix, name))
        })
        .or_else(|| yaml_name.map(|name| name_to_target_prefix(root_prefix, name)))
        .or(config_target_prefix)
        .or_else(|| print_config_only.then(|| root_prefix.to_path_buf()))
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
    platform_subdir(std::env::consts::OS, std::env::consts::ARCH)
}

fn platform_subdir(os: &str, arch: &str) -> String {
    match (os, arch) {
        ("linux", "x86_64") => "linux-64".to_string(),
        ("linux", "x86") | ("linux", "i686") => "linux-32".to_string(),
        ("linux", "aarch64") | ("linux", "arm64") => "linux-aarch64".to_string(),
        ("linux", "armv7") | ("linux", "armv7l") => "linux-armv7l".to_string(),
        ("linux", "armv6") | ("linux", "armv6l") => "linux-armv6l".to_string(),
        ("linux", "riscv64") => "linux-riscv64".to_string(),
        ("macos", "x86_64") => "osx-64".to_string(),
        ("macos", "aarch64") | ("macos", "arm64") => "osx-arm64".to_string(),
        ("windows", "x86_64") => "win-64".to_string(),
        ("windows", "aarch64") | ("windows", "arm64") => "win-arm64".to_string(),
        ("windows", "x86") | ("windows", "i686") => "win-32".to_string(),
        _ => format!("{os}-{arch}"),
    }
}

fn current_virtual_packages() -> Vec<String> {
    let os = std::env::consts::OS;
    let platform = current_platform_subdir();
    let archspec = std::env::var("CONDA_OVERRIDE_ARCHSPEC")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| archspec_for_platform(&platform));
    let mut packages = Vec::new();
    match os {
        "linux" => {
            packages.push("__unix=0=0".to_string());
            let linux_version = std::env::var("CONDA_OVERRIDE_LINUX")
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(probe_linux_kernel_version)
                .unwrap_or_else(|| "0".to_string());
            packages.push(format!("__linux={linux_version}=0"));
            if let Some(version) = std::env::var("CONDA_OVERRIDE_GLIBC")
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(probe_glibc_version)
            {
                packages.push(format!("__glibc={version}=0"));
            }
        }
        "macos" => {
            packages.push("__unix=0=0".to_string());
            let osx_version = std::env::var("CONDA_OVERRIDE_OSX")
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(probe_macos_version)
                .unwrap_or_else(|| "0".to_string());
            packages.push(format!("__osx={osx_version}=0"));
        }
        "windows" => {
            if let Some(version) = std::env::var("CONDA_OVERRIDE_WIN")
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(probe_windows_version)
            {
                packages.push(format!("__win={version}=0"));
            } else {
                packages.push("__win=0=0".to_string());
            }
        }
        _ => {}
    }
    packages.push(format!("__archspec=1={archspec}"));
    if let Some(version) = probe_cuda_version() {
        packages.push(format!("__cuda={version}=0"));
    }
    packages
}

fn archspec_for_platform(platform: &str) -> String {
    if matches!(platform, "linux-64" | "osx-64" | "win-64") {
        return detect_x86_64_archspec();
    }
    if platform == "linux-arm64" || platform.ends_with("aarch64") {
        return "aarch64".to_string();
    }
    if platform.ends_with("arm64") {
        return "arm64".to_string();
    }
    if platform.ends_with("32") {
        return "x86".to_string();
    }
    if platform.ends_with("ppc64le")
        || platform.ends_with("s390x")
        || platform.ends_with("riscv64")
        || platform.ends_with("armv7l")
        || platform.ends_with("armv6l")
    {
        return platform
            .rsplit('-')
            .next()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| platform.to_string());
    }
    platform
        .rsplit('-')
        .next()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| platform.to_string())
}

fn detect_x86_64_archspec() -> String {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx512f")
            && std::is_x86_feature_detected!("avx512bw")
            && std::is_x86_feature_detected!("avx512cd")
            && std::is_x86_feature_detected!("avx512dq")
            && std::is_x86_feature_detected!("avx512vl")
        {
            return "x86_64_v4".to_string();
        }
        if std::is_x86_feature_detected!("avx2")
            && std::is_x86_feature_detected!("bmi1")
            && std::is_x86_feature_detected!("bmi2")
            && std::is_x86_feature_detected!("fma")
        {
            return "x86_64_v3".to_string();
        }
        if std::is_x86_feature_detected!("popcnt")
            && std::is_x86_feature_detected!("sse4.2")
            && std::is_x86_feature_detected!("ssse3")
        {
            return "x86_64_v2".to_string();
        }
    }
    "x86_64".to_string()
}

fn probe_linux_kernel_version() -> Option<String> {
    probe_command_output("uname", &["-r"])
        .map(|release| release.split('-').next().unwrap_or(&release).to_string())
}

fn probe_glibc_version() -> Option<String> {
    let output = probe_command_output("getconf", &["GNU_LIBC_VERSION"])?;
    output.split_whitespace().last().map(ToOwned::to_owned)
}

fn probe_macos_version() -> Option<String> {
    probe_command_output("sw_vers", &["-productVersion"])
}

fn probe_windows_version() -> Option<String> {
    let output = probe_command_output("cmd", &["/C", "ver"])?;
    parse_windows_version_from_ver_output(&output)
}

fn probe_cuda_version() -> Option<String> {
    if let Some(version) = std::env::var("CONDA_OVERRIDE_CUDA")
        .ok()
        .filter(|value| !value.is_empty())
    {
        return normalize_cuda_version(&version);
    }
    let output = probe_command_output(
        "nvidia-smi",
        &["--query-gpu=cuda_version", "--format=csv,noheader"],
    )?;
    let first_line = output.lines().next()?.trim();
    normalize_cuda_version(first_line)
}

fn normalize_cuda_version(value: &str) -> Option<String> {
    let version_re = Regex::new(r"(\d+)\.(\d+)").ok()?;
    let captures = version_re.captures(value)?;
    Some(format!("{}.{}", &captures[1], &captures[2]))
}

fn probe_command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_windows_version_from_ver_output(output: &str) -> Option<String> {
    let mut started = false;
    let mut value = String::new();
    for ch in output.chars() {
        if !started {
            if ch.is_ascii_digit() {
                started = true;
                value.push(ch);
            }
            continue;
        }
        if ch.is_ascii_digit() || ch == '.' {
            value.push(ch);
        } else {
            break;
        }
    }
    if value.is_empty() { None } else { Some(value) }
}

fn select_repodata_source(specs: &[String]) -> RepodataSource {
    if specs.iter().any(|spec| spec_requires_full_repodata(spec)) {
        RepodataSource::Full
    } else {
        RepodataSource::Current
    }
}

fn solve_with_production_entry(
    request: &SolveRequest,
) -> Result<SolveResult, CoreError> {
    let _engine = production_solver_engine();
    request.solve().map_err(CoreError::UnsatisfiedSpecs)
}

fn build_solve_request(
    solve_specs: Vec<String>,
    user_requested_specs: Vec<String>,
    repodata: Vec<RepoPackage>,
    channels: Vec<String>,
    strict_channel_priority: bool,
    installed_preferred: HashMap<String, (String, String)>,
) -> SolveRequest {
    SolveRequest {
        specs: solve_specs,
        repodata,
        options: SolveOptions {
            channels,
            strict_channel_priority,
            installed_preferred,
            user_requested: requested_names(&user_requested_specs),
        },
    }
}

struct RenderedListOutput {
    payload: serde_json::Value,
    warnings: Vec<String>,
}

fn render_list_output(
    mut packages: Vec<PackageRecord>,
    options: &ListOptions,
) -> Result<RenderedListOutput, CoreError> {
    let mut warnings = Vec::new();
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
        if options.canonical {
            warnings.push(
                "Option --canonical ignored because --explicit was also provided.".to_string(),
            );
        }
        if options.export {
            warnings
                .push("Option --export ignored because --explicit was also provided.".to_string());
        }
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
        return Ok(RenderedListOutput {
            payload: json!(lines),
            warnings,
        });
    }

    if options.canonical {
        if options.export {
            warnings
                .push("Option --export ignored because --canonical was also provided.".to_string());
        }
        let rows = packages
            .iter()
            .map(|pkg| {
                format!(
                    "{}/{}::{}-{}-{}",
                    package_channel(pkg),
                    pkg.platform,
                    pkg.name,
                    package_version(pkg),
                    package_build_string(pkg)
                )
            })
            .collect::<Vec<_>>();
        return Ok(RenderedListOutput {
            payload: json!(rows),
            warnings,
        });
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
        return Ok(RenderedListOutput {
            payload: json!(rows),
            warnings,
        });
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
    Ok(RenderedListOutput {
        payload: json!(rows),
        warnings,
    })
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
    format_channel_name(&pkg.channel)
}

fn format_channel_name(channel: &str) -> String {
    let trimmed = channel.trim_end_matches('/');
    let without_repodata = strip_repodata_suffix(trimmed);
    if without_repodata.starts_with("file://") {
        return strip_known_subdir_suffix(without_repodata);
    }
    if without_repodata.starts_with("https://") || without_repodata.starts_with("http://") {
        let without_scheme = without_repodata
            .strip_prefix("https://")
            .or_else(|| without_repodata.strip_prefix("http://"))
            .unwrap_or(without_repodata);
        if !without_scheme.starts_with("conda.anaconda.org/")
            && !without_scheme.starts_with("repo.anaconda.com/")
        {
            if should_strip_known_subdir_suffix(without_repodata) {
                return strip_known_subdir_suffix(without_repodata);
            }
            return without_repodata.to_string();
        }
    }
    let without_scheme = without_repodata
        .strip_prefix("https://")
        .or_else(|| without_repodata.strip_prefix("http://"))
        .unwrap_or(without_repodata);
    let display = without_scheme
        .strip_prefix("conda.anaconda.org/")
        .or_else(|| without_scheme.strip_prefix("repo.anaconda.com/"))
        .unwrap_or(without_scheme);
    let Some((prefix, suffix)) = display.rsplit_once('/') else {
        return display.to_string();
    };
    if is_known_conda_subdir(suffix) {
        prefix.to_string()
    } else {
        display.to_string()
    }
}

fn is_known_conda_subdir(value: &str) -> bool {
    matches!(
        value,
        "noarch"
            | "linux-32"
            | "linux-64"
            | "linux-armv6l"
            | "linux-armv7l"
            | "linux-aarch64"
            | "linux-ppc64le"
            | "linux-riscv64"
            | "linux-s390x"
            | "osx-64"
            | "osx-arm64"
            | "win-32"
            | "win-64"
            | "win-arm64"
    )
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

fn info_environment_status(
    target_prefix: Option<&std::path::PathBuf>,
    root_prefix: &std::path::Path,
) -> (String, String) {
    let Some(target) = target_prefix else {
        return ("None".to_string(), "-".to_string());
    };
    let envs_dir = root_prefix.join("envs");
    let mut name = if target == root_prefix {
        "base".to_string()
    } else if target
        .parent()
        .is_some_and(|parent| parent == envs_dir.as_path())
    {
        target
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| target.display().to_string())
    } else {
        target.display().to_string()
    };
    let target_display = target.display().to_string();
    if std::env::var_os("CONDA_PREFIX")
        .map(std::path::PathBuf::from)
        .is_some_and(|active| active == *target)
    {
        name.push_str(" (active)");
    } else if target.exists() {
        let is_env = target == root_prefix || target.join("conda-meta").exists();
        if !is_env {
            name.push_str(" (not env)");
        }
    } else {
        name.push_str(" (not found)");
    }
    (name, target_display)
}

fn expanded_channel_urls(channels: &[String], platform: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for channel in channels {
        let bases = normalize_channel_base_urls(channel, platform);
        for base in bases {
            if let Some((prefix, suffix)) = split_known_subdir_suffix(&base) {
                if suffix == platform {
                    for url in [base.clone(), format!("{prefix}/noarch")] {
                        if !urls.iter().any(|existing| existing == &url) {
                            urls.push(url);
                        }
                    }
                    continue;
                }
                if suffix == "noarch" {
                    for url in [format!("{prefix}/{platform}"), base.clone()] {
                        if !urls.iter().any(|existing| existing == &url) {
                            urls.push(url);
                        }
                    }
                    continue;
                }
            }
            for suffix in [platform, "noarch"] {
                let url = format!("{base}/{suffix}");
                if !urls.iter().any(|existing| existing == &url) {
                    urls.push(url);
                }
            }
        }
    }
    urls
}

fn normalize_channel_base_urls(channel: &str, platform: &str) -> Vec<String> {
    let trimmed = strip_repodata_suffix(channel.trim_end_matches('/'));
    if trimmed == "defaults" {
        let mut defaults = vec![
            "https://repo.anaconda.com/pkgs/main".to_string(),
            "https://repo.anaconda.com/pkgs/r".to_string(),
        ];
        if platform.starts_with("win-") {
            defaults.push("https://repo.anaconda.com/pkgs/msys2".to_string());
        }
        return defaults;
    }
    let normalized = if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
    {
        trimmed.to_string()
    } else if trimmed.starts_with("pkgs/") {
        format!("https://repo.anaconda.com/{trimmed}")
    } else {
        format!("https://conda.anaconda.org/{trimmed}")
    };
    let sanitized = strip_url_credentials(&normalized);
    if should_strip_known_subdir_suffix(&sanitized) {
        vec![strip_known_subdir_suffix(&sanitized)]
    } else {
        vec![sanitized]
    }
}

fn strip_repodata_suffix(value: &str) -> &str {
    value
        .trim_end_matches("/repodata.json")
        .trim_end_matches("/current_repodata.json")
}

fn strip_known_subdir_suffix(value: &str) -> String {
    let Some((prefix, suffix)) = value.rsplit_once('/') else {
        return value.to_string();
    };
    if is_known_conda_subdir(suffix) {
        prefix.to_string()
    } else {
        value.to_string()
    }
}

fn split_known_subdir_suffix(value: &str) -> Option<(&str, &str)> {
    let (prefix, suffix) = value.rsplit_once('/')?;
    if is_known_conda_subdir(suffix) {
        Some((prefix, suffix))
    } else {
        None
    }
}

fn should_strip_known_subdir_suffix(value: &str) -> bool {
    value.starts_with("file://")
        || value.starts_with("https://conda.anaconda.org/")
        || value.starts_with("http://conda.anaconda.org/")
        || value.starts_with("https://repo.anaconda.com/")
        || value.starts_with("http://repo.anaconda.com/")
}

fn strip_url_credentials(value: &str) -> String {
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return value.to_string();
    }
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_string();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let path = &rest[authority_end..];
    let sanitized_authority = authority
        .split_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    format!("{scheme}://{sanitized_authority}{path}")
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

fn fallback_keep_specs_without_requested_map(
    state: &EnvironmentState,
    removal_preview: &[crate::transaction::PlannedUnlink],
) -> Vec<String> {
    let removed_names = removal_preview
        .iter()
        .filter(|item| item.source == "conda")
        .map(|item| item.name.clone())
        .collect::<HashSet<_>>();

    let mut removal_closure = removed_names.clone();
    let mut queue = removed_names.into_iter().collect::<Vec<_>>();
    while let Some(name) = queue.pop() {
        let Some(pkg) = state
            .packages
            .iter()
            .find(|pkg| pkg.source == "conda" && pkg.name == name)
        else {
            continue;
        };
        for dep in pkg
            .depends
            .iter()
            .filter_map(|dep| package_name_from_spec(dep).ok())
        {
            if removal_closure.insert(dep.clone()) {
                queue.push(dep);
            }
        }
    }

    let keep_specs = state
        .packages
        .iter()
        .filter(|pkg| pkg.source == "conda" && !removal_closure.contains(&pkg.name))
        .map(|pkg| format!("{}=={}", pkg.name, pkg.version))
        .collect::<Vec<_>>();
    dedup_specs(keep_specs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_production_solver_entry_returns_expected_action() {
        let pkg = RepoPackage {
            name: "python".to_string(),
            version: "3.12.2".to_string(),
            build: "0".to_string(),
            build_number: 0,
            subdir: "linux-64".to_string(),
            filename: "python-3.12.2-0.conda".to_string(),
            depends: Vec::new(),
            constrains: Vec::new(),
            md5: None,
            sha256: None,
            channel: "https://conda.anaconda.org/conda-forge".to_string(),
            base_url: "https://conda.anaconda.org/conda-forge".to_string(),
            url: "https://conda.anaconda.org/conda-forge/linux-64/python-3.12.2-0.conda"
                .to_string(),
        };
        let opts = SolveOptions {
            channels: vec!["conda-forge".to_string()],
            strict_channel_priority: false,
            installed_preferred: HashMap::new(),
            user_requested: HashSet::from(["python".to_string()]),
        };
        let request = SolveRequest {
            specs: vec!["python>=3.11".to_string()],
            repodata: vec![pkg],
            options: opts,
        };
        let solved =
            solve_with_production_entry(&request).expect("solve via core production entry");
        assert_eq!(solved.actions.len(), 1);
        assert_eq!(solved.actions[0].name, "python");
    }

    #[test]
    fn build_solve_request_uses_same_user_requested_logic_for_create_and_install() {
        let channels = vec!["conda-forge".to_string()];
        let create_request = build_solve_request(
            vec!["python>=3.11".to_string()],
            vec!["python>=3.11".to_string()],
            Vec::new(),
            channels.clone(),
            false,
            HashMap::new(),
        );
        let install_request = build_solve_request(
            vec!["python>=3.11".to_string(), "numpy".to_string()],
            vec!["numpy".to_string()],
            Vec::new(),
            channels,
            false,
            HashMap::new(),
        );
        assert_eq!(
            create_request.options.user_requested,
            HashSet::from(["python".to_string()])
        );
        assert_eq!(
            install_request.options.user_requested,
            HashSet::from(["numpy".to_string()])
        );
    }

    #[test]
    fn build_solve_request_carries_remove_solver_preferences() {
        let mut installed_preferred = HashMap::new();
        installed_preferred.insert(
            "python".to_string(),
            ("3.12.2".to_string(), "0".to_string()),
        );
        let request = build_solve_request(
            vec!["python==3.12.2".to_string()],
            vec!["python==3.12.2".to_string()],
            Vec::new(),
            vec!["conda-forge".to_string()],
            true,
            installed_preferred.clone(),
        );
        assert!(request.options.strict_channel_priority);
        assert_eq!(request.options.installed_preferred, installed_preferred);
        assert_eq!(
            request.options.user_requested,
            HashSet::from(["python".to_string()])
        );
    }

    #[test]
    fn format_channel_name_preserves_custom_paths() {
        assert_eq!(
            format_channel_name("https://conda.anaconda.org/conda-forge/linux-64"),
            "conda-forge"
        );
        assert_eq!(
            format_channel_name("file:///tmp/local-channel/linux-64"),
            "file:///tmp/local-channel"
        );
        assert_eq!(
            format_channel_name("https://repo.anaconda.com/pkgs/main/linux-64"),
            "pkgs/main"
        );
        assert_eq!(
            format_channel_name("https://repo.example.com/team/conda/linux-64"),
            "https://repo.example.com/team/conda/linux-64"
        );
        assert_eq!(
            format_channel_name("https://repo.example.com/pkgs/main/noarch"),
            "https://repo.example.com/pkgs/main/noarch"
        );
        assert_eq!(
            format_channel_name("https://repo.example.com/noarch"),
            "https://repo.example.com/noarch"
        );
        assert_eq!(
            format_channel_name("https://repo.example.com/custom/channel"),
            "https://repo.example.com/custom/channel"
        );
    }

    #[test]
    fn expanded_channel_urls_normalize_existing_subdir_channels() {
        let urls = expanded_channel_urls(
            &[
                "conda-forge".to_string(),
                "https://conda.anaconda.org/conda-forge/linux-64".to_string(),
                "defaults".to_string(),
                "https://repo.example.com/team/conda/linux-64".to_string(),
            ],
            "linux-64",
        );
        assert_eq!(
            urls,
            vec![
                "https://conda.anaconda.org/conda-forge/linux-64".to_string(),
                "https://conda.anaconda.org/conda-forge/noarch".to_string(),
                "https://repo.anaconda.com/pkgs/main/linux-64".to_string(),
                "https://repo.anaconda.com/pkgs/main/noarch".to_string(),
                "https://repo.anaconda.com/pkgs/r/linux-64".to_string(),
                "https://repo.anaconda.com/pkgs/r/noarch".to_string(),
                "https://repo.example.com/team/conda/linux-64".to_string(),
                "https://repo.example.com/team/conda/noarch".to_string(),
            ]
        );
    }

    #[test]
    fn platform_subdir_maps_windows_arm64_and_known_conda_subdirs() {
        assert_eq!(platform_subdir("windows", "aarch64"), "win-arm64");
        assert_eq!(platform_subdir("linux", "arm"), "linux-arm");
        assert!(is_known_conda_subdir("linux-riscv64"));
    }

    #[test]
    fn parse_windows_version_from_ver_output_extracts_version() {
        assert_eq!(
            parse_windows_version_from_ver_output("Microsoft Windows [Version 10.0.22621.3007]"),
            Some("10.0.22621.3007".to_string())
        );
        assert_eq!(
            parse_windows_version_from_ver_output("Microsoft Windows [Версия 10.0.22621.3007]"),
            Some("10.0.22621.3007".to_string())
        );
    }

    #[test]
    fn archspec_for_platform_uses_platform_tokens() {
        assert_eq!(archspec_for_platform("win-arm64"), "arm64");
        assert_eq!(archspec_for_platform("linux-aarch64"), "aarch64");
        assert_eq!(archspec_for_platform("linux-arm64"), "aarch64");
        assert_eq!(archspec_for_platform("linux-riscv64"), "riscv64");
        assert!(archspec_for_platform("linux-64").starts_with("x86_64"));
    }

    #[test]
    fn strip_url_credentials_removes_userinfo() {
        assert_eq!(
            strip_url_credentials("https://user:token@repo.example.com/team"),
            "https://repo.example.com/team"
        );
        assert_eq!(
            strip_url_credentials("https://repo.example.com/team"),
            "https://repo.example.com/team"
        );
    }

    #[test]
    fn normalize_cuda_version_extracts_major_minor() {
        assert_eq!(normalize_cuda_version("12.4"), Some("12.4".to_string()));
        assert_eq!(
            normalize_cuda_version("CUDA Version: 11.8"),
            Some("11.8".to_string())
        );
        assert_eq!(normalize_cuda_version("unknown"), None);
    }
}

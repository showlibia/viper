# Viper vs Micromamba Compatibility Matrix

This document tracks command-level behavior alignment using upstream `mamba/` sources/tests as evidence and links each row to an enforcing `viper` test.

## Command Matrix

| Command | Behavior | Upstream reference | Viper enforcement |
|---|---|---|---|
| `create` | Reject conflicting `--prefix` + `--name` | `mamba/micromamba/tests/test_create.py` (target-prefix selection paths) | `crates/viper-cli/tests/cli_smoke.rs` `create_rejects_prefix_and_name_together` |
| `create` | `--dry-run` returns planned actions and avoids writes | `mamba/micromamba/tests/test_create.py#test_create_dry_run` | `crates/viper-cli/tests/cli_smoke.rs` `create_dry_run_returns_transaction_actions` |
| `create` | Env-file YAML name determines env name when provided | `mamba/micromamba/tests/test_create.py` env-file creation cases | `crates/viper-cli/tests/cli_smoke.rs` `create_from_env_file_uses_yaml_name_channels_and_pip_specs` |
| `create` | `--name` takes precedence over YAML `name` | `mamba/micromamba/tests/test_install.py` target-prefix precedence matrix | `crates/viper-cli/tests/cli_smoke.rs` `create_from_env_file_prefers_cli_name_over_yaml_name` |
| `create` | `--prefix` takes precedence over YAML `name` | `mamba/micromamba/tests/test_install.py` target-prefix precedence matrix | `crates/viper-cli/tests/cli_smoke.rs` `create_from_env_file_prefers_cli_prefix_over_yaml_name` |
| `create` | Multiple YAML `-f/--file` inputs merge dependencies in order | `mamba/micromamba/tests/test_create.py#test_multiple_yaml_specs` | `crates/viper-cli/tests/cli_smoke.rs` `create_merges_multiple_env_files_specs` |
| `create` | Conflicting YAML names across files keep the first name and emit warning | `mamba/micromamba/tests/test_create.py#test_multiple_yaml_specs_different_names` | `crates/viper-cli/tests/cli_smoke.rs` `create_multiple_env_files_keep_first_name_and_warn` |
| `create` | Repeated non-YAML spec files merge in order | `mamba/micromamba/tests/test_create.py#test_create_with_multiple_files` | Pending (`viper` currently only supports YAML env files) |
| `create` | YAML channels accumulate with rc/base channels | `mamba/libmamba/src/api/install.cpp` channel merge path; `mamba/micromamba/tests/test_create.py` channel precedence tests | `crates/viper-cli/tests/cli_smoke.rs` `create_accumulates_yaml_and_rc_channels_in_order` |
| `create` | Env-file without `name` falls back to file stem | `mamba/micromamba/tests/test_install.py` target-prefix/env-file precedence coverage | `crates/viper-cli/tests/cli_smoke.rs` `create_from_env_file_without_name_uses_file_stem_for_prefix` |
| `create` | Env-file `name` must be an env name, not a prefix path | `mamba/libmamba/src/api/configuration.cpp` `file_spec_env_name_hook`; `mamba/micromamba/tests/test_install.py` `yaml_name == "prefix"` failure path | `crates/viper-cli/tests/cli_smoke.rs` `create_from_env_file_rejects_name_with_path_separator` |
| `install` | Missing target prefix fails | `mamba/micromamba/tests/test_install.py` target-prefix checks (`MAMBA_NOT_ALLOW_MISSING_PREFIX`) | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_when_prefix_missing` |
| `install` | Non-managed prefix fails | `mamba/micromamba/tests/test_install.py` target-prefix checks (`MAMBA_NOT_ALLOW_NOT_ENV_PREFIX`) | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_for_unmanaged_prefix` |
| `install` | Env-file with prefix-like `name` is rejected | `mamba/micromamba/tests/test_install.py` `yaml_name == "prefix"` failure path | `crates/viper-cli/tests/cli_smoke.rs` `install_from_env_file_rejects_name_with_path_separator` |
| `install` | `--name` overrides YAML `name` and emits warning | `mamba/libmamba/src/api/install.cpp` env-name conflict warning path | `crates/viper-cli/tests/cli_smoke.rs` `install_prefers_cli_name_over_yaml_name_and_warns` |
| `install` | Pip-only env-file update preserves existing conda version/build state (no silent conda upgrade) | `mamba/libmamba/src/api/install.cpp` installed-prefix load + solve request path | `crates/viper-cli/tests/cli_smoke.rs` `install_pip_only_env_file_keeps_existing_conda_packages` |
| `remove` | Missing target prefix fails | `mamba/micromamba/src/remove.cpp` prefix-validation flow; `mamba/micromamba/tests/test_remove.py` target-prefix checks | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_when_prefix_missing` |
| `remove` | Non-managed prefix fails | `mamba/micromamba/src/remove.cpp` env-prefix guard; `mamba/micromamba/tests/test_remove.py` target-prefix checks | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_for_unmanaged_prefix` |
| `remove` | Removing missing package returns explicit error | `mamba/micromamba/tests/test_remove.py` remove error paths | `crates/viper-cli/tests/cli_smoke.rs` `remove_non_installed_package_fails` |
| `remove` | Default remove prunes dependent/orphaned packages to keep prefix consistent | `mamba/libmamba/src/api/remove.cpp` solver-backed remove request (`clean_dependencies=true`) | `crates/viper-cli/tests/cli_smoke.rs` `remove_dependency_also_removes_dependents` |
| `remove` | `--no-prune-deps` keeps orphan deps while removing the requested package set | `mamba/libmamba/src/api/remove.cpp` `clean_dependencies=false` path | `crates/viper-cli/tests/cli_smoke.rs` `remove_no_prune_deps_keeps_orphans` |
| `remove` | `--force` exposes unsafe removal path that does not enforce dependency consistency | `mamba/libmamba/src/api/remove.cpp` force branch; `mamba/micromamba/tests/test_remove.py` force coverage | `crates/viper-cli/tests/cli_smoke.rs` `remove_force_keeps_dependents_in_unsafe_mode` |
| `list` | Missing target prefix fails | `mamba/micromamba/src/list.cpp` prefix option handling; `mamba/micromamba/tests/test_list.py` prefix handling expectations | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_when_prefix_missing` |
| `list` | Non-managed prefix fails | `mamba/micromamba/src/list.cpp` prefix/env checks; `mamba/micromamba/tests/test_list.py` env-prefix validity checks | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_for_unmanaged_prefix` |
| `list` | Supports regex + `--full-name` filtering on package names | `mamba/micromamba/src/list.cpp` `list_regex` and `full_name`; `mamba/micromamba/tests/test_list.py` `test_list_name` | `crates/viper-cli/tests/cli_smoke.rs` `list_supports_filter_and_mode_flags` |
| `list` | Supports `--no-pip` filtering and canonical output mode | `mamba/micromamba/src/list.cpp` `no_pip` and `canonical`; `mamba/micromamba/tests/test_list.py` `test_list_with_pip` and `test_list_subcommands` | `crates/viper-cli/tests/cli_smoke.rs` `list_supports_filter_and_mode_flags` |
| `list` | Rejects `--md5` + `--sha256` together when rendering explicit outputs | `mamba/micromamba/tests/test_list.py` `test_list_subcommands` invalid-hash-flags assertion | `crates/viper-cli/tests/cli_smoke.rs` `list_explicit_rejects_md5_and_sha256_together` |
| `list` | `--explicit --md5/--sha256` renders persisted hash digests | `mamba/libmamba/src/api/list.cpp` explicit/hash formatting path | `crates/viper-cli/tests/cli_smoke.rs` `list_explicit_uses_record_hashes` |
| `list` | `--revisions --json` emits structured revision entries with dist-name install/remove payload | `mamba/libmamba/src/api/list.cpp` revisions JSON output; `mamba/libmamba/src/core/history.cpp` dist diff parsing | `crates/viper-cli/tests/cli_smoke.rs` `list_revisions_reads_history`, `remove_history_uses_dist_names_in_revisions` |
| `info` | Returns JSON metadata envelope with environment/config path fields | `mamba/micromamba/src/info.cpp`; `mamba/micromamba/tests/test_info.py` `flags_test` required keys | `crates/viper-cli/tests/cli_smoke.rs` `config_set_get_and_info` |
| `config` | `set/get` roundtrip persists and `config list` returns structured keys | `mamba/micromamba/src/config.cpp` set/get/list command paths; `mamba/micromamba/tests/test_config.py` `TestConfigList` | `crates/viper-cli/tests/cli_smoke.rs` `config_set_get_and_info` |

## Pending List Surface

`viper-cli` now accepts the full option surface from `mamba/micromamba/src/list.cpp`:

- `regex`
- `--full-name`
- `--no-pip`
- `--reverse`
- `--explicit`
- `--md5`
- `--sha256`
- `--canonical`
- `--export`
- `--revisions`

Behavior parity remains in progress for full transaction/rollback semantics.

## Repodata and Parser Matrix

| Area | Behavior | Upstream reference | Viper enforcement |
|---|---|---|---|
| Repodata | Offline without cache fails explicitly | `mamba/libmamba/src/core/subdir_index.cpp` offline/cache semantics | `crates/viper-cli/tests/cli_smoke.rs` `offline_without_cache_fails` |
| Repodata | Offline with valid cache succeeds | `mamba/libmamba/src/core/subdir_index.cpp` cache reuse | `crates/viper-cli/tests/cli_smoke.rs` `offline_with_cache_works` |
| Repodata | First online fetch writes cache JSON + state metadata | `mamba/libmamba/src/core/subdir_index.cpp` `download_and_check_targets` + `finalize_transfer` cache write path | `crates/viper-core/src/repodata.rs` `first_online_fetch_writes_cache_json_and_state_files` |
| Repodata | Fresh TTL cache reuses local index without HTTP request | `mamba/libmamba/src/core/subdir_index.cpp` `load_cache` TTL branch | `crates/viper-core/src/repodata.rs` `fresh_ttl_cache_reuses_local_repodata_without_network` |
| Repodata | `304 Not Modified` refreshes cache metadata timestamp | `mamba/libmamba/src/core/subdir_index.cpp` `download_and_check_targets` `http_status == 304` -> `use_existing_cache` | `crates/viper-core/src/repodata.rs` `http_304_refreshes_cached_metadata_timestamp` |
| Repodata | HTTP failures fall back only when cache exists | `mamba/libmamba/src/core/subdir_index.cpp` `download_and_check_targets` + `use_existing_cache` fallback semantics | `crates/viper-core/src/repodata.rs` `remote_failure_falls_back_only_when_cache_exists` |
| Env parser | Non-string pip entries are rejected | `mamba/micromamba/tests` env-file validation paths | `crates/viper-core/src/spec.rs` `parse_environment_file_rejects_non_string_pip_entry` |
| Env parser | Unsupported dependency mapping is rejected | `mamba/micromamba/tests` env-file validation paths | `crates/viper-core/src/spec.rs` `parse_environment_file_rejects_unknown_dependency_mapping` |
| Env parser | `pip` section must be a sequence | `mamba/micromamba/tests` env-file validation paths | `crates/viper-core/src/spec.rs` `parse_environment_file_rejects_pip_non_sequence` |
| Env parser | Non-string dependency entries are rejected | `mamba/micromamba/tests` env-file validation paths | `crates/viper-core/src/spec.rs` `parse_environment_file_rejects_non_string_dependency_entry` |

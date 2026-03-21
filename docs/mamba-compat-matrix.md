# Viper vs Micromamba Compatibility Matrix

This document tracks command-level behavior alignment using upstream `mamba/` sources/tests as evidence and links each row to an enforcing `viper` test.

## Command Matrix

| Command | Behavior | Upstream reference | Viper enforcement |
|---|---|---|---|
| `create` | Reject conflicting `--prefix` + `--name` | `mamba/micromamba/tests/test_create.py` (target-prefix selection paths) | `crates/viper-cli/tests/cli_smoke.rs` `create_rejects_prefix_and_name_together` |
| `create` | `--dry-run` returns planned actions and avoids writes | `mamba/micromamba/tests/test_create.py#test_create_dry_run` | `crates/viper-cli/tests/cli_smoke.rs` `create_dry_run_returns_transaction_actions` |
| `create` | Env-file YAML name determines env name when provided | `mamba/micromamba/tests/test_create.py` env-file creation cases | `crates/viper-cli/tests/cli_smoke.rs` `create_from_env_file_uses_yaml_name_channels_and_pip_specs` |
| `create` | Env-file without `name` falls back to file stem | `mamba/micromamba/tests/test_install.py` target-prefix/env-file precedence coverage | `crates/viper-cli/tests/cli_smoke.rs` `create_from_env_file_without_name_uses_file_stem_for_prefix` |
| `create` | Env-file `name` must be an env name, not a prefix path | `mamba/libmamba/src/api/configuration.cpp` `file_spec_env_name_hook`; `mamba/micromamba/tests/test_install.py` `yaml_name == "prefix"` failure path | `crates/viper-cli/tests/cli_smoke.rs` `create_from_env_file_rejects_name_with_path_separator` |
| `install` | Missing target prefix fails | `mamba/micromamba/tests/test_install.py` target-prefix checks (`MAMBA_NOT_ALLOW_MISSING_PREFIX`) | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_when_prefix_missing` |
| `install` | Non-managed prefix fails | `mamba/micromamba/tests/test_install.py` target-prefix checks (`MAMBA_NOT_ALLOW_NOT_ENV_PREFIX`) | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_for_unmanaged_prefix` |
| `install` | Env-file with prefix-like `name` is rejected | `mamba/micromamba/tests/test_install.py` `yaml_name == "prefix"` failure path | `crates/viper-cli/tests/cli_smoke.rs` `install_from_env_file_rejects_name_with_path_separator` |
| `remove` | Missing target prefix fails | `mamba/micromamba/tests/test_remove.py` target-prefix checks | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_when_prefix_missing` |
| `remove` | Non-managed prefix fails | `mamba/micromamba/tests/test_remove.py` target-prefix checks | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_for_unmanaged_prefix` |
| `list` | Missing target prefix fails | `mamba/micromamba/tests/test_list.py` prefix handling expectations | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_when_prefix_missing` |
| `list` | Non-managed prefix fails | `mamba/micromamba/tests/test_list.py` env-prefix validity checks | `crates/viper-cli/tests/cli_smoke.rs` `install_remove_list_fail_for_unmanaged_prefix` |
| `info` | Returns JSON metadata envelope | `mamba/micromamba/src/info.cpp` and JSON output paths | `crates/viper-cli/tests/cli_smoke.rs` `config_set_get_and_info` |
| `config` | `set/get` roundtrip persists and reads values | `mamba/micromamba/src/config.cpp` | `crates/viper-cli/tests/cli_smoke.rs` `config_set_get_and_info` |

## Repodata and Parser Matrix

| Area | Behavior | Upstream reference | Viper enforcement |
|---|---|---|---|
| Repodata | Offline without cache fails explicitly | `mamba/libmamba/src/core/subdir_index.cpp` offline/cache semantics | `crates/viper-cli/tests/cli_smoke.rs` `offline_without_cache_fails` |
| Repodata | Offline with valid cache succeeds | `mamba/libmamba/src/core/subdir_index.cpp` cache reuse | `crates/viper-cli/tests/cli_smoke.rs` `offline_with_cache_works` |
| Env parser | Non-string pip entries are rejected | `mamba/micromamba/tests` env-file validation paths | `crates/viper-core/src/spec.rs` `parse_environment_file_rejects_non_string_pip_entry` |
| Env parser | Unsupported dependency mapping is rejected | `mamba/micromamba/tests` env-file validation paths | `crates/viper-core/src/spec.rs` `parse_environment_file_rejects_unknown_dependency_mapping` |
| Env parser | `pip` section must be a sequence | `mamba/micromamba/tests` env-file validation paths | `crates/viper-core/src/spec.rs` `parse_environment_file_rejects_pip_non_sequence` |
| Env parser | Non-string dependency entries are rejected | `mamba/micromamba/tests` env-file validation paths | `crates/viper-core/src/spec.rs` `parse_environment_file_rejects_non_string_dependency_entry` |

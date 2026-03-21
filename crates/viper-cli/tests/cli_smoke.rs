use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn create_install_list_remove_roundtrip() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            ("python", "3.12.0", "0"),
            ("pip", "24.0", "0"),
            ("numpy", "2.0.0", "0"),
        ],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "python>=3.11",
            "pip",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let create_json: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(create_json["success"], true);

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "numpy",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let install_json: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(install_json["success"], true);

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_json: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(list_json["success"], true);
    let packages = list_json["data"]["packages"]
        .as_array()
        .expect("packages array");
    assert!(packages.iter().any(|p| p["name"] == "python"));
    assert!(packages.iter().any(|p| p["name"] == "numpy"));

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "numpy",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let remove_json: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(remove_json["success"], true);

    let state_file = prefix.join("conda-meta").join("viper-state.json");
    let raw = fs::read_to_string(state_file).expect("state file exists");
    let state: Value = serde_json::from_str(&raw).expect("valid state json");
    let names = state["packages"].as_array().expect("packages array");
    assert!(!names.iter().any(|p| p["name"] == "numpy"));
}

#[test]
fn config_set_get_and_info() {
    let tmp_home = tempdir().expect("create temp home");
    let mut config_set = Command::cargo_bin("viper").expect("binary exists");
    config_set
        .env("HOME", tmp_home.path())
        .args(["config", "set", "offline", "true", "--json"])
        .assert()
        .success();

    let mut config_get = Command::cargo_bin("viper").expect("binary exists");
    let output = config_get
        .env("HOME", tmp_home.path())
        .args(["config", "get", "offline", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cfg_json: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(cfg_json["data"]["value"], true);

    let mut info = Command::cargo_bin("viper").expect("binary exists");
    let output = info
        .env("HOME", tmp_home.path())
        .args(["--no-rc", "info", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let info_json: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(info_json["success"], true);
    assert!(info_json["data"]["platform"].is_string());
}

#[test]
fn create_from_env_file_uses_yaml_name_channels_and_pip_specs() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
name: from-yaml
channels:
  - conda-forge
  - bioconda
dependencies:
  - python>=3.11
  - pip
  - pip:
      - numpy==2.0.0
"#,
    )
    .expect("write env file");
    seed_repodata_cache(
        tmp_home.path(),
        &[
            "https://conda.anaconda.org/conda-forge",
            "https://conda.anaconda.org/bioconda",
        ],
        &[("python", "3.12.0", "0"), ("pip", "24.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");

    let expected_prefix = tmp_home
        .path()
        .join(".viper")
        .join("envs")
        .join("from-yaml");
    assert_eq!(
        body["data"]["target_prefix"],
        expected_prefix.display().to_string()
    );
    assert_eq!(
        body["data"]["channels"],
        serde_json::json!(["conda-forge", "bioconda"])
    );

    let state_file = expected_prefix.join("conda-meta").join("viper-state.json");
    let raw = fs::read_to_string(state_file).expect("state file exists");
    let state: Value = serde_json::from_str(&raw).expect("valid state json");
    let packages = state["packages"].as_array().expect("packages array");
    assert!(
        packages
            .iter()
            .any(|p| p["name"] == "python" && p["source"] == "conda")
    );
    assert!(
        packages
            .iter()
            .any(|p| p["name"] == "numpy" && p["source"] == "pip")
    );
}

#[test]
fn create_rejects_prefix_and_name_together() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-n",
            "dev",
            "python",
        ])
        .assert()
        .failure()
        .stderr(contains("cannot set both --prefix and --name"));
}

#[test]
fn create_from_env_file_prefers_yaml_name_over_conda_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let active_prefix = tmp.path().join("active");
    fs::create_dir_all(active_prefix.join("conda-meta")).expect("create active prefix");

    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
name: from-yaml-priority
dependencies:
  - python
"#,
    )
    .expect("write env file");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .env("CONDA_PREFIX", &active_prefix)
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");

    let expected_prefix = tmp_home
        .path()
        .join(".viper")
        .join("envs")
        .join("from-yaml-priority");
    assert_eq!(
        body["data"]["target_prefix"],
        expected_prefix.display().to_string()
    );
    assert_ne!(
        body["data"]["target_prefix"],
        active_prefix.display().to_string()
    );
}

#[test]
fn create_from_env_file_without_name_uses_file_stem_for_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let spec_file = tmp.path().join("env-stem.yaml");
    fs::write(
        &spec_file,
        r#"
dependencies:
  - python
"#,
    )
    .expect("write env file");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");

    let expected_prefix = tmp_home.path().join(".viper").join("envs").join("env-stem");
    assert_eq!(
        body["data"]["target_prefix"],
        expected_prefix.display().to_string()
    );
}

#[test]
fn create_from_env_file_prefers_cli_name_over_yaml_name() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
name: from-yaml
dependencies:
  - python
"#,
    )
    .expect("write env file");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-n",
            "from-cli",
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");

    let expected_prefix = tmp_home.path().join(".viper").join("envs").join("from-cli");
    assert_eq!(
        body["data"]["target_prefix"],
        expected_prefix.display().to_string()
    );
    assert!(
        body["warnings"]
            .as_array()
            .is_some_and(|w| w.iter().any(|msg| msg
                .as_str()
                .unwrap_or("")
                .contains("ignoring environment name 'from-yaml'")))
    );
}

#[test]
fn create_from_env_file_prefers_cli_prefix_over_yaml_name() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let explicit_prefix = tmp.path().join("explicit-prefix");
    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
name: from-yaml
dependencies:
  - python
"#,
    )
    .expect("write env file");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-p",
            explicit_prefix.to_str().expect("utf8"),
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");

    assert_eq!(
        body["data"]["target_prefix"],
        explicit_prefix.display().to_string()
    );
}

#[test]
fn create_from_env_file_rejects_name_with_path_separator() {
    let tmp = tempdir().expect("create temp dir");
    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
name: /tmp/absolute-target
dependencies:
  - python
"#,
    )
    .expect("write env file");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .args([
            "--no-rc",
            "create",
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], false);
    assert_eq!(
        body["error"],
        "invalid environment file: environment name cannot contain path separators"
    );
}

#[test]
fn install_from_env_file_rejects_name_with_path_separator() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("pip", "24.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    let spec_file = tmp.path().join("bad-name.yaml");
    fs::write(
        &spec_file,
        r#"
name: /tmp/bad
dependencies:
  - pip
"#,
    )
    .expect("write env file");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], false);
    assert_eq!(
        body["error"],
        "invalid environment file: environment name cannot contain path separators"
    );
}

#[test]
fn install_prefers_cli_name_over_yaml_name_and_warns() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("pip", "24.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-n",
            "from-cli",
            "python",
            "--json",
        ])
        .assert()
        .success();

    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
name: from-yaml
dependencies:
  - pip
"#,
    )
    .expect("write env file");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "-n",
            "from-cli",
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], true);
    assert!(
        body["warnings"]
            .as_array()
            .is_some_and(|w| w.iter().any(|msg| msg
                .as_str()
                .unwrap_or("")
                .contains("ignoring environment name 'from-yaml'")))
    );
}

#[test]
fn create_accumulates_yaml_and_rc_channels_in_order() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    write_viperrc(tmp_home.path(), "channels:\n  - rc-channel\n");

    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
channels:
  - yaml-channel
dependencies:
  - python
"#,
    )
    .expect("write env file");
    seed_repodata_cache(
        tmp_home.path(),
        &[
            "https://conda.anaconda.org/yaml-channel",
            "https://conda.anaconda.org/rc-channel",
            "https://conda.anaconda.org/conda-forge",
        ],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "create",
            "--offline",
            "--dry-run",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(
        body["data"]["channels"],
        serde_json::json!(["yaml-channel", "rc-channel", "conda-forge"])
    );
}

#[test]
fn create_merges_multiple_env_files_specs() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");

    let first = tmp.path().join("first.yaml");
    fs::write(
        &first,
        r#"
dependencies:
  - python
"#,
    )
    .expect("write first env file");
    let second = tmp.path().join("second.yaml");
    fs::write(
        &second,
        r#"
dependencies:
  - pip
  - pip:
      - numpy==2.0.0
"#,
    )
    .expect("write second env file");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("pip", "24.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            first.to_str().expect("utf8"),
            "-f",
            second.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], true);
    assert!(
        body["data"]["specs"]
            .as_array()
            .is_some_and(|v| v.len() >= 2)
    );
    assert!(
        body["data"]["pip_specs"]
            .as_array()
            .is_some_and(|v| v.iter().any(|x| x == "numpy==2.0.0"))
    );
}

#[test]
fn create_multiple_env_files_keep_first_name_and_warn() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let first = tmp.path().join("first.yaml");
    fs::write(
        &first,
        r#"
name: env-one
dependencies:
  - python
"#,
    )
    .expect("write first env file");
    let second = tmp.path().join("second.yaml");
    fs::write(
        &second,
        r#"
name: env-two
dependencies:
  - pip
"#,
    )
    .expect("write second env file");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("pip", "24.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-f",
            first.to_str().expect("utf8"),
            "-f",
            second.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], true);
    let expected_prefix = tmp_home.path().join(".viper").join("envs").join("env-one");
    assert_eq!(
        body["data"]["target_prefix"],
        expected_prefix.display().to_string()
    );
    assert!(body["warnings"].as_array().is_some_and(|w| {
        !w.is_empty()
            && w[0]
                .as_str()
                .unwrap_or("")
                .contains("ignoring environment name")
    }));
}

#[test]
fn create_accumulates_cli_yaml_rc_and_default_channels() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    write_viperrc(tmp_home.path(), "channels:\n  - rc-channel\n");

    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
channels:
  - yaml-channel
dependencies:
  - python
"#,
    )
    .expect("write env file");
    seed_repodata_cache(
        tmp_home.path(),
        &[
            "https://conda.anaconda.org/cli-channel",
            "https://conda.anaconda.org/yaml-channel",
            "https://conda.anaconda.org/rc-channel",
            "https://conda.anaconda.org/conda-forge",
        ],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "create",
            "--offline",
            "--dry-run",
            "-p",
            prefix.to_str().expect("utf8"),
            "-c",
            "cli-channel",
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(
        body["data"]["channels"],
        serde_json::json!(["cli-channel", "yaml-channel", "rc-channel", "conda-forge"])
    );
}

#[test]
fn create_accumulates_yaml_env_rc_and_default_channels() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    write_viperrc(tmp_home.path(), "channels:\n  - rc-channel\n");

    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
channels:
  - yaml-channel
dependencies:
  - python
"#,
    )
    .expect("write env file");
    seed_repodata_cache(
        tmp_home.path(),
        &[
            "https://conda.anaconda.org/yaml-channel",
            "https://conda.anaconda.org/env-channel",
            "https://conda.anaconda.org/rc-channel",
            "https://conda.anaconda.org/conda-forge",
        ],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .env("VIPER_CHANNELS", "env-channel")
        .args([
            "create",
            "--offline",
            "--dry-run",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(
        body["data"]["channels"],
        serde_json::json!(["yaml-channel", "env-channel", "rc-channel", "conda-forge"])
    );
}

#[test]
fn create_dry_run_returns_transaction_actions() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--dry-run",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "python>=3.11",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");

    assert!(body["data"]["actions"]["link"].is_array());
}

#[test]
fn offline_without_cache_fails() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "python>=3.11",
            "--json",
        ])
        .assert()
        .failure()
        .stdout(contains("offline mode requires a cached repodata index"));
}

#[test]
fn offline_with_cache_works() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let cache_root = tmp_home.path().join(".viper").join("pkgs").join("cache");
    fs::create_dir_all(&cache_root).expect("create cache root");

    let channel = "https://conda.anaconda.org/conda-forge";
    for subdir in [current_platform_subdir(), "noarch".to_string()] {
        let key =
            cache_name_from_repodata_url(&format!("{channel}/{subdir}/current_repodata.json"));
        fs::write(
            cache_root.join(format!("{key}.json")),
            r#"{"packages":{"python-3.12.0-0.tar.bz2":{"name":"python","version":"3.12.0","build":"0"}}}"#,
        )
        .expect("write repodata cache");
        fs::write(
            cache_root.join(format!("{key}.state.json")),
            format!(
                "{{\"fetched_at_epoch_s\":{},\"cache_control\":\"max-age=3600\"}}",
                4_102_444_800u64
            ),
        )
        .expect("write repodata state");
    }

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "--dry-run",
            "-p",
            prefix.to_str().expect("utf8"),
            "python>=3.11",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let links = body["data"]["actions"]["link"]
        .as_array()
        .expect("link actions");
    assert!(links.iter().any(|p| p["name"] == "python"));
}

#[test]
fn create_fails_without_repodata_and_does_not_write_state() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .args([
            "--no-rc",
            "create",
            "-c",
            "http://127.0.0.1:9/bad-channel",
            "-p",
            prefix.to_str().expect("utf8"),
            "python>=3.11",
            "--json",
        ])
        .assert()
        .failure();

    let state_file = prefix.join("conda-meta").join("viper-state.json");
    assert!(!state_file.exists());
}

#[test]
fn install_remove_list_fail_when_prefix_missing() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("missing-env");
    let expected = format!("prefix '{}' does not exist", prefix.display());

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .args([
            "--no-rc",
            "install",
            "-p",
            prefix.to_str().expect("utf8"),
            "numpy",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let install_body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(install_body["success"], false);
    assert_eq!(install_body["error"], expected);

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "numpy",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let remove_body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(remove_body["success"], false);
    assert_eq!(remove_body["error"], expected);

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let list_body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(list_body["success"], false);
    assert_eq!(list_body["error"], expected);
}

#[test]
fn install_remove_list_fail_for_unmanaged_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("plain-dir");
    fs::create_dir_all(&prefix).expect("create prefix dir");
    let expected = format!("prefix '{}' is not a managed environment", prefix.display());

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .args([
            "--no-rc",
            "install",
            "-p",
            prefix.to_str().expect("utf8"),
            "numpy",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let install_body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(install_body["success"], false);
    assert_eq!(install_body["error"], expected);

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "numpy",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let remove_body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(remove_body["success"], false);
    assert_eq!(remove_body["error"], expected);

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let list_body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(list_body["success"], false);
    assert_eq!(list_body["error"], expected);
}

fn cache_name_from_repodata_url(url: &str) -> String {
    let mut normalized = url.trim_end_matches('/').to_string();
    if normalized.ends_with("/repodata.json") {
        normalized.truncate(normalized.len().saturating_sub("/repodata.json".len()));
    }
    let digest = md5::compute(normalized.as_bytes());
    format!("{digest:x}")[..8].to_string()
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

fn seed_repodata_cache(home: &Path, channels: &[&str], packages: &[(&str, &str, &str)]) {
    let cache_root = home.join(".viper").join("pkgs").join("cache");
    fs::create_dir_all(&cache_root).expect("create cache root");

    let packages_json = packages
        .iter()
        .map(|(name, version, build)| {
            let filename = format!("{name}-{version}-{build}.tar.bz2");
            (
                filename,
                serde_json::json!({
                    "name": name,
                    "version": version,
                    "build": build,
                }),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();
    let repodata_body = serde_json::json!({ "packages": packages_json });

    for channel in channels {
        for subdir in [current_platform_subdir(), "noarch".to_string()] {
            let key =
                cache_name_from_repodata_url(&format!("{channel}/{subdir}/current_repodata.json"));
            fs::write(
                cache_root.join(format!("{key}.json")),
                serde_json::to_string(&repodata_body).expect("serialize repodata"),
            )
            .expect("write repodata cache");
            fs::write(
                cache_root.join(format!("{key}.state.json")),
                format!(
                    "{{\"fetched_at_epoch_s\":{},\"cache_control\":\"max-age=3600\"}}",
                    4_102_444_800u64
                ),
            )
            .expect("write repodata state");
        }
    }
}

fn write_viperrc(home: &Path, content: &str) {
    let rc_dir = home.join(".viper");
    fs::create_dir_all(&rc_dir).expect("create rc dir");
    fs::write(rc_dir.join("viperrc"), content).expect("write viperrc");
}

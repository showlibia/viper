use std::fs;

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn create_install_list_remove_roundtrip() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .args([
            "--no-rc",
            "create",
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
        .args([
            "--no-rc",
            "install",
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

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
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

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .env("CONDA_PREFIX", &active_prefix)
        .args([
            "--no-rc",
            "create",
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
fn create_dry_run_returns_transaction_actions() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .args([
            "--no-rc",
            "create",
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
    install
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
        .stdout(contains(&expected));

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
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
        .stdout(contains(&expected));

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    list.args([
        "--no-rc",
        "list",
        "-p",
        prefix.to_str().expect("utf8"),
        "--json",
    ])
    .assert()
    .failure()
    .stdout(contains(&expected));
}

#[test]
fn install_remove_list_fail_for_unmanaged_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("plain-dir");
    fs::create_dir_all(&prefix).expect("create prefix dir");
    let expected = format!("prefix '{}' is not a managed environment", prefix.display());

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
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
        .stdout(contains(&expected));

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
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
        .stdout(contains(&expected));

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    list.args([
        "--no-rc",
        "list",
        "-p",
        prefix.to_str().expect("utf8"),
        "--json",
    ])
    .assert()
    .failure()
    .stdout(contains(&expected));
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

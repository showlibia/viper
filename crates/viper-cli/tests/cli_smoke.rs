use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;

use assert_cmd::Command;
use insta::assert_json_snapshot;
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

    let names = installed_package_names(&prefix);
    assert!(!names.iter().any(|name| name == "numpy"));
}

#[test]
fn create_invalid_spec_fails() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "!bad",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("invalid package specification"))
    );
}

#[test]
fn install_invalid_spec_fails() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .args([
            "--no-rc",
            "install",
            "-p",
            prefix.to_str().expect("utf8"),
            "!bad",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("invalid package specification"))
    );
}

#[test]
fn remove_invalid_spec_fails() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "!bad",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("invalid package specification"))
    );
}

#[test]
fn remove_all_keeps_prefix_but_removes_all_packages() {
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
            "pip",
            "--json",
        ])
        .assert()
        .success();

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "--all",
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
        prefix.exists(),
        "remove --all should not delete the environment prefix"
    );
    assert!(installed_package_names(&prefix).is_empty());
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
    let root_prefix = info_json["data"]["root_prefix"]
        .as_str()
        .expect("root_prefix is string");
    assert_eq!(info_json["data"]["target_prefix"], root_prefix);
    assert_eq!(info_json["data"]["platform"], current_platform_subdir());
    assert_eq!(info_json["data"]["environment"], "base");
    assert_eq!(info_json["data"]["env location"], root_prefix);
    assert_eq!(
        info_json["data"]["populated config files"],
        serde_json::json!([])
    );
    assert!(info_json["data"]["envs_dirs"].is_array());
    assert!(info_json["data"]["envs directories"].is_array());
    assert!(info_json["data"]["package_cache"].is_array());
    assert!(info_json["data"]["package cache"].is_array());
    assert!(info_json["data"]["user_config_files"].is_array());
    assert!(info_json["data"]["user config files"].is_array());
    assert!(info_json["data"]["populated config files"].is_array());
    assert!(
        info_json["data"]["virtual packages"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty() && entries.iter().all(|v| v.is_string()))
    );
    if std::env::consts::OS == "linux" {
        assert!(
            info_json["data"]["virtual packages"]
                .as_array()
                .is_some_and(|entries| entries.iter().any(|v| {
                    v.as_str()
                        .is_some_and(|entry| entry.starts_with("__linux="))
                }))
        );
    }
    assert!(info_json["data"]["environment"].is_string());
    assert!(info_json["data"]["env location"].is_string());
    assert!(info_json["data"]["base_environment"].is_string());
    assert!(info_json["data"]["base environment"].is_string());
    assert!(
        info_json["data"]["channels"]
            .as_array()
            .is_some_and(|channels| channels.iter().all(|v| {
                v.as_str()
                    .is_some_and(|url| url.starts_with("https://conda.anaconda.org/"))
            }))
    );
    assert!(info_json["data"]["libmamba version"].is_string());
    assert!(info_json["data"]["mamba version"].is_string());
    assert!(info_json["data"]["curl version"].is_string());
    assert!(info_json["data"]["libarchive version"].is_string());

    let mut config_list = Command::cargo_bin("viper").expect("binary exists");
    let output = config_list
        .env("HOME", tmp_home.path())
        .args(["--no-rc", "config", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_json: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(list_json["success"], true);
    assert!(list_json["data"]["target_prefix"].is_null());
    assert_eq!(list_json["data"]["json"], true);
}

#[test]
fn info_reports_empty_populated_config_files_when_rc_missing() {
    let tmp_home = tempdir().expect("create temp home");
    let mut info = Command::cargo_bin("viper").expect("binary exists");
    let output = info
        .env("HOME", tmp_home.path())
        .args(["info", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(
        body["data"]["populated config files"],
        serde_json::json!([])
    );
}

#[test]
fn info_environment_status_reports_active_not_env_and_not_found() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let root_prefix = tmp.path().join("root");
    let named_prefix = root_prefix.join("envs").join("demo");
    fs::create_dir_all(named_prefix.join("conda-meta")).expect("create managed env");
    let not_env_prefix = tmp.path().join("plain-dir");
    fs::create_dir_all(&not_env_prefix).expect("create plain dir");
    let missing_prefix = tmp.path().join("missing-env");

    let mut active = Command::cargo_bin("viper").expect("binary exists");
    let output = active
        .env("HOME", tmp_home.path())
        .env("CONDA_PREFIX", &named_prefix)
        .args([
            "--no-rc",
            "--root-prefix",
            root_prefix.to_str().expect("utf8"),
            "--prefix",
            named_prefix.to_str().expect("utf8"),
            "info",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["data"]["environment"], "demo (active)");
    assert_eq!(
        body["data"]["env location"],
        named_prefix.to_str().expect("utf8")
    );

    let mut not_env = Command::cargo_bin("viper").expect("binary exists");
    let output = not_env
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--root-prefix",
            root_prefix.to_str().expect("utf8"),
            "--prefix",
            not_env_prefix.to_str().expect("utf8"),
            "info",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(
        body["data"]["environment"],
        format!("{} (not env)", not_env_prefix.display())
    );
    assert_eq!(
        body["data"]["env location"],
        not_env_prefix.to_str().expect("utf8")
    );

    let mut missing = Command::cargo_bin("viper").expect("binary exists");
    let output = missing
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--root-prefix",
            root_prefix.to_str().expect("utf8"),
            "--prefix",
            missing_prefix.to_str().expect("utf8"),
            "info",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["data"]["environment"]
            .as_str()
            .is_some_and(|name| name.contains("(not found)"))
    );
    assert_eq!(
        body["data"]["env location"],
        missing_prefix.to_str().expect("utf8")
    );

    let mut default_info = Command::cargo_bin("viper").expect("binary exists");
    let output = default_info
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--root-prefix",
            root_prefix.to_str().expect("utf8"),
            "info",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["data"]["environment"], "base");
    assert_eq!(
        body["data"]["env location"],
        root_prefix.to_str().expect("utf8")
    );

    let mut base_active = Command::cargo_bin("viper").expect("binary exists");
    let output = base_active
        .env("HOME", tmp_home.path())
        .env("CONDA_PREFIX", &root_prefix)
        .args([
            "--no-rc",
            "--root-prefix",
            root_prefix.to_str().expect("utf8"),
            "info",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["data"]["environment"], "base (active)");
    assert_eq!(
        body["data"]["env location"],
        root_prefix.to_str().expect("utf8")
    );
}

#[test]
fn info_json_snapshot_is_stable() {
    let tmp_home = tempdir().expect("create temp home");
    let mut info = Command::cargo_bin("viper").expect("binary exists");
    let output = info
        .env("HOME", tmp_home.path())
        .args(["--no-rc", "info", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut body: Value = serde_json::from_slice(&output).expect("valid json");
    body["data"]["root_prefix"] = serde_json::json!("<root_prefix>");
    body["data"]["package_cache"] = serde_json::json!(["<package_cache>"]);
    body["data"]["package cache"] = serde_json::json!(["<package_cache>"]);
    body["data"]["envs_dirs"] = serde_json::json!(["<envs_dir>"]);
    body["data"]["envs directories"] = serde_json::json!(["<envs_dir>"]);
    body["data"]["user_config_files"] = serde_json::json!(["<config_file>"]);
    body["data"]["user config files"] = serde_json::json!(["<config_file>"]);
    body["data"]["environment"] = serde_json::json!("<environment>");
    body["data"]["env location"] = serde_json::json!("<env_location>");
    body["data"]["virtual packages"] = serde_json::json!([]);
    body["data"]["base_environment"] = serde_json::json!("<base_environment>");
    body["data"]["base environment"] = serde_json::json!("<base_environment>");
    body["data"]["target_prefix"] = serde_json::json!("<target_prefix>");
    assert_json_snapshot!("info_json_snapshot", body);
}

#[test]
fn config_list_json_snapshot_is_stable() {
    let tmp_home = tempdir().expect("create temp home");
    let mut config_list = Command::cargo_bin("viper").expect("binary exists");
    let output = config_list
        .env("HOME", tmp_home.path())
        .args(["--no-rc", "config", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut body: Value = serde_json::from_slice(&output).expect("valid json");
    body["data"]["root_prefix"] = serde_json::json!("<root_prefix>");
    body["data"]["target_prefix"] = serde_json::json!(null);
    body["data"]["rc_path"] = serde_json::json!("<rc_path>");
    assert_json_snapshot!("config_list_json_snapshot", body);
}

#[test]
fn list_supports_filter_and_mode_flags() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("pip", "24.0", "0")],
    );

    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
dependencies:
  - python
  - pip
  - pip:
      - pandas==2.2.3
"#,
    )
    .expect("write env file");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            spec_file.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let mut full_name = Command::cargo_bin("viper").expect("binary exists");
    let output = full_name
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--full-name",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let packages = body["data"]["packages"].as_array().expect("packages array");
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0]["name"], "python");

    let mut no_pip = Command::cargo_bin("viper").expect("binary exists");
    let output = no_pip
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--no-pip",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let packages = body["data"]["packages"].as_array().expect("packages array");
    assert!(packages.iter().all(|pkg| pkg["source"] != "pip"));
    assert!(packages.iter().all(|pkg| {
        pkg["channel"]
            .as_str()
            .is_some_and(|c| !c.starts_with("http"))
    }));

    let mut canonical = Command::cargo_bin("viper").expect("binary exists");
    let output = canonical
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--no-pip",
            "--canonical",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let mut canonical_rows = body["data"]["packages"]
        .as_array()
        .expect("canonical rows")
        .iter()
        .map(|row| row.as_str().expect("canonical row string").to_string())
        .collect::<Vec<_>>();
    canonical_rows.sort();
    let subdir = current_platform_subdir();
    let mut expected_rows = vec![
        format!("conda-forge/{subdir}::pip-24.0-0"),
        format!("conda-forge/{subdir}::python-3.12.0-0"),
    ];
    expected_rows.sort();
    assert_eq!(canonical_rows, expected_rows);
    assert!(canonical_rows.iter().all(|row| !row.contains(' ')));

    let mut export = Command::cargo_bin("viper").expect("binary exists");
    let output = export
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--export",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let export_rows = body["data"]["packages"].as_array().expect("export rows");
    assert!(
        export_rows
            .iter()
            .all(|row| row.as_str().is_some_and(|s| s.contains('=')))
    );

    let mut explicit_wins = Command::cargo_bin("viper").expect("binary exists");
    let output = explicit_wins
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--explicit",
            "--canonical",
            "--export",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|msg| msg.contains("--canonical ignored because --explicit"))))
    );
    assert!(
        body["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|msg| msg.contains("--export ignored because --explicit"))))
    );
}

#[test]
fn list_revisions_warns_that_other_modes_are_ignored() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--revisions",
            "--explicit",
            "--canonical",
            "--export",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|msg| msg.contains("--explicit ignored because --revisions"))))
    );
    assert!(
        body["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|msg| msg.contains("--canonical ignored because --revisions"))))
    );
    assert!(
        body["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|msg| msg.contains("--export ignored because --revisions"))))
    );
}

#[test]
fn list_plain_output_shows_ignored_mode_warnings() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--explicit",
            "--canonical",
            "--export",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).expect("utf8");
    assert!(stderr.contains("warning: Option --canonical ignored because --explicit"));
    assert!(stderr.contains("warning: Option --export ignored because --explicit"));
}

#[test]
fn list_json_snapshot_is_stable() {
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
            "pip",
            "--json",
        ])
        .assert()
        .success();

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
    let mut body: Value = serde_json::from_slice(&output).expect("valid json");
    body["data"]["target_prefix"] = serde_json::json!("<target_prefix>");
    if let Some(packages) = body["data"]["packages"].as_array_mut() {
        for package in packages {
            if package.get("installed_at").is_some() {
                package["installed_at"] = serde_json::json!("<installed_at>");
            }
        }
    }
    assert_json_snapshot!("list_json_snapshot", body);
}

#[test]
fn list_explicit_rejects_md5_and_sha256_together() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--explicit",
            "--md5",
            "--sha256",
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
        "invalid list options: only one of --md5 and --sha256 can be specified"
    );
}

#[test]
fn list_regex_matches_installed_packages() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("pytables", "3.9.2", "0")],
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
            "pytables",
            "--json",
        ])
        .assert()
        .success();

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "py.*",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let packages = body["data"]["packages"].as_array().expect("packages array");
    assert!(packages.iter().any(|pkg| pkg["name"] == "python"));
    assert!(packages.iter().any(|pkg| pkg["name"] == "pytables"));
}

#[test]
fn list_revisions_reads_history() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--revisions",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let revisions = body["data"]["revisions"]
        .as_array()
        .expect("revisions array");
    assert!(!revisions.is_empty());
    assert!(revisions[0]["rev"].is_number());
    assert!(revisions[0]["date"].is_string());
    assert!(revisions[0]["install"].is_array());
    assert!(revisions[0]["remove"].is_array());
}

#[test]
fn list_uses_viper_target_prefix_when_prefix_omitted() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .env("VIPER_TARGET_PREFIX", &prefix)
        .args(["--no-rc", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["target_prefix"], prefix.display().to_string());
}

#[test]
fn install_pip_only_env_file_keeps_existing_conda_packages() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[PackageSeed::new("python", "3.11.9", "0")],
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

    let spec_file = tmp.path().join("pip-only.yaml");
    fs::write(
        &spec_file,
        r#"
dependencies:
  - pip:
      - rich==13.0.0
"#,
    )
    .expect("write env file");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            PackageSeed::new("python", "3.11.9", "0"),
            PackageSeed::new("python", "3.12.0", "0"),
        ],
    );

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
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
        .success();

    let records = load_installed_records(&prefix);
    assert!(records.iter().any(|pkg| {
        pkg["name"] == "python" && pkg["source"] == "conda" && pkg["version"] == "3.11.9"
    }));
    assert!(
        records
            .iter()
            .any(|pkg| pkg["name"] == "rich" && pkg["source"] == "pip")
    );
    let history = fs::read_to_string(prefix.join("conda-meta").join("history")).expect("history");
    assert!(!history.contains("+ python-3.12.0-0"));
}

#[test]
fn install_channel_qualified_spec_upgrades_requested_package() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");

    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.11.0", "0")],
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

    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.11.0", "0"), ("python", "3.12.0", "0")],
    );

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "conda-forge::python",
            "--json",
        ])
        .assert()
        .success();

    let records = load_installed_records(&prefix);
    assert!(records.iter().any(|pkg| {
        pkg["name"] == "python" && pkg["source"] == "conda" && pkg["version"] == "3.12.0"
    }));
}

#[test]
fn remove_channel_qualified_spec_targets_package_name() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");

    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "conda-forge::python",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(!names.iter().any(|name| name == "python"));
}

#[test]
fn remove_dependency_also_removes_dependents() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            PackageSeed::new("python", "3.12.0", "0").depends(&["openssl >=3.0"]),
            PackageSeed::new("openssl", "3.0.13", "0"),
        ],
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

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "openssl",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["data"]["removed_names"]
            .as_array()
            .is_some_and(|items| items.iter().any(|name| name == "python"))
    );

    let names = installed_package_names(&prefix);
    assert!(!names.iter().any(|name| name == "openssl"));
    assert!(!names.iter().any(|name| name == "python"));
}

#[test]
fn remove_no_prune_deps_keeps_orphans() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            PackageSeed::new("python", "3.12.0", "0").depends(&["openssl >=3.0"]),
            PackageSeed::new("openssl", "3.0.13", "0"),
        ],
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

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "--no-prune-deps",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(!names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "openssl"));
}

#[test]
fn remove_force_keeps_dependents_in_unsafe_mode() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            PackageSeed::new("python", "3.12.0", "0").depends(&["openssl >=3.0"]),
            PackageSeed::new("openssl", "3.0.13", "0"),
        ],
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

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "--force",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(!names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "openssl"));
}

#[test]
fn remove_print_config_only_is_read_only() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let before_count = installed_package_names(&prefix).len();
    let history_before =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .args([
            "--no-rc",
            "--print-config-only",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["message"], "config rendered");

    let after_count = installed_package_names(&prefix).len();
    assert_eq!(before_count, after_count);
    let history_after =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    assert_eq!(history_before, history_after);
}

#[test]
fn remove_print_config_only_renders_target_prefix_and_specs() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .args([
            "--no-rc",
            "--print-config-only",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "xtensor-python",
            "xtl",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["message"], "config rendered");
    assert_eq!(body["data"]["operation"], "remove");
    assert_eq!(body["data"]["target_prefix"], prefix.display().to_string());
    assert_eq!(
        body["data"]["specs"],
        serde_json::json!(["xtensor-python", "xtl"])
    );
}

#[test]
fn remove_print_config_only_uses_viper_target_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("remove-from-viper-target");

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .env("VIPER_TARGET_PREFIX", &prefix)
        .args([
            "--no-rc",
            "--print-config-only",
            "remove",
            "python",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["message"], "config rendered");
    assert_eq!(body["data"]["target_prefix"], prefix.display().to_string());
}

#[test]
fn remove_default_prune_keeps_explicitly_requested_dependency() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            PackageSeed::new("python", "3.12.0", "0").depends(&["openssl >=3.0"]),
            PackageSeed::new("openssl", "3.0.13", "0"),
        ],
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
            "openssl",
            "--json",
        ])
        .assert()
        .success();

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(!names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "openssl"));
}

#[test]
fn remove_default_prune_removes_orphans_when_no_requested_roots_remain() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            PackageSeed::new("python", "3.12.0", "0").depends(&["openssl >=3.0"]),
            PackageSeed::new("openssl", "3.0.13", "0"),
        ],
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

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(!names.iter().any(|name| name == "python"));
    assert!(!names.iter().any(|name| name == "openssl"));
}

#[test]
fn remove_default_prune_removes_requested_pip_package() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[PackageSeed::new("python", "3.11.9", "0")],
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

    let spec_file = tmp.path().join("pip-only.yaml");
    fs::write(
        &spec_file,
        r#"
dependencies:
  - pip:
      - rich==13.0.0
"#,
    )
    .expect("write env file");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[PackageSeed::new("python", "3.11.9", "0")],
    );

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
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
        .success();

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "rich",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(!names.iter().any(|name| name == "rich"));
}

#[test]
fn remove_with_unparseable_history_keeps_unrelated_conda_packages() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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
            "numpy",
            "--json",
        ])
        .assert()
        .success();

    let history_path = prefix.join("conda-meta").join("history");
    fs::write(
        &history_path,
        "==> 2026-03-23 00:00:00 <==\n# history truncated for test\n",
    )
    .expect("truncate history");

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
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(!names.iter().any(|name| name == "numpy"));
}

#[test]
fn remove_with_missing_history_keeps_unrelated_conda_packages() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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
            "numpy",
            "--json",
        ])
        .assert()
        .success();

    let history_path = prefix.join("conda-meta").join("history");
    fs::remove_file(&history_path).expect("remove history");

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
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(!names.iter().any(|name| name == "numpy"));
}

#[test]
fn remove_with_missing_history_keeps_shared_dependency_needed_by_survivor() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            PackageSeed::new("app-a", "1.0.0", "0").depends(&["shared-lib >=1.0"]),
            PackageSeed::new("app-b", "1.0.0", "0").depends(&["shared-lib >=1.0"]),
            PackageSeed::new("shared-lib", "1.0.0", "0"),
        ],
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
            "app-a",
            "app-b",
            "--json",
        ])
        .assert()
        .success();

    let history_path = prefix.join("conda-meta").join("history");
    fs::remove_file(&history_path).expect("remove history");

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "app-a",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(!names.iter().any(|name| name == "app-a"));
    assert!(names.iter().any(|name| name == "app-b"));
    assert!(names.iter().any(|name| name == "shared-lib"));
}

#[test]
fn remove_with_missing_history_preserves_conda_when_removing_pip_only_package() {
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
            "pip",
            "--json",
        ])
        .assert()
        .success();

    let spec_file = tmp.path().join("pip-only.yaml");
    fs::write(
        &spec_file,
        r#"
dependencies:
  - pip:
      - rich==13.0.0
"#,
    )
    .expect("write env file");
    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
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
        .success();

    let history_path = prefix.join("conda-meta").join("history");
    fs::remove_file(&history_path).expect("remove history");

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "rich",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "pip"));
    assert!(!names.iter().any(|name| name == "rich"));
}

#[test]
fn explicit_noop_install_persists_keep_spec_for_default_remove() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            PackageSeed::new("python", "3.12.0", "0").depends(&["openssl >=3.0"]),
            PackageSeed::new("openssl", "3.0.13", "0"),
        ],
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

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "openssl",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["data"]["actions"]["link"], serde_json::json!([]));
    assert_eq!(body["data"]["actions"]["unlink"], serde_json::json!([]));

    let history = fs::read_to_string(prefix.join("conda-meta").join("history")).expect("history");
    assert!(history.contains("# install specs: [\"openssl\"]"));

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "openssl"));
    assert!(!names.iter().any(|name| name == "python"));
}

#[test]
fn list_explicit_uses_record_hashes() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache_with_options(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[PackageSeed::new("python", "3.12.0", "0")
            .md5("0123456789abcdef0123456789abcdef")
            .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")],
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

    let mut list_md5 = Command::cargo_bin("viper").expect("binary exists");
    let output = list_md5
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--explicit",
            "--md5",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let lines = body["data"]["packages"]
        .as_array()
        .expect("explicit output lines");
    assert!(
        lines
            .iter()
            .filter_map(Value::as_str)
            .any(|line| line.ends_with("#0123456789abcdef0123456789abcdef"))
    );

    let mut list_sha256 = Command::cargo_bin("viper").expect("binary exists");
    let output = list_sha256
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--explicit",
            "--sha256",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let lines = body["data"]["packages"]
        .as_array()
        .expect("explicit output lines");
    assert!(lines.iter().filter_map(Value::as_str).any(|line| {
        line.ends_with("#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    }));
}

#[test]
fn remove_non_installed_package_fails() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["error"], "package 'numpy' is not installed");
}

#[test]
fn remove_history_uses_dist_names_in_revisions() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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
            "numpy",
            "--json",
        ])
        .assert()
        .success();

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
        .success();

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--revisions",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let revisions = body["data"]["revisions"].as_array().expect("revisions");
    assert!(revisions.iter().any(|rev| {
        rev["remove"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|v| v == "numpy-2.0.0-0"))
    }));
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

    let packages = load_installed_records(&expected_prefix);
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
fn install_print_config_only_uses_viper_target_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("from-viper-target");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("VIPER_TARGET_PREFIX", &prefix)
        .args([
            "--no-rc",
            "--print-config-only",
            "install",
            "python",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["message"], "config rendered");
    assert_eq!(body["data"]["target_prefix"], prefix.display().to_string());
}

#[test]
fn install_print_config_only_prefers_viper_target_prefix_over_conda_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let viper_prefix = tmp.path().join("from-viper-target");
    let conda_prefix = tmp.path().join("from-conda-prefix");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("VIPER_TARGET_PREFIX", &viper_prefix)
        .env("CONDA_PREFIX", &conda_prefix)
        .args([
            "--no-rc",
            "--print-config-only",
            "install",
            "python",
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
        viper_prefix.display().to_string()
    );
    assert_ne!(
        body["data"]["target_prefix"],
        conda_prefix.display().to_string()
    );
}

#[test]
fn install_prefers_viper_target_prefix_over_conda_prefix_non_print_path() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let viper_prefix = tmp.path().join("from-viper-target");
    let conda_prefix = tmp.path().join("from-conda-prefix");

    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-p",
            viper_prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
        .env("HOME", tmp_home.path())
        .env("VIPER_TARGET_PREFIX", &viper_prefix)
        .env("CONDA_PREFIX", &conda_prefix)
        .args(["--no-rc", "install", "--offline", "numpy", "--json"])
        .assert()
        .success();

    let names = installed_package_names(&viper_prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "numpy"));
    assert!(!conda_prefix.exists());
}

#[test]
fn create_from_env_file_without_name_requires_explicit_target() {
    let tmp = tempdir().expect("create temp dir");
    let spec_file = tmp.path().join("env-stem.yaml");
    fs::write(
        &spec_file,
        r#"
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
            "--print-config-only",
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
    assert_eq!(
        body["error"],
        "target prefix is required: pass --prefix or --name"
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
fn install_print_config_only_uses_yaml_name_for_target_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
name: from-yaml-install
dependencies:
  - python
"#,
    )
    .expect("write env file");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "install",
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
        .join("from-yaml-install");
    assert_eq!(
        body["data"]["target_prefix"],
        expected_prefix.display().to_string()
    );
}

#[test]
fn install_print_config_only_name_base_uses_root_prefix() {
    let tmp_home = tempdir().expect("create temp home");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "install",
            "-n",
            "base",
            "python",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let expected_prefix = tmp_home.path().join(".viper");
    assert_eq!(
        body["data"]["target_prefix"],
        expected_prefix.display().to_string()
    );
}

#[test]
fn install_print_config_only_yaml_name_base_uses_root_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
name: base
dependencies:
  - python
"#,
    )
    .expect("write env file");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "install",
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
    let expected_prefix = tmp_home.path().join(".viper");
    assert_eq!(
        body["data"]["target_prefix"],
        expected_prefix.display().to_string()
    );
}

#[test]
fn install_print_config_only_without_target_falls_back_to_root_prefix() {
    let tmp_home = tempdir().expect("create temp home");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "install",
            "python",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["message"], "config rendered");
    assert_eq!(
        body["data"]["target_prefix"],
        tmp_home.path().join(".viper").display().to_string()
    );
}

#[test]
fn create_print_config_only_yaml_name_base_uses_root_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let spec_file = tmp.path().join("environment.yaml");
    fs::write(
        &spec_file,
        r#"
name: base
dependencies:
  - python
"#,
    )
    .expect("write env file");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
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
    let expected_prefix = tmp_home.path().join(".viper");
    assert_eq!(
        body["data"]["target_prefix"],
        expected_prefix.display().to_string()
    );
}

#[test]
fn install_name_base_non_print_path_uses_root_prefix() {
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
            "base",
            "python",
            "--json",
        ])
        .assert()
        .success();

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "-n",
            "base",
            "pip",
            "--json",
        ])
        .assert()
        .success();

    let expected_prefix = tmp_home.path().join(".viper");
    let names = installed_package_names(&expected_prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "pip"));
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
fn create_merges_multiple_classic_spec_files() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
    );

    let spec_a = tmp.path().join("a.txt");
    let spec_b = tmp.path().join("b.txt");
    fs::write(&spec_a, "python>=3.11\n").expect("write classic spec a");
    fs::write(&spec_b, "numpy\n").expect("write classic spec b");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            spec_a.to_str().expect("utf8"),
            "-f",
            spec_b.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "numpy"));
}

#[test]
fn create_merges_multiple_explicit_files() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
    );

    let subdir = current_platform_subdir();
    let explicit_a = tmp.path().join("a.explicit");
    let explicit_b = tmp.path().join("b.explicit");
    fs::write(
        &explicit_a,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write explicit a");
    fs::write(
        &explicit_b,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/numpy-2.0.0-0.tar.bz2\n"
        ),
    )
    .expect("write explicit b");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit_a.to_str().expect("utf8"),
            "-f",
            explicit_b.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(!names.iter().any(|name| name == "numpy"));
}

#[test]
fn create_rejects_mixed_file_spec_types() {
    let tmp = tempdir().expect("create temp dir");
    let spec_yaml = tmp.path().join("env.yaml");
    let spec_classic = tmp.path().join("specs.txt");
    fs::write(&spec_yaml, "dependencies:\n  - python\n").expect("write yaml");
    fs::write(&spec_classic, "numpy\n").expect("write classic");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .args([
            "--no-rc",
            "create",
            "-n",
            "mix",
            "-f",
            spec_yaml.to_str().expect("utf8"),
            "-f",
            spec_classic.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("same format group"))
    );
}

#[test]
fn create_rejects_mixed_explicit_and_yaml_regardless_of_order() {
    let tmp = tempdir().expect("create temp dir");
    let subdir = current_platform_subdir();
    let explicit = tmp.path().join("spec.explicit");
    let yaml = tmp.path().join("env.yaml");
    fs::write(
        &explicit,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write explicit");
    fs::write(&yaml, "dependencies:\n  - python\n").expect("write yaml");

    for pair in [(&explicit, &yaml), (&yaml, &explicit)] {
        let mut create = Command::cargo_bin("viper").expect("binary exists");
        let output = create
            .args([
                "--no-rc",
                "--print-config-only",
                "create",
                "-n",
                "mix",
                "-f",
                pair.0.to_str().expect("utf8"),
                "-f",
                pair.1.to_str().expect("utf8"),
                "--json",
            ])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let body: Value = serde_json::from_slice(&output).expect("valid json");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|msg| msg.contains("same format group"))
        );
    }
}

#[test]
fn create_rejects_mixed_explicit_and_lockfile_regardless_of_order() {
    let tmp = tempdir().expect("create temp dir");
    let subdir = current_platform_subdir();
    let explicit = tmp.path().join("spec.explicit");
    let lock = tmp.path().join("lock.json");
    fs::write(
        &explicit,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write explicit");
    fs::write(
        &lock,
        format!(
            r#"{{"lockVersion":1,"packages":{{"numpy-2.0.0-0.tar.bz2":{{"name":"numpy","version":"2.0.0","build":"0","subdir":"{subdir}","channel":"conda-forge"}}}}}}"#
        ),
    )
    .expect("write lock");

    for pair in [(&explicit, &lock), (&lock, &explicit)] {
        let mut create = Command::cargo_bin("viper").expect("binary exists");
        let output = create
            .args([
                "--no-rc",
                "--print-config-only",
                "create",
                "-n",
                "mix",
                "-f",
                pair.0.to_str().expect("utf8"),
                "-f",
                pair.1.to_str().expect("utf8"),
                "--json",
            ])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let body: Value = serde_json::from_slice(&output).expect("valid json");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|msg| msg.contains("same format group"))
        );
    }
}

#[test]
fn create_allows_classic_and_explicit_file_combo() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();

    let classic = tmp.path().join("classic.txt");
    let explicit = tmp.path().join("explicit.txt");
    fs::write(&classic, "numpy\n").expect("write classic");
    fs::write(
        &explicit,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write explicit");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            classic.to_str().expect("utf8"),
            "-f",
            explicit.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(!names.iter().any(|name| name == "numpy"));
}

#[test]
fn create_explicit_file_accepts_hash_url() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();
    let explicit = tmp.path().join("explicit.txt");
    fs::write(
        &explicit,
        format!(
            "@EXPLICIT\n# platform: {subdir}\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2#deadbeefdeadbeefdeadbeefdeadbeef\n"
        ),
    )
    .expect("write explicit");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let records = load_installed_records(&prefix);
    let python = records
        .iter()
        .find(|record| record["name"] == "python")
        .expect("python record");
    assert_eq!(
        python["md5"],
        serde_json::json!("deadbeefdeadbeefdeadbeefdeadbeef")
    );
}

#[test]
fn create_explicit_file_accepts_file_scheme_entries() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();
    let explicit = tmp.path().join("explicit-file.txt");
    fs::write(
        &explicit,
        format!("@EXPLICIT\nfile:///tmp/local-chan/{subdir}/python-3.12.0-0.tar.bz2\n"),
    )
    .expect("write explicit file entry");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let records = load_installed_records(&prefix);
    let python = records
        .iter()
        .find(|record| record["name"] == "python")
        .expect("python record");
    assert_eq!(
        python["url"],
        serde_json::json!(format!(
            "file:///tmp/local-chan/{subdir}/python-3.12.0-0.tar.bz2"
        ))
    );
}

#[test]
fn create_explicit_file_ignores_invalid_positional_specs() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();
    let explicit = tmp.path().join("explicit.txt");
    fs::write(
        &explicit,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write explicit");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit.to_str().expect("utf8"),
            "!bad",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
}

#[test]
fn install_explicit_does_not_remove_unrelated_packages() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();

    let explicit_python = tmp.path().join("python.explicit");
    fs::write(
        &explicit_python,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write python explicit");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit_python.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let explicit_numpy = tmp.path().join("numpy.explicit");
    fs::write(
        &explicit_numpy,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/numpy-2.0.0-0.tar.bz2\n"
        ),
    )
    .expect("write numpy explicit");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit_numpy.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "numpy"));
}

#[test]
fn install_explicit_file_ignores_invalid_positional_specs() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();

    let explicit_python = tmp.path().join("python.explicit");
    fs::write(
        &explicit_python,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write python explicit");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit_python.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let explicit_numpy = tmp.path().join("numpy.explicit");
    fs::write(
        &explicit_numpy,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/numpy-2.0.0-0.tar.bz2\n"
        ),
    )
    .expect("write numpy explicit");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit_numpy.to_str().expect("utf8"),
            "!bad",
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "numpy"));
}

#[test]
fn install_explicit_file_rejects_non_url_entries() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();

    let explicit_python = tmp.path().join("python.explicit");
    fs::write(
        &explicit_python,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write python explicit");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit_python.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let explicit = tmp.path().join("bad.explicit");
    fs::write(&explicit, "@EXPLICIT\npython=3.12\n").expect("write bad explicit");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("must end with .tar.bz2 or .conda"))
    );
}

#[test]
fn install_merges_multiple_classic_spec_files() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            ("python", "3.12.0", "0"),
            ("numpy", "2.0.0", "0"),
            ("pip", "24.0", "0"),
        ],
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

    let spec_a = tmp.path().join("a.txt");
    let spec_b = tmp.path().join("b.txt");
    fs::write(&spec_a, "numpy\n").expect("write classic spec a");
    fs::write(&spec_b, "pip\n").expect("write classic spec b");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            spec_a.to_str().expect("utf8"),
            "-f",
            spec_b.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "numpy"));
    assert!(names.iter().any(|name| name == "pip"));
}

#[test]
fn install_multiple_explicit_files_use_first_explicit_file() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[
            ("python", "3.12.0", "0"),
            ("numpy", "2.0.0", "0"),
            ("pip", "24.0", "0"),
        ],
    );

    let subdir = current_platform_subdir();
    let explicit_a = tmp.path().join("a.explicit");
    let explicit_b = tmp.path().join("b.explicit");
    fs::write(
        &explicit_a,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/numpy-2.0.0-0.tar.bz2\n"
        ),
    )
    .expect("write explicit a");
    fs::write(
        &explicit_b,
        format!("@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/pip-24.0-0.tar.bz2\n"),
    )
    .expect("write explicit b");

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

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit_a.to_str().expect("utf8"),
            "-f",
            explicit_b.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(names.iter().any(|name| name == "numpy"));
    assert!(!names.iter().any(|name| name == "pip"));
}

#[test]
fn install_rejects_mixed_file_spec_types() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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

    let spec_yaml = tmp.path().join("env.yaml");
    let spec_classic = tmp.path().join("specs.txt");
    fs::write(&spec_yaml, "dependencies:\n  - numpy\n").expect("write yaml");
    fs::write(&spec_classic, "numpy\n").expect("write classic");

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
            spec_yaml.to_str().expect("utf8"),
            "-f",
            spec_classic.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("same format group"))
    );
}

#[test]
fn install_print_config_only_multiple_classic_files_preserve_spec_order() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");
    let spec_a = tmp.path().join("a.txt");
    let spec_b = tmp.path().join("b.txt");
    fs::write(&spec_a, "numpy\n").expect("write classic spec a");
    fs::write(&spec_b, "pip\n").expect("write classic spec b");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .args([
            "--no-rc",
            "--print-config-only",
            "install",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            spec_a.to_str().expect("utf8"),
            "-f",
            spec_b.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["data"]["specs"], serde_json::json!(["numpy", "pip"]));
}

#[test]
fn install_print_config_only_explicit_short_circuits_to_first_file() {
    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();
    let explicit_a = tmp.path().join("a.explicit");
    let explicit_b = tmp.path().join("b.explicit");
    let first_url =
        format!("https://conda.anaconda.org/conda-forge/{subdir}/numpy-2.0.0-0.tar.bz2");
    let second_url = format!("https://conda.anaconda.org/conda-forge/{subdir}/pip-24.0-0.tar.bz2");
    fs::write(&explicit_a, format!("@EXPLICIT\n{first_url}\n")).expect("write explicit a");
    fs::write(&explicit_b, format!("@EXPLICIT\n{second_url}\n")).expect("write explicit b");

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .args([
            "--no-rc",
            "--print-config-only",
            "install",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit_a.to_str().expect("utf8"),
            "-f",
            explicit_b.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["data"]["explicit_mode"], true);
    assert_eq!(body["data"]["specs"], serde_json::json!([first_url]));
}

#[test]
fn install_rejects_mixed_lockfile_and_classic_files() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let subdir = current_platform_subdir();
    let lockfile = tmp.path().join("one-lock.json");
    fs::write(
        &lockfile,
        format!(
            r#"{{"lockVersion":1,"packages":{{"python-3.12.0-0.tar.bz2":{{"name":"python","version":"3.12.0","build":"0","subdir":"{subdir}","channel":"conda-forge"}}}}}}"#
        ),
    )
    .expect("write lockfile");
    let classic = tmp.path().join("specs.txt");
    fs::write(&classic, "numpy\n").expect("write classic specs");

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
            lockfile.to_str().expect("utf8"),
            "-f",
            classic.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("same format group"))
    );
}

#[test]
fn install_rejects_mixed_explicit_and_yaml_regardless_of_order() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let subdir = current_platform_subdir();
    let explicit = tmp.path().join("spec.explicit");
    let yaml = tmp.path().join("env.yaml");
    fs::write(
        &explicit,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write explicit");
    fs::write(&yaml, "dependencies:\n  - python\n").expect("write yaml");

    for pair in [(&explicit, &yaml), (&yaml, &explicit)] {
        let mut install = Command::cargo_bin("viper").expect("binary exists");
        let output = install
            .env("HOME", tmp_home.path())
            .args([
                "--no-rc",
                "--print-config-only",
                "install",
                "-p",
                prefix.to_str().expect("utf8"),
                "-f",
                pair.0.to_str().expect("utf8"),
                "-f",
                pair.1.to_str().expect("utf8"),
                "--json",
            ])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let body: Value = serde_json::from_slice(&output).expect("valid json");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|msg| msg.contains("same format group"))
        );
    }
}

#[test]
fn install_rejects_mixed_explicit_and_lockfile_regardless_of_order() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let subdir = current_platform_subdir();
    let explicit = tmp.path().join("spec.explicit");
    let lock = tmp.path().join("lock.json");
    fs::write(
        &explicit,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write explicit");
    fs::write(
        &lock,
        format!(
            r#"{{"lockVersion":1,"packages":{{"numpy-2.0.0-0.tar.bz2":{{"name":"numpy","version":"2.0.0","build":"0","subdir":"{subdir}","channel":"conda-forge"}}}}}}"#
        ),
    )
    .expect("write lock");

    for pair in [(&explicit, &lock), (&lock, &explicit)] {
        let mut install = Command::cargo_bin("viper").expect("binary exists");
        let output = install
            .env("HOME", tmp_home.path())
            .args([
                "--no-rc",
                "--print-config-only",
                "install",
                "-p",
                prefix.to_str().expect("utf8"),
                "-f",
                pair.0.to_str().expect("utf8"),
                "-f",
                pair.1.to_str().expect("utf8"),
                "--json",
            ])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let body: Value = serde_json::from_slice(&output).expect("valid json");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|msg| msg.contains("same format group"))
        );
    }
}

#[test]
fn create_and_install_multiple_lockfiles_use_last_file() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();

    let lock_a = tmp.path().join("a-lock.json");
    let lock_b = tmp.path().join("b-lock.json");
    fs::write(
        &lock_a,
        format!(
            r#"{{"lockVersion":1,"packages":{{"python-3.12.0-0.tar.bz2":{{"name":"python","version":"3.12.0","build":"0","subdir":"{subdir}","channel":"conda-forge"}}}}}}"#
        ),
    )
    .expect("write lockfile a");
    fs::write(
        &lock_b,
        format!(
            r#"{{"lockVersion":1,"packages":{{"numpy-2.0.0-0.tar.bz2":{{"name":"numpy","version":"2.0.0","build":"0","subdir":"{subdir}","channel":"conda-forge"}}}}}}"#
        ),
    )
    .expect("write lockfile b");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let create_output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            lock_a.to_str().expect("utf8"),
            "-f",
            lock_b.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let create_body: Value = serde_json::from_slice(&create_output).expect("valid json");
    assert_eq!(create_body["data"]["explicit_mode"], true);
    assert!(
        create_body["data"]["specs"]
            .as_array()
            .is_some_and(|specs| specs
                .iter()
                .any(|spec| spec.as_str().is_some_and(|s| s.contains("numpy-2.0.0-0"))))
    );
    assert!(
        create_body["data"]["specs"]
            .as_array()
            .is_some_and(|specs| !specs
                .iter()
                .any(|spec| spec.as_str().is_some_and(|s| s.contains("python-3.12.0-0"))))
    );

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let install_output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "install",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            lock_a.to_str().expect("utf8"),
            "-f",
            lock_b.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let install_body: Value = serde_json::from_slice(&install_output).expect("valid json");
    assert_eq!(install_body["data"]["explicit_mode"], true);
    assert!(
        install_body["data"]["specs"]
            .as_array()
            .is_some_and(|specs| specs
                .iter()
                .any(|spec| spec.as_str().is_some_and(|s| s.contains("numpy-2.0.0-0"))))
    );
    assert!(
        install_body["data"]["specs"]
            .as_array()
            .is_some_and(|specs| !specs
                .iter()
                .any(|spec| spec.as_str().is_some_and(|s| s.contains("python-3.12.0-0"))))
    );
}

#[test]
fn remove_after_explicit_create_removes_target_package() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();

    let explicit_python = tmp.path().join("python.explicit");
    fs::write(
        &explicit_python,
        format!(
            "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write python explicit");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            explicit_python.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["data"]["removed"], 1);
    assert!(
        body["data"]["removed_names"]
            .as_array()
            .is_some_and(|items| items.iter().any(|name| name == "python"))
    );
}

#[test]
fn print_config_only_outputs_normalized_request_without_writing_prefix() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let spec_file = tmp.path().join("specs.txt");
    fs::write(&spec_file, "python\n").expect("write spec file");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "create",
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
    assert_eq!(body["success"], true);
    assert_eq!(body["message"], "config rendered");
    assert_eq!(body["data"]["operation"], "create");
    assert_eq!(body["data"]["target_prefix"], prefix.display().to_string());
    assert!(!prefix.exists());
}

#[test]
fn create_print_config_only_accepts_remote_yaml_file() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let (url, server) = serve_single_http_file(
        "env.yaml",
        "name: remote-env\ndependencies:\n  - python\nchannels:\n  - conda-forge\n",
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            &url,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    server.join().expect("server thread");
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], true);
    assert_eq!(body["message"], "config rendered");
    assert_eq!(body["data"]["operation"], "create");
    assert_eq!(body["data"]["target_prefix"], prefix.display().to_string());
}

#[test]
fn create_print_config_only_accepts_remote_classic_spec_file() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let (url, server) = serve_single_http_file("specs.txt", "python>=3.12\nnumpy\n");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            &url,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    server.join().expect("server thread");
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], true);
    assert_eq!(body["message"], "config rendered");
    assert_eq!(body["data"]["operation"], "create");
    assert_eq!(body["data"]["target_prefix"], prefix.display().to_string());
}

#[test]
fn create_print_config_only_accepts_remote_explicit_file() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();
    let explicit = format!(
        "@EXPLICIT\nhttps://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
    );
    let (url, server) = serve_single_http_file("explicit.txt", &explicit);

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            &url,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    server.join().expect("server thread");
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["explicit_mode"], true);
}

#[test]
fn create_print_config_only_accepts_remote_lockfile() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();
    let lock = format!(
        "package:\n  - manager: conda\n    url: https://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
    );
    let (url, server) = serve_single_http_file("conda-lock.yml", &lock);

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "--print-config-only",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            &url,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    server.join().expect("server thread");
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["explicit_mode"], true);
}

#[test]
fn create_accepts_conda_lockfile_input() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();
    let lockfile = tmp.path().join("conda-lock.yml");
    fs::write(
        &lockfile,
        format!(
            "package:\n  - manager: conda\n    url: https://conda.anaconda.org/conda-forge/{subdir}/python-3.12.0-0.tar.bz2\n"
        ),
    )
    .expect("write lockfile");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            lockfile.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
}

#[test]
fn create_accepts_mambajs_lockfile_input() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();
    let source = mamba_lockfile_fixture(&format!("test-env-lock-{subdir}.json"));
    let lockfile = tmp.path().join("test-env-lock.json");
    fs::copy(source, &lockfile).expect("copy mambajs lockfile");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            lockfile.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "zlib"));
}

#[test]
fn create_rejects_mixed_lockfile_and_classic_files() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let subdir = current_platform_subdir();
    let source = mamba_lockfile_fixture(&format!("test-env-lock-{subdir}.json"));
    let lockfile = tmp.path().join("test-env-lock.json");
    fs::copy(source, &lockfile).expect("copy mambajs lockfile");
    let classic = tmp.path().join("specs.txt");
    fs::write(&classic, "python\n").expect("write classic specs");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            lockfile.to_str().expect("utf8"),
            "-f",
            classic.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("same format group"))
    );
}

#[test]
fn create_lockfile_filters_non_current_platform_packages() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let source = mamba_lockfile_fixture("test-env-lock.yaml");
    let lockfile = tmp.path().join("test-env-lock.yaml");
    fs::copy(source, &lockfile).expect("copy lockfile");
    let current = current_platform_subdir();

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            lockfile.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let records = load_installed_records(&prefix);
    for record in records
        .iter()
        .filter(|record| record["source"] == "conda")
        .filter_map(|record| record["platform"].as_str())
    {
        assert!(
            record == current || record == "noarch",
            "unexpected platform record: {record} != {current}"
        );
    }
}

#[test]
fn create_lockfile_with_pip_installs_pip_records() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let lockfile = tmp.path().join("pip-lock.json");
    fs::write(
        &lockfile,
        r#"
{
  "lockVersion": "1.0.1",
  "platform": "linux-64",
  "channels": ["conda-forge"],
  "packages": {},
  "pipPackages": {
    "starlette-0.17.1-py3-none-any.whl": {
      "name": "starlette",
      "version": "0.17.1",
      "url": "https://files.pythonhosted.org/packages/starlette-0.17.1-py3-none-any.whl",
      "registry": "PyPi"
    }
  }
}
"#,
    )
    .expect("write pip lock");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "-p",
            prefix.to_str().expect("utf8"),
            "-f",
            lockfile.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success();

    let records = load_installed_records(&prefix);
    assert!(
        records
            .iter()
            .any(|record| record["name"] == "starlette" && record["source"] == "pip")
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

    assert!(body["data"]["actions"]["fetch"].is_array());
    assert!(body["data"]["actions"]["extract"].is_array());
    assert!(body["data"]["actions"]["link"].is_array());
    assert!(body["data"]["actions"]["fetch"][0]["url"].is_string());
    assert!(body["data"]["actions"]["extract"][0]["fetched_dist"].is_string());
}

#[test]
fn create_dry_run_does_not_write_prefix_state() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--dry-run",
            "--offline",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    assert!(!prefix.join("conda-meta").exists());
}

#[test]
fn create_failure_rolls_back_existing_prefix_layout() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("existing-prefix");
    fs::create_dir_all(&prefix).expect("create existing prefix");
    fs::write(prefix.join("keep.txt"), "keep").expect("write keep marker");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    let output = create
        .env("HOME", tmp_home.path())
        .env("VIPER_TX_FAIL_POINT", "before_persist")
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
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("transaction failed"))
    );
    assert!(prefix.join("keep.txt").exists());
    assert!(!prefix.join("conda-meta").exists());
    assert!(!prefix.join("bin").exists());
    assert!(!prefix.join("pkgs").exists());
}

#[cfg(unix)]
#[test]
fn create_failure_rolls_back_symlinked_prefix_layout() {
    use std::os::unix::fs as unix_fs;

    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("existing-prefix");
    fs::create_dir_all(&prefix).expect("create existing prefix");
    let target_file = prefix.join("keep-target.txt");
    fs::write(&target_file, "keep-target").expect("write keep target");
    unix_fs::symlink("keep-target.txt", prefix.join("keep-link.txt")).expect("create file symlink");
    fs::create_dir_all(prefix.join("real-dir")).expect("create dir");
    unix_fs::symlink("real-dir", prefix.join("dir-link")).expect("create dir symlink");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .env("VIPER_TX_FAIL_POINT", "before_persist")
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
        .failure();

    let file_link_meta = fs::symlink_metadata(prefix.join("keep-link.txt")).expect("file link meta");
    assert!(file_link_meta.file_type().is_symlink());
    assert_eq!(
        fs::read_link(prefix.join("keep-link.txt")).expect("read file link"),
        std::path::PathBuf::from("keep-target.txt")
    );
    let dir_link_meta = fs::symlink_metadata(prefix.join("dir-link")).expect("dir link meta");
    assert!(dir_link_meta.file_type().is_symlink());
    assert_eq!(
        fs::read_link(prefix.join("dir-link")).expect("read dir link"),
        std::path::PathBuf::from("real-dir")
    );
}

#[test]
fn install_dry_run_does_not_write_state_or_history() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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

    let before_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let before_names = installed_package_names(&prefix);

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "--dry-run",
            "-p",
            prefix.to_str().expect("utf8"),
            "numpy",
            "--json",
        ])
        .assert()
        .success();

    let after_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let after_names = installed_package_names(&prefix);
    assert_eq!(before_history, after_history);
    assert_eq!(before_names, after_names);
    assert!(!after_names.iter().any(|name| name == "numpy"));
}

#[test]
fn install_dry_run_returns_phase_separated_actions() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.11.0", "0"), ("python", "3.12.0", "0")],
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
            "python==3.11.0",
            "--json",
        ])
        .assert()
        .success();

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "install",
            "--offline",
            "--dry-run",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(body["data"]["actions"]["fetch"].is_array());
    assert!(body["data"]["actions"]["extract"].is_array());
    assert!(body["data"]["actions"]["link"].is_array());
    assert!(body["data"]["actions"]["fetch"][0]["url"].is_string());
    assert!(body["data"]["actions"]["extract"][0]["fetched_dist"].is_string());
    assert!(body["data"]["actions"]["link"][0]["build_number"].is_number());
}

#[test]
fn install_failure_after_persist_rolls_back_state_and_history() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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

    let before_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let before_names = installed_package_names(&prefix);

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .env("VIPER_TX_FAIL_POINT", "after_persist")
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
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("transaction failed"))
    );

    let after_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let after_names = installed_package_names(&prefix);
    assert_eq!(before_history, after_history);
    assert_eq!(before_names, after_names);
    assert!(!after_names.iter().any(|name| name == "numpy"));
}

#[test]
fn install_failure_after_extract_rolls_back_state_and_history() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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

    let before_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let before_names = installed_package_names(&prefix);

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    let output = install
        .env("HOME", tmp_home.path())
        .env("VIPER_TX_FAIL_POINT", "after_extract")
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
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("transaction failed"))
    );

    let after_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let after_names = installed_package_names(&prefix);
    assert_eq!(before_history, after_history);
    assert_eq!(before_names, after_names);
    assert!(!after_names.iter().any(|name| name == "numpy"));
    assert!(!prefix.join("pkgs").join(".viper-transaction").exists());
}

#[test]
fn install_success_cleans_transaction_stage_artifacts() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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

    let mut install = Command::cargo_bin("viper").expect("binary exists");
    install
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
        .success();

    assert!(!prefix.join("pkgs").join(".viper-transaction").exists());
}

#[test]
fn install_real_history_io_failure_rolls_back_state() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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

    let history_path = prefix.join("conda-meta").join("history");
    fs::remove_file(&history_path).expect("remove history file");
    fs::create_dir_all(&history_path).expect("create history dir");

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
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("Is a directory"))
    );

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(!names.iter().any(|name| name == "numpy"));
}

#[test]
fn remove_dry_run_does_not_write_state_or_history() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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
            "numpy",
            "--json",
        ])
        .assert()
        .success();

    let before_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let before_names = installed_package_names(&prefix);

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .args([
            "--no-rc",
            "remove",
            "--dry-run",
            "-p",
            prefix.to_str().expect("utf8"),
            "numpy",
            "--json",
        ])
        .assert()
        .success();

    let after_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let mut after_names = installed_package_names(&prefix);
    let mut before_names = before_names;
    before_names.sort();
    after_names.sort();
    assert_eq!(before_history, after_history);
    assert_eq!(before_names, after_names);
    assert!(after_names.iter().any(|name| name == "numpy"));
}

#[test]
fn remove_failure_after_persist_rolls_back_state_and_history() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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
            "numpy",
            "--json",
        ])
        .assert()
        .success();

    let before_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let before_names = installed_package_names(&prefix);

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .env("VIPER_TX_FAIL_POINT", "after_persist")
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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("transaction failed"))
    );

    let after_history =
        fs::read_to_string(prefix.join("conda-meta").join("history")).expect("read history");
    let mut after_names = installed_package_names(&prefix);
    let mut before_names = before_names;
    before_names.sort();
    after_names.sort();
    assert_eq!(before_history, after_history);
    assert_eq!(before_names, after_names);
    assert!(after_names.iter().any(|name| name == "numpy"));
}

#[test]
fn remove_real_history_io_failure_rolls_back_state() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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
            "numpy",
            "--json",
        ])
        .assert()
        .success();

    let history_path = prefix.join("conda-meta").join("history");
    fs::remove_file(&history_path).expect("remove history file");
    fs::create_dir_all(&history_path).expect("create history dir");

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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("Is a directory"))
    );

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "numpy"));
    assert!(names.iter().any(|name| name == "python"));
}

#[test]
fn remove_fails_on_malformed_history_specs() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
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
            "numpy",
            "--json",
        ])
        .assert()
        .success();

    let history_path = prefix.join("conda-meta").join("history");
    fs::write(
        &history_path,
        "==> 2026-03-23 00:00:00 <==\n# install specs: [invalid-json\n",
    )
    .expect("write malformed history");

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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("invalid history specs entry"))
    );
}

#[test]
fn list_revisions_fails_when_history_is_unreadable() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    seed_repodata_cache(
        tmp_home.path(),
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0")],
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

    let history_path = prefix.join("conda-meta").join("history");
    fs::remove_file(&history_path).expect("remove history file");
    fs::create_dir_all(&history_path).expect("make unreadable history path");

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .args([
            "--no-rc",
            "list",
            "-p",
            prefix.to_str().expect("utf8"),
            "--revisions",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("Is a directory"))
    );
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
fn offline_fails_when_cached_state_metadata_is_malformed() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let channel = "https://conda.anaconda.org/conda-forge";
    seed_named_repodata_cache(
        tmp_home.path(),
        channel,
        "current_repodata.json",
        &[("python", "3.12.0", "0")],
    );

    let cache_root = tmp_home.path().join(".viper").join("pkgs").join("cache");
    let subdir = current_platform_subdir();
    let key = cache_name_from_repodata_url(&format!("{channel}/{subdir}/current_repodata.json"));
    fs::write(cache_root.join(format!("{key}.state.json")), "{not-json")
        .expect("corrupt state metadata");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
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
        .failure()
        .stdout(contains("invalid repodata cache metadata"));
}

#[test]
fn offline_fails_when_cached_repodata_json_is_malformed() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let channel = "https://conda.anaconda.org/conda-forge";
    seed_named_repodata_cache(
        tmp_home.path(),
        channel,
        "current_repodata.json",
        &[("python", "3.12.0", "0")],
    );

    let cache_root = tmp_home.path().join(".viper").join("pkgs").join("cache");
    let subdir = current_platform_subdir();
    let key = cache_name_from_repodata_url(&format!("{channel}/{subdir}/current_repodata.json"));
    fs::write(cache_root.join(format!("{key}.json")), "{not-json").expect("corrupt repodata json");

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
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
        .failure()
        .stdout(contains("invalid repodata"));
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
fn offline_relaxed_spec_uses_current_repodata_without_full_cache() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let channel = "https://conda.anaconda.org/conda-forge";
    seed_named_repodata_cache(
        tmp_home.path(),
        channel,
        "current_repodata.json",
        &[("python", "3.12.0", "0")],
    );

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
fn offline_restrictive_spec_requires_full_repodata_cache() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let channel = "https://conda.anaconda.org/conda-forge";
    seed_named_repodata_cache(
        tmp_home.path(),
        channel,
        "current_repodata.json",
        &[("python", "3.12.0", "0")],
    );

    let mut create = Command::cargo_bin("viper").expect("binary exists");
    create
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "create",
            "--offline",
            "--dry-run",
            "-p",
            prefix.to_str().expect("utf8"),
            "python<3.10",
            "--json",
        ])
        .assert()
        .failure()
        .stdout(contains("offline mode requires a cached repodata index"));
}

#[test]
fn offline_restrictive_build_channel_subdir_and_hash_specs_require_full_repodata_cache() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let channel = "https://conda.anaconda.org/conda-forge";
    seed_named_repodata_cache(
        tmp_home.path(),
        channel,
        "current_repodata.json",
        &[("python", "3.12.0", "0")],
    );
    let platform = current_platform_subdir();
    let restrictive_specs = [
        "python[build=\"0\"]".to_string(),
        "conda-forge::python>=3.11".to_string(),
        format!("python[subdir={platform}]"),
        "python[md5=deadbeefdeadbeefdeadbeefdeadbeef]".to_string(),
    ];

    for spec in restrictive_specs {
        let mut create = Command::cargo_bin("viper").expect("binary exists");
        create
            .env("HOME", tmp_home.path())
            .args([
                "--no-rc",
                "create",
                "--offline",
                "--dry-run",
                "-p",
                prefix.to_str().expect("utf8"),
                spec.as_str(),
                "--json",
            ])
            .assert()
            .failure()
            .stdout(contains("offline mode requires a cached repodata index"));
    }
}

#[test]
fn offline_restrictive_spec_uses_full_repodata_when_available() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let channel = "https://conda.anaconda.org/conda-forge";
    seed_named_repodata_cache(
        tmp_home.path(),
        channel,
        "current_repodata.json",
        &[("python", "3.12.0", "0")],
    );
    seed_named_repodata_cache(
        tmp_home.path(),
        channel,
        "repodata.json",
        &[("python", "3.9.18", "0")],
    );

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
            "python<3.10",
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
    let python = links
        .iter()
        .find(|pkg| pkg["name"] == "python")
        .expect("python action");
    assert_eq!(python["version"], "3.9.18");
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

    assert!(!prefix.join("conda-meta").exists());
}

#[cfg(unix)]
#[test]
fn list_discovers_pip_site_packages_and_no_pip_hides_them() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");
    let meta_dir = prefix.join("conda-meta");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::write(
        meta_dir.join("python-3.12.0-0.json"),
        r#"{
  "name":"python",
  "version":"3.12.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"python-3.12.0-0",
  "spec":"python",
  "source":"conda",
  "depends":[],
  "installed_at":"now",
  "platform":"linux-64"
}"#,
    )
    .expect("write python record");
    fs::write(
        meta_dir.join("pip-24.0-0.json"),
        r#"{
  "name":"pip",
  "version":"24.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/noarch/pip-24.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"pip-24.0-0",
  "spec":"pip",
  "source":"conda",
  "depends":["python >=3.12,<3.13"],
  "installed_at":"now",
  "platform":"noarch"
}"#,
    )
    .expect("write pip record");
    fs::write(meta_dir.join("history"), "").expect("write history");

    let bin_dir = prefix.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin");
    let python = bin_dir.join("python");
    fs::write(
        &python,
        "#!/bin/sh\nprintf '{\"environment\":{\"sys_platform\":\"linux\",\"platform_machine\":\"x86_64\"},\"installed\":[{\"installer\":\"pip\",\"metadata\":{\"name\":\"rich\",\"version\":\"13.7.1\"}}]}'\n",
    )
    .expect("write fake python");
    let mut perms = fs::metadata(&python).expect("python meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&python, perms).expect("chmod python");

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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let names = body["data"]["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .filter_map(|pkg| pkg["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"python"));
    assert!(names.contains(&"rich"));
    let rich_platform = body["data"]["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .find(|pkg| pkg["name"].as_str() == Some("rich"))
        .and_then(|pkg| pkg["platform"].as_str())
        .unwrap_or_default()
        .to_string();
    assert_eq!(rich_platform, "linux-x86_64");

    let mut list_no_pip = Command::cargo_bin("viper").expect("binary exists");
    let output = list_no_pip
        .args([
            "--no-rc",
            "list",
            "--no-pip",
            "-p",
            prefix.to_str().expect("utf8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let names = body["data"]["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .filter_map(|pkg| pkg["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"python"));
    assert!(!names.contains(&"rich"));
}

#[cfg(unix)]
#[test]
fn list_ignores_site_packages_when_conda_pip_is_not_installed() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");
    let meta_dir = prefix.join("conda-meta");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::write(
        meta_dir.join("python-3.12.0-0.json"),
        r#"{
  "name":"python",
  "version":"3.12.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"python-3.12.0-0",
  "spec":"python",
  "source":"conda",
  "depends":[],
  "installed_at":"now",
  "platform":"linux-64"
}"#,
    )
    .expect("write python record");
    fs::write(meta_dir.join("history"), "").expect("write history");

    let bin_dir = prefix.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin");
    let python = bin_dir.join("python");
    fs::write(
        &python,
        "#!/bin/sh\nprintf '{\"environment\":{\"platform\":\"linux-64\"},\"installed\":[{\"installer\":\"pip\",\"metadata\":{\"name\":\"rich\",\"version\":\"13.7.1\"}}]}'\n",
    )
    .expect("write fake python");
    let mut perms = fs::metadata(&python).expect("python meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&python, perms).expect("chmod python");

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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let names = body["data"]["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .filter_map(|pkg| pkg["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"python"));
    assert!(!names.contains(&"rich"));
}

#[cfg(unix)]
#[test]
fn list_does_not_fallback_to_host_path_python_for_pip_discovery() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let meta_dir = prefix.join("conda-meta");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::write(
        meta_dir.join("pip-24.0-0.json"),
        r#"{
  "name":"pip",
  "version":"24.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/noarch/pip-24.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"pip-24.0-0",
  "spec":"pip",
  "source":"conda",
  "depends":["python >=3.12,<3.13"],
  "installed_at":"now",
  "platform":"noarch"
}"#,
    )
    .expect("write pip record");
    fs::write(meta_dir.join("history"), "").expect("write history");

    let host_bin = tmp.path().join("host-bin");
    fs::create_dir_all(&host_bin).expect("create host bin");
    let host_python = host_bin.join("python");
    fs::write(
        &host_python,
        "#!/bin/sh\nprintf '{\"environment\":{\"sys_platform\":\"fakeos\",\"platform_machine\":\"fakearch\"},\"installed\":[{\"installer\":\"pip\",\"metadata\":{\"name\":\"host-leak\",\"version\":\"1.0\"}}]}'\n",
    )
    .expect("write host python");
    let mut perms = fs::metadata(&host_python).expect("host python meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&host_python, perms).expect("chmod host python");

    let mut list = Command::cargo_bin("viper").expect("binary exists");
    let output = list
        .env("HOME", tmp_home.path())
        .env("PATH", host_bin.to_str().expect("utf8"))
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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let err = body["error"].as_str().unwrap_or_default();
    assert!(err.contains("no python executable was found"));
}

#[cfg(unix)]
#[test]
fn list_ignores_non_pip_installers_from_pip_inspect() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");
    let meta_dir = prefix.join("conda-meta");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::write(
        meta_dir.join("pip-24.0-0.json"),
        r#"{
  "name":"pip",
  "version":"24.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/noarch/pip-24.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"pip-24.0-0",
  "spec":"pip",
  "source":"conda",
  "depends":["python >=3.12,<3.13"],
  "installed_at":"now",
  "platform":"noarch"
}"#,
    )
    .expect("write pip record");
    fs::write(meta_dir.join("history"), "").expect("write history");

    let bin_dir = prefix.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin");
    let python = bin_dir.join("python");
    fs::write(
        &python,
        "#!/bin/sh\nprintf '{\"installed\":[{\"installer\":\"poetry\",\"metadata\":{\"name\":\"poetry-plugin\",\"version\":\"1.0\"}},{\"installer\":\"uv\",\"metadata\":{\"name\":\"uvicorn\",\"version\":\"0.30.0\"}}]}'\n",
    )
    .expect("write fake python");
    let mut perms = fs::metadata(&python).expect("python meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&python, perms).expect("chmod python");

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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let names = body["data"]["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .filter_map(|pkg| pkg["name"].as_str())
        .collect::<Vec<_>>();
    assert!(!names.contains(&"poetry-plugin"));
    assert!(names.contains(&"uvicorn"));
}

#[cfg(unix)]
#[test]
fn list_fails_when_pip_inspect_exits_with_non_zero_status() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");
    let meta_dir = prefix.join("conda-meta");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::write(
        meta_dir.join("pip-24.0-0.json"),
        r#"{
  "name":"pip",
  "version":"24.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/noarch/pip-24.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"pip-24.0-0",
  "spec":"pip",
  "source":"conda",
  "depends":["python >=3.12,<3.13"],
  "installed_at":"now",
  "platform":"noarch"
}"#,
    )
    .expect("write pip record");
    fs::write(meta_dir.join("history"), "").expect("write history");

    let bin_dir = prefix.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin");
    let python = bin_dir.join("python");
    fs::write(&python, "#!/bin/sh\nexit 1\n").expect("write fake python");
    let mut perms = fs::metadata(&python).expect("python meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&python, perms).expect("chmod python");

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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let err = body["error"].as_str().unwrap_or_default();
    assert!(err.contains("pip inspect exited with status"));
}

#[cfg(unix)]
#[test]
fn list_fails_when_pip_inspect_returns_invalid_json() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().expect("create temp dir");
    let prefix = tmp.path().join("env");
    let meta_dir = prefix.join("conda-meta");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::write(
        meta_dir.join("pip-24.0-0.json"),
        r#"{
  "name":"pip",
  "version":"24.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/noarch/pip-24.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"pip-24.0-0",
  "spec":"pip",
  "source":"conda",
  "depends":["python >=3.12,<3.13"],
  "installed_at":"now",
  "platform":"noarch"
}"#,
    )
    .expect("write pip record");
    fs::write(meta_dir.join("history"), "").expect("write history");

    let bin_dir = prefix.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin");
    let python = bin_dir.join("python");
    fs::write(&python, "#!/bin/sh\nprintf '{not-json}'\n").expect("write fake python");
    let mut perms = fs::metadata(&python).expect("python meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&python, perms).expect("chmod python");

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
    let body: Value = serde_json::from_slice(&output).expect("valid json");
    let err = body["error"].as_str().unwrap_or_default();
    assert!(err.contains("pip inspect returned invalid JSON"));
}

#[cfg(unix)]
#[test]
fn remove_python_succeeds_while_executable_is_in_use() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let meta_dir = prefix.join("conda-meta");
    let bin_dir = prefix.join("bin");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::create_dir_all(&bin_dir).expect("create bin");
    fs::write(
        meta_dir.join("python-3.12.0-0.json"),
        r#"{
  "name":"python",
  "version":"3.12.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"python-3.12.0-0",
  "spec":"python",
  "source":"conda",
  "depends":[],
  "installed_at":"now",
  "platform":"linux-64"
}"#,
    )
    .expect("write python record");
    fs::write(
        meta_dir.join("history"),
        "==> 2026-03-23 00:00:00 <==\n# create specs: [\"python\"]\n+ python-3.12.0-0\n",
    )
    .expect("write history");
    let python = bin_dir.join("python");
    fs::write(&python, "#!/bin/sh\nsleep 10\n").expect("write python executable");
    let mut perms = fs::metadata(&python).expect("python meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&python, perms).expect("chmod python");

    let mut child = std::process::Command::new(&python)
        .spawn()
        .expect("spawn python process");
    std::thread::sleep(std::time::Duration::from_secs(1));

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    let output = remove
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
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
        body["data"]["removed_names"]
            .as_array()
            .is_some_and(|names| names.iter().any(|item| item.as_str() == Some("python")))
    );

    assert!(!python.exists(), "in-use executable should be removed from prefix");
    assert!(installed_package_names(&prefix).is_empty());
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
#[test]
fn transaction_cleans_previous_mamba_trash_entries_before_apply() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let meta_dir = prefix.join("conda-meta");
    let bin_dir = prefix.join("bin");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::create_dir_all(&bin_dir).expect("create bin");

    fs::write(
        meta_dir.join("python-3.12.0-0.json"),
        r#"{
  "name":"python",
  "version":"3.12.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"python-3.12.0-0",
  "spec":"python",
  "source":"conda",
  "depends":[],
  "installed_at":"now",
  "platform":"linux-64"
}"#,
    )
    .expect("write python record");
    fs::write(
        meta_dir.join("history"),
        "==> 2026-03-23 00:00:00 <==\n# create specs: [\"python\"]\n+ python-3.12.0-0\n",
    )
    .expect("write history");

    let python = bin_dir.join("python");
    fs::write(&python, "#!/bin/sh\nexit 0\n").expect("write python executable");
    let mut perms = fs::metadata(&python).expect("python meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&python, perms).expect("chmod python");

    let stale_trash = bin_dir.join("python.mamba_trash");
    fs::write(&stale_trash, "stale").expect("write stale trash");
    fs::write(meta_dir.join("mamba_trash.txt"), "bin/python.mamba_trash\n")
        .expect("write trash index");

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    assert!(!stale_trash.exists(), "stale trash file should be cleaned");
    assert!(
        !meta_dir.join("mamba_trash.txt").exists(),
        "trash index should be removed when empty"
    );
}

#[cfg(unix)]
#[test]
fn transaction_cleanup_survives_failpoint_rollback() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let meta_dir = prefix.join("conda-meta");
    let bin_dir = prefix.join("bin");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::create_dir_all(&bin_dir).expect("create bin");

    fs::write(
        meta_dir.join("python-3.12.0-0.json"),
        r#"{
  "name":"python",
  "version":"3.12.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"python-3.12.0-0",
  "spec":"python",
  "source":"conda",
  "depends":[],
  "installed_at":"now",
  "platform":"linux-64"
}"#,
    )
    .expect("write python record");
    fs::write(
        meta_dir.join("history"),
        "==> 2026-03-23 00:00:00 <==\n# create specs: [\"python\"]\n+ python-3.12.0-0\n",
    )
    .expect("write history");

    let python = bin_dir.join("python");
    fs::write(&python, "#!/bin/sh\nexit 0\n").expect("write python executable");
    let mut perms = fs::metadata(&python).expect("python meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&python, perms).expect("chmod python");

    let stale_trash = bin_dir.join("python.mamba_trash");
    fs::write(&stale_trash, "stale").expect("write stale trash");
    fs::write(meta_dir.join("mamba_trash.txt"), "bin/python.mamba_trash\n")
        .expect("write trash index");

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .env("HOME", tmp_home.path())
        .env("VIPER_TX_FAIL_POINT", "after_fetch")
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .failure();

    assert!(!stale_trash.exists(), "trash file should stay cleaned after rollback");
    assert!(
        !meta_dir.join("mamba_trash.txt").exists(),
        "trash index should stay removed after rollback"
    );
}

#[cfg(windows)]
fn windows_seed_python_numpy_prefix(
    prefix: &std::path::Path,
    tmp_home: &std::path::Path,
) -> std::path::PathBuf {
    let meta_dir = prefix.join("conda-meta");
    fs::create_dir_all(&meta_dir).expect("create conda-meta");
    fs::write(
        meta_dir.join("python-3.12.0-0.json"),
        r#"{
  "name":"python",
  "version":"3.12.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/win-64/python-3.12.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"python-3.12.0-0",
  "spec":"python",
  "source":"conda",
  "depends":[],
  "installed_at":"now",
  "platform":"win-64"
}"#,
    )
    .expect("write python record");
    fs::write(
        meta_dir.join("numpy-2.0.0-0.json"),
        r#"{
  "name":"numpy",
  "version":"2.0.0",
  "build_string":"0",
  "channel":"https://conda.anaconda.org/conda-forge",
  "base_url":"https://conda.anaconda.org/conda-forge",
  "url":"https://conda.anaconda.org/conda-forge/win-64/numpy-2.0.0-0.tar.bz2",
  "build_number":0,
  "dist_name":"numpy-2.0.0-0",
  "spec":"numpy",
  "source":"conda",
  "depends":["python >=3.12,<3.13"],
  "installed_at":"now",
  "platform":"win-64"
}"#,
    )
    .expect("write numpy record");
    fs::write(
        meta_dir.join("history"),
        "==> 2026-03-23 00:00:00 <==\n# create specs: [\"python\",\"numpy\"]\n+ python-3.12.0-0\n+ numpy-2.0.0-0\n",
    )
    .expect("write history");

    seed_repodata_cache(
        tmp_home,
        &["https://conda.anaconda.org/conda-forge"],
        &[("python", "3.12.0", "0"), ("numpy", "2.0.0", "0")],
    );

    let python_exe = prefix.join("python.exe");
    let cmd_exe = std::env::var("ComSpec")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "C:\\Windows\\System32\\cmd.exe".to_string());
    fs::copy(cmd_exe, &python_exe).expect("copy executable payload");
    fs::write(prefix.join("numpy.exe"), "numpy payload").expect("write numpy payload");
    python_exe
}

#[cfg(windows)]
fn windows_launch_locked_python(python_exe: &std::path::Path) -> std::process::Child {
    std::process::Command::new(python_exe)
        .args(["/C", "ping", "-n", "20", "127.0.0.1", ">", "NUL"])
        .spawn()
        .expect("spawn locked python process")
}

#[cfg(windows)]
fn windows_trash_entries(prefix: &std::path::Path) -> Option<Vec<String>> {
    let path = prefix.join("conda-meta").join("mamba_trash.txt");
    let raw = fs::read_to_string(path).ok()?;
    Some(
        raw.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>(),
    )
}

#[cfg(windows)]
fn windows_actual_trash_payloads(prefix: &std::path::Path) -> Vec<String> {
    let mut pending = vec![prefix.to_path_buf()];
    let mut payloads = Vec::new();
    while let Some(dir) = pending.pop() {
        let Ok(read_dir) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".mamba_trash"))
                && let Ok(rel) = path.strip_prefix(prefix)
            {
                payloads.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    payloads.sort();
    payloads
}

#[cfg(windows)]
fn windows_assert_trash_index_matches_payloads(prefix: &std::path::Path) {
    use std::collections::BTreeSet;

    let payloads = windows_actual_trash_payloads(prefix)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let entries = windows_trash_entries(prefix)
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeSet<_>>();

    if payloads.is_empty() {
        assert!(
            windows_trash_entries(prefix).is_none(),
            "mamba_trash.txt must be absent when no trash payloads exist"
        );
        return;
    }
    assert!(
        windows_trash_entries(prefix).is_some(),
        "mamba_trash.txt must exist when trash payloads exist"
    );
    assert_eq!(
        entries, payloads,
        "mamba_trash.txt must exactly match the existing .mamba_trash payload set"
    );
}

#[cfg(windows)]
#[test]
fn windows_remove_in_use_creates_and_tracks_trash_survivor() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let python_exe = windows_seed_python_numpy_prefix(&prefix, tmp_home.path());
    let mut child = windows_launch_locked_python(&python_exe);
    std::thread::sleep(std::time::Duration::from_secs(1));

    let mut remove = Command::cargo_bin("viper").expect("binary exists");
    remove
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    assert!(!prefix.join("python.exe").exists(), "python.exe should be removed");
    let trash_path = prefix.join("python.exe.mamba_trash");
    if trash_path.exists() {
        windows_assert_trash_index_matches_payloads(&prefix);
    } else {
        assert!(
            windows_actual_trash_payloads(&prefix).is_empty(),
            "no-trash branch should not leave any .mamba_trash payloads in prefix"
        );
        assert!(
            windows_trash_entries(&prefix).is_none(),
            "mamba_trash.txt should not exist when no trash payload was created"
        );
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
#[test]
fn windows_trash_survives_mutation_while_lock_held_and_cleans_after_release() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let python_exe = windows_seed_python_numpy_prefix(&prefix, tmp_home.path());
    let mut child = windows_launch_locked_python(&python_exe);
    std::thread::sleep(std::time::Duration::from_secs(1));

    let mut remove_python = Command::cargo_bin("viper").expect("binary exists");
    remove_python
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .success();

    let had_trash = prefix.join("python.exe.mamba_trash").exists();
    let before_payloads = windows_actual_trash_payloads(&prefix);
    windows_assert_trash_index_matches_payloads(&prefix);

    let mut remove_numpy = Command::cargo_bin("viper").expect("binary exists");
    remove_numpy
        .env("HOME", tmp_home.path())
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "numpy",
            "--json",
        ])
        .assert()
        .success();

    if had_trash {
        let during_payloads = windows_actual_trash_payloads(&prefix);
        assert_eq!(
            during_payloads, before_payloads,
            "trash payload set should remain unchanged while lock is still held"
        );
        windows_assert_trash_index_matches_payloads(&prefix);
    } else {
        assert!(
            windows_actual_trash_payloads(&prefix).is_empty(),
            "no-trash branch should keep prefix free of .mamba_trash payloads after second mutation"
        );
        assert!(
            windows_trash_entries(&prefix).is_none(),
            "no-trash branch should not create mamba_trash.txt on second mutation"
        );
    }

    let _ = child.kill();
    let _ = child.wait();

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

    assert!(
        windows_actual_trash_payloads(&prefix).is_empty(),
        "all .mamba_trash payloads should be cleaned after lock release and next mutation"
    );
    assert!(
        windows_trash_entries(&prefix).is_none(),
        "trash index should be removed after cleanup"
    );
}

#[cfg(windows)]
#[test]
fn windows_after_unlink_rollback_restores_python_and_retains_trackable_trash() {
    let tmp = tempdir().expect("create temp dir");
    let tmp_home = tempdir().expect("create temp home");
    let prefix = tmp.path().join("env");
    let python_exe = windows_seed_python_numpy_prefix(&prefix, tmp_home.path());
    let mut child = windows_launch_locked_python(&python_exe);
    std::thread::sleep(std::time::Duration::from_secs(1));

    let mut remove_python = Command::cargo_bin("viper").expect("binary exists");
    remove_python
        .env("HOME", tmp_home.path())
        .env("VIPER_TX_FAIL_POINT", "after_unlink")
        .args([
            "--no-rc",
            "remove",
            "-p",
            prefix.to_str().expect("utf8"),
            "python",
            "--json",
        ])
        .assert()
        .failure();

    let names = installed_package_names(&prefix);
    assert!(names.iter().any(|name| name == "python"));
    assert!(prefix.join("python.exe").exists());
    let history = fs::read_to_string(prefix.join("conda-meta").join("history")).unwrap_or_default();
    assert!(
        history.contains("python-3.12.0-0"),
        "rollback should keep python history metadata available"
    );

    windows_assert_trash_index_matches_payloads(&prefix);

    let _ = child.kill();
    let _ = child.wait();

    let mut install_numpy = Command::cargo_bin("viper").expect("binary exists");
    install_numpy
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
        .success();

    assert!(
        windows_actual_trash_payloads(&prefix).is_empty(),
        "all tracked trash payloads should be cleaned after release and later mutation"
    );
    assert!(
        windows_trash_entries(&prefix).is_none(),
        "trash index should be removed after later mutation"
    );
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

fn mamba_lockfile_fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../mamba/micromamba/tests/env_lockfiles")
        .join(name)
}

#[derive(Clone)]
struct PackageSeed {
    name: String,
    version: String,
    build: String,
    build_number: i64,
    depends: Vec<String>,
    md5: Option<String>,
    sha256: Option<String>,
}

impl PackageSeed {
    fn new(name: &str, version: &str, build: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            build: build.to_string(),
            build_number: 0,
            depends: Vec::new(),
            md5: None,
            sha256: None,
        }
    }

    fn depends(mut self, deps: &[&str]) -> Self {
        self.depends = deps.iter().map(ToString::to_string).collect();
        self
    }

    fn md5(mut self, digest: &str) -> Self {
        self.md5 = Some(digest.to_string());
        self
    }

    fn sha256(mut self, digest: &str) -> Self {
        self.sha256 = Some(digest.to_string());
        self
    }
}

fn seed_repodata_cache(home: &Path, channels: &[&str], packages: &[(&str, &str, &str)]) {
    let with_defaults = packages
        .iter()
        .map(|(name, version, build)| PackageSeed::new(name, version, build))
        .collect::<Vec<_>>();
    seed_repodata_cache_with_options(home, channels, &with_defaults);
}

fn seed_repodata_cache_with_options(home: &Path, channels: &[&str], packages: &[PackageSeed]) {
    let cache_root = home.join(".viper").join("pkgs").join("cache");
    fs::create_dir_all(&cache_root).expect("create cache root");

    let packages_json = packages
        .iter()
        .map(|pkg| {
            let filename = format!("{}-{}-{}.tar.bz2", pkg.name, pkg.version, pkg.build);
            (
                filename,
                serde_json::json!({
                    "name": &pkg.name,
                    "version": &pkg.version,
                    "build": &pkg.build,
                    "build_number": pkg.build_number,
                    "depends": &pkg.depends,
                    "md5": &pkg.md5,
                    "sha256": &pkg.sha256,
                }),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();
    let repodata_body = serde_json::json!({ "packages": packages_json });

    for channel in channels {
        for subdir in [current_platform_subdir(), "noarch".to_string()] {
            for repodata_name in ["current_repodata.json", "repodata.json"] {
                let key =
                    cache_name_from_repodata_url(&format!("{channel}/{subdir}/{repodata_name}"));
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
}

fn seed_named_repodata_cache(
    home: &Path,
    channel: &str,
    repodata_name: &str,
    packages: &[(&str, &str, &str)],
) {
    let with_defaults = packages
        .iter()
        .map(|(name, version, build)| PackageSeed::new(name, version, build))
        .collect::<Vec<_>>();
    let cache_root = home.join(".viper").join("pkgs").join("cache");
    fs::create_dir_all(&cache_root).expect("create cache root");

    let packages_json = with_defaults
        .iter()
        .map(|pkg| {
            let filename = format!("{}-{}-{}.tar.bz2", pkg.name, pkg.version, pkg.build);
            (
                filename,
                serde_json::json!({
                    "name": &pkg.name,
                    "version": &pkg.version,
                    "build": &pkg.build,
                    "build_number": pkg.build_number,
                    "depends": &pkg.depends,
                    "md5": &pkg.md5,
                    "sha256": &pkg.sha256,
                }),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();
    let repodata_body = serde_json::json!({ "packages": packages_json });

    for subdir in [current_platform_subdir(), "noarch".to_string()] {
        let key = cache_name_from_repodata_url(&format!("{channel}/{subdir}/{repodata_name}"));
        fs::write(
            cache_root.join(format!("{key}.json")),
            serde_json::to_string(&repodata_body).expect("serialize repodata"),
        )
        .expect("write repodata cache");
        fs::write(
            cache_root.join(format!("{key}.state.json")),
            format!(
                "{{\"fetched_at_epoch_s\":{},\"cache_control\":\"max-age=3600\",\"url\":\"{channel}/{subdir}/{repodata_name}\"}}",
                4_102_444_800u64
            ),
        )
        .expect("write repodata state");
    }
}

fn serve_single_http_file(name: &str, body: &str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test http server");
    let addr = listener.local_addr().expect("local addr");
    let path = format!("/{name}");
    let served_path = path.clone();
    let body = body.to_string();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept connection");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut request_line = String::new();
        reader.read_line(&mut request_line).expect("read request line");
        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).expect("read header line");
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        let requested_path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
        let (status, payload) = if requested_path == served_path {
            ("200 OK", body)
        } else {
            ("404 Not Found", "not found".to_string())
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
        stream.flush().expect("flush response");
        let mut drain = Vec::new();
        let _ = stream.read_to_end(&mut drain);
    });
    (format!("http://127.0.0.1:{}{}", addr.port(), path), handle)
}

fn write_viperrc(home: &Path, content: &str) {
    let rc_dir = home.join(".viper");
    fs::create_dir_all(&rc_dir).expect("create rc dir");
    fs::write(rc_dir.join("viperrc"), content).expect("write viperrc");
}

fn load_installed_records(prefix: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    let meta_dir = prefix.join("conda-meta");
    let entries = fs::read_dir(meta_dir).expect("read conda-meta");
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(path).expect("read package json");
        out.push(serde_json::from_str(&raw).expect("valid package json"));
    }
    out
}

fn installed_package_names(prefix: &Path) -> Vec<String> {
    load_installed_records(prefix)
        .into_iter()
        .filter_map(|record| {
            record
                .get("name")
                .and_then(|name| name.as_str())
                .map(ToString::to_string)
        })
        .collect()
}

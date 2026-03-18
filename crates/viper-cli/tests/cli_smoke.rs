use std::fs;

use assert_cmd::Command;
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

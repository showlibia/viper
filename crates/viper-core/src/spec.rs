use std::fs;
use std::path::Path;

use rattler_conda_types::MatchSpec;
use serde::Deserialize;

use crate::error::CoreError;

#[derive(Debug, Deserialize)]
struct EnvFile {
    name: Option<String>,
    channels: Option<Vec<String>>,
    dependencies: Option<Vec<serde_yaml::Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvSpecFile {
    pub name: Option<String>,
    pub channels: Vec<String>,
    pub conda_specs: Vec<String>,
    pub pip_specs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecFileKind {
    Yaml,
    Classic,
    Explicit,
    Lock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSpecFile {
    pub kind: SpecFileKind,
    pub env: EnvSpecFile,
}

pub fn normalize_spec(spec: &str) -> Result<String, CoreError> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(CoreError::EmptySpec);
    }
    Ok(trimmed.to_string())
}

pub fn parse_match_spec(spec: &str) -> Result<MatchSpec, CoreError> {
    let normalized = normalize_spec(spec)?;
    normalized
        .parse::<MatchSpec>()
        .map_err(|err| CoreError::InvalidSpec(format!("{normalized}: {err}")))
}

pub fn package_name_from_spec(spec: &str) -> Result<String, CoreError> {
    let normalized = normalize_spec(spec)?;
    let name: String = normalized
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .collect();
    if name.is_empty() {
        return Err(CoreError::EmptySpec);
    }
    Ok(name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplicitUrlInfo {
    pub normalized: String,
    pub url_no_fragment: String,
    pub fragment: Option<String>,
    pub filename: String,
    pub dist: String,
    pub name: String,
    pub version: String,
    pub build: String,
    pub base_url: String,
    pub subdir: String,
}

pub fn parse_explicit_url(url: &str) -> Result<ExplicitUrlInfo, CoreError> {
    let normalized = normalize_spec(url)?;
    let (url_no_fragment, fragment) = normalized
        .split_once('#')
        .map_or((normalized.as_str(), ""), |parts| parts);
    let url_no_fragment = url_no_fragment.to_string();
    let fragment = (!fragment.is_empty()).then(|| fragment.to_string());
    let filename = url_no_fragment
        .rsplit('/')
        .next()
        .ok_or_else(|| {
            CoreError::InvalidEnvironmentFile(format!(
                "explicit entry has invalid path: {normalized}"
            ))
        })?
        .to_string();
    let dist = filename
        .strip_suffix(".tar.bz2")
        .or_else(|| filename.strip_suffix(".conda"))
        .ok_or_else(|| {
            CoreError::InvalidEnvironmentFile(format!(
                "explicit entry must end with .tar.bz2 or .conda: {filename}"
            ))
        })?
        .to_string();

    let mut parts = dist.rsplitn(3, '-');
    let build = parts
        .next()
        .ok_or_else(|| {
            CoreError::InvalidEnvironmentFile(format!("explicit entry missing build: {filename}"))
        })?
        .to_string();
    let version = parts
        .next()
        .ok_or_else(|| {
            CoreError::InvalidEnvironmentFile(format!("explicit entry missing version: {filename}"))
        })?
        .to_string();
    let name = parts
        .next()
        .ok_or_else(|| {
            CoreError::InvalidEnvironmentFile(format!("explicit entry missing name: {filename}"))
        })?
        .to_string();

    let (prefix, _) = url_no_fragment.rsplit_once('/').ok_or_else(|| {
        CoreError::InvalidEnvironmentFile(format!("explicit entry has invalid path: {normalized}"))
    })?;
    let (base_url, subdir) = prefix.rsplit_once('/').ok_or_else(|| {
        CoreError::InvalidEnvironmentFile(format!("explicit entry has invalid path: {normalized}"))
    })?;
    let base_url = base_url.to_string();
    let subdir = subdir.to_string();

    Ok(ExplicitUrlInfo {
        normalized,
        url_no_fragment,
        fragment,
        filename,
        dist,
        name,
        version,
        build,
        base_url,
        subdir,
    })
}

pub fn parse_env_file(path: &Path) -> Result<EnvSpecFile, CoreError> {
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or_default();
    if !matches!(ext, "yml" | "yaml") {
        return Err(CoreError::UnsupportedEnvironmentFile);
    }

    let content = fs::read_to_string(path)?;
    let parsed: EnvFile = serde_yaml::from_str(&content)?;

    let mut conda_specs = Vec::new();
    let mut pip_specs = Vec::new();
    if let Some(deps) = parsed.dependencies {
        for dep in deps {
            if let Some(s) = dep.as_str() {
                conda_specs.push(normalize_spec(s)?);
                continue;
            }

            if let Some(mapping) = dep.as_mapping() {
                let pip_key = serde_yaml::Value::String("pip".to_string());
                if mapping.len() != 1 || !mapping.contains_key(&pip_key) {
                    return Err(CoreError::InvalidEnvironmentFile(
                        "unsupported dependency mapping; only 'pip' is allowed".to_string(),
                    ));
                }
                let pip_section = mapping
                    .get(&pip_key)
                    .and_then(|x| x.as_sequence())
                    .ok_or_else(|| {
                        CoreError::InvalidEnvironmentFile(
                            "pip dependency section must be a sequence".to_string(),
                        )
                    })?;
                for entry in pip_section {
                    let spec = entry.as_str().ok_or_else(|| {
                        CoreError::InvalidEnvironmentFile(
                            "pip dependency entries must be strings".to_string(),
                        )
                    })?;
                    pip_specs.push(normalize_spec(spec)?);
                }
                continue;
            }

            return Err(CoreError::InvalidEnvironmentFile(
                "unsupported dependency entry type".to_string(),
            ));
        }
    }

    let name = parsed
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if let Some(env_name) = name.as_ref()
        && (env_name.contains('/') || env_name.contains('\\'))
    {
        return Err(CoreError::InvalidEnvironmentFile(
            "environment name cannot contain path separators".to_string(),
        ));
    }
    let channels = parsed
        .channels
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>();

    Ok(EnvSpecFile {
        name,
        channels,
        conda_specs,
        pip_specs,
    })
}

pub fn parse_spec_file(path: &Path) -> Result<ParsedSpecFile, CoreError> {
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or_default();
    if matches!(ext, "yml" | "yaml") {
        if let Some(lock_env) = parse_yaml_lockfile(path)? {
            return Ok(ParsedSpecFile {
                kind: SpecFileKind::Lock,
                env: lock_env,
            });
        }
        let env = parse_env_file(path)?;
        return Ok(ParsedSpecFile {
            kind: SpecFileKind::Yaml,
            env,
        });
    }
    if ext.eq_ignore_ascii_case("json")
        && let Some(lock_env) = parse_mambajs_lockfile(path)?
    {
        return Ok(ParsedSpecFile {
            kind: SpecFileKind::Lock,
            env: lock_env,
        });
    }

    let content = fs::read_to_string(path)?;
    let lines = content.lines().map(str::trim).collect::<Vec<_>>();
    let first_content = lines
        .iter()
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .copied()
        .ok_or_else(|| CoreError::InvalidEnvironmentFile("got an empty file".to_string()))?;

    if first_content.eq_ignore_ascii_case("@EXPLICIT") {
        let mut conda_specs = Vec::new();
        let mut in_explicit_section = false;
        for line in lines {
            if line.is_empty() || line.starts_with("# platform:") {
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            if line.eq_ignore_ascii_case("@EXPLICIT") {
                in_explicit_section = true;
                continue;
            }
            if !in_explicit_section {
                continue;
            }
            validate_explicit_url(line)?;
            conda_specs.push(line.to_string());
        }
        return Ok(ParsedSpecFile {
            kind: SpecFileKind::Explicit,
            env: EnvSpecFile {
                name: None,
                channels: Vec::new(),
                conda_specs,
                pip_specs: Vec::new(),
            },
        });
    }

    let mut conda_specs = Vec::new();
    for line in lines {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let normalized = normalize_spec(line)?;
        parse_match_spec(&normalized)?;
        conda_specs.push(normalized);
    }
    Ok(ParsedSpecFile {
        kind: SpecFileKind::Classic,
        env: EnvSpecFile {
            name: None,
            channels: Vec::new(),
            conda_specs,
            pip_specs: Vec::new(),
        },
    })
}

fn validate_explicit_url(url: &str) -> Result<(), CoreError> {
    let _ = parse_explicit_url(url)?;
    Ok(())
}

fn parse_yaml_lockfile(path: &Path) -> Result<Option<EnvSpecFile>, CoreError> {
    let content = fs::read_to_string(path)?;
    let root = match serde_yaml::from_str::<serde_yaml::Value>(&content) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let Some(mapping) = root.as_mapping() else {
        return Ok(None);
    };
    let package_key = serde_yaml::Value::String("package".to_string());
    let Some(packages) = mapping
        .get(&package_key)
        .and_then(|value| value.as_sequence())
    else {
        return Ok(None);
    };

    let current_platform = current_platform_subdir();
    let mut conda_specs = Vec::new();
    let mut pip_specs = Vec::new();
    for package in packages {
        let Some(entry) = package.as_mapping() else {
            continue;
        };
        if !yaml_lock_entry_matches_platform(entry, &current_platform) {
            continue;
        }
        let manager = entry
            .get(serde_yaml::Value::String("manager".to_string()))
            .and_then(|value| value.as_str())
            .unwrap_or("conda");
        if manager == "pip" {
            if let Some(name) = entry
                .get(serde_yaml::Value::String("name".to_string()))
                .and_then(|value| value.as_str())
            {
                let version = entry
                    .get(serde_yaml::Value::String("version".to_string()))
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                pip_specs.push(format!("{name}=={version}"));
                continue;
            }
            if let Some(url) = entry
                .get(serde_yaml::Value::String("url".to_string()))
                .and_then(|value| value.as_str())
            {
                pip_specs.push(url.to_string());
            }
            continue;
        }
        if let Some(url) = entry
            .get(serde_yaml::Value::String("url".to_string()))
            .and_then(|value| value.as_str())
        {
            let explicit = parse_explicit_url(url)?;
            if explicit.subdir != current_platform && explicit.subdir != "noarch" {
                continue;
            }
            conda_specs.push(url.to_string());
            continue;
        }
        let Some(name) = entry
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        if manager == "conda" {
            return Err(CoreError::InvalidEnvironmentFile(format!(
                "lockfile conda entry for '{name}' is missing required 'url'"
            )));
        }
        let version = entry
            .get(serde_yaml::Value::String("version".to_string()))
            .and_then(|value| value.as_str())
            .unwrap_or("*");
        let build = entry
            .get(serde_yaml::Value::String("build".to_string()))
            .and_then(|value| value.as_str());
        match build {
            Some(build) => conda_specs.push(format!("{name}={version}={build}")),
            None => conda_specs.push(format!("{name}={version}")),
        }
    }

    Ok(Some(EnvSpecFile {
        name: None,
        channels: Vec::new(),
        conda_specs,
        pip_specs,
    }))
}

fn parse_mambajs_lockfile(path: &Path) -> Result<Option<EnvSpecFile>, CoreError> {
    let content = fs::read_to_string(path)?;
    let root = match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if root.get("lockVersion").is_none() || root.get("packages").is_none() {
        return Ok(None);
    }

    let current_platform = current_platform_subdir();
    let mut conda_specs = Vec::new();
    let mut pip_specs = Vec::new();

    if let Some(packages) = root.get("packages").and_then(|value| value.as_object()) {
        for (filename, package) in packages {
            let Some(name) = package.get("name").and_then(|value| value.as_str()) else {
                continue;
            };
            let version = package
                .get("version")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let build = package
                .get("build")
                .and_then(|value| value.as_str())
                .unwrap_or("0");
            let subdir = package
                .get("subdir")
                .and_then(|value| value.as_str())
                .unwrap_or(&current_platform);
            if subdir != current_platform && subdir != "noarch" {
                continue;
            }
            let channel = package
                .get("channel")
                .and_then(|value| value.as_str())
                .unwrap_or("conda-forge");
            let base_url = format!("https://conda.anaconda.org/{channel}");
            let mut spec = format!("{base_url}/{subdir}/{filename}");
            if !spec.ends_with(".conda") && !spec.ends_with(".tar.bz2") {
                spec = format!("{base_url}/{subdir}/{name}-{version}-{build}.conda");
            }
            if let Some(hash) = package.get("hash").and_then(|value| value.as_object()) {
                if let Some(md5) = hash.get("md5").and_then(|value| value.as_str()) {
                    spec.push('#');
                    spec.push_str(md5);
                } else if let Some(sha256) = hash.get("sha256").and_then(|value| value.as_str()) {
                    spec.push('#');
                    spec.push_str(sha256);
                }
            }
            conda_specs.push(spec);
        }
    }

    if let Some(packages) = root.get("pipPackages").and_then(|value| value.as_object()) {
        for package in packages.values() {
            let Some(name) = package.get("name").and_then(|value| value.as_str()) else {
                continue;
            };
            let version = package
                .get("version")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            pip_specs.push(format!("{name}=={version}"));
        }
    }

    Ok(Some(EnvSpecFile {
        name: None,
        channels: Vec::new(),
        conda_specs,
        pip_specs,
    }))
}

fn yaml_lock_entry_matches_platform(entry: &serde_yaml::Mapping, current_platform: &str) -> bool {
    let entry_platform = entry
        .get(serde_yaml::Value::String("platform".to_string()))
        .and_then(|value| value.as_str());
    match entry_platform {
        Some(platform) if platform == current_platform || platform == "noarch" => true,
        Some(_) => false,
        None => true,
    }
}

fn current_platform_subdir() -> String {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-64".to_string(),
        ("linux", "aarch64") => "linux-aarch64".to_string(),
        ("macos", "x86_64") => "osx-64".to_string(),
        ("macos", "aarch64") => "osx-arm64".to_string(),
        ("windows", "x86_64") => "win-64".to_string(),
        (os, arch) => format!("{os}-{arch}"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_name_from_spec() {
        let name = package_name_from_spec("python>=3.11").expect("valid spec");
        assert_eq!(name, "python");
    }

    #[test]
    fn parse_match_spec_rejects_invalid_input() {
        let err = parse_match_spec("!bad").expect_err("must reject invalid match spec");
        assert!(matches!(err, CoreError::InvalidSpec(_)));
    }

    #[test]
    fn parse_environment_file_with_name_channels_and_pip() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("env.yaml");
        fs::write(
            &file,
            r#"
name: myenv
channels:
  - conda-forge
dependencies:
  - python >=3.11
  - pip
  - pip:
      - numpy==2.0.0
"#,
        )
        .expect("write env yaml");

        let parsed = parse_env_file(&file).expect("must parse");
        assert_eq!(parsed.name.as_deref(), Some("myenv"));
        assert_eq!(parsed.channels, vec!["conda-forge".to_string()]);
        assert_eq!(
            parsed.conda_specs,
            vec!["python >=3.11".to_string(), "pip".to_string()]
        );
        assert_eq!(parsed.pip_specs, vec!["numpy==2.0.0".to_string()]);
    }

    #[test]
    fn parse_environment_file_without_dependencies_is_valid() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("env.yaml");
        fs::write(
            &file,
            r#"
name: empty
channels:
  - conda-forge
"#,
        )
        .expect("write env yaml");

        let parsed = parse_env_file(&file).expect("must parse");
        assert_eq!(parsed.name.as_deref(), Some("empty"));
        assert!(parsed.conda_specs.is_empty());
        assert!(parsed.pip_specs.is_empty());
    }

    #[test]
    fn parse_environment_file_rejects_non_string_pip_entry() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("env.yaml");
        fs::write(
            &file,
            r#"
dependencies:
  - pip:
      - numpy==2.0.0
      - 3
"#,
        )
        .expect("write env yaml");

        let err = parse_env_file(&file).expect_err("must reject invalid pip entry");
        assert!(matches!(err, CoreError::InvalidEnvironmentFile(_)));
        assert!(
            err.to_string()
                .contains("pip dependency entries must be strings")
        );
    }

    #[test]
    fn parse_environment_file_rejects_unknown_dependency_mapping() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("env.yaml");
        fs::write(
            &file,
            r#"
dependencies:
  - conda:
      - python
"#,
        )
        .expect("write env yaml");

        let err = parse_env_file(&file).expect_err("must reject unsupported mapping");
        assert!(matches!(err, CoreError::InvalidEnvironmentFile(_)));
        assert!(err.to_string().contains("unsupported dependency mapping"));
    }

    #[test]
    fn parse_environment_file_rejects_pip_non_sequence() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("env.yaml");
        fs::write(
            &file,
            r#"
dependencies:
  - pip: numpy==2.0.0
"#,
        )
        .expect("write env yaml");

        let err = parse_env_file(&file).expect_err("must reject pip non-sequence");
        assert!(matches!(err, CoreError::InvalidEnvironmentFile(_)));
        assert!(
            err.to_string()
                .contains("pip dependency section must be a sequence")
        );
    }

    #[test]
    fn parse_environment_file_rejects_non_string_dependency_entry() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("env.yaml");
        fs::write(
            &file,
            r#"
dependencies:
  - 3
"#,
        )
        .expect("write env yaml");

        let err = parse_env_file(&file).expect_err("must reject unsupported dependency entry");
        assert!(matches!(err, CoreError::InvalidEnvironmentFile(_)));
        assert!(
            err.to_string()
                .contains("unsupported dependency entry type")
        );
    }

    #[test]
    fn parse_environment_file_rejects_name_with_path_separator() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("env.yaml");
        fs::write(
            &file,
            r#"
name: /tmp/absolute-prefix
dependencies:
  - python
"#,
        )
        .expect("write env yaml");

        let err = parse_env_file(&file).expect_err("must reject env name path");
        assert!(matches!(err, CoreError::InvalidEnvironmentFile(_)));
        assert!(
            err.to_string()
                .contains("environment name cannot contain path separators")
        );
    }

    #[test]
    fn parse_spec_file_supports_classic_specs() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("specs.txt");
        fs::write(&file, "python>=3.11\nnumpy\n").expect("write specs");

        let parsed = parse_spec_file(&file).expect("parse classic");
        assert_eq!(parsed.kind, SpecFileKind::Classic);
        assert_eq!(
            parsed.env.conda_specs,
            vec!["python>=3.11".to_string(), "numpy".to_string()]
        );
    }

    #[test]
    fn parse_spec_file_supports_explicit_urls() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("explicit.txt");
        fs::write(
            &file,
            r#"
@EXPLICIT
https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2
"#,
        )
        .expect("write explicit");

        let parsed = parse_spec_file(&file).expect("parse explicit");
        assert_eq!(parsed.kind, SpecFileKind::Explicit);
        assert_eq!(
            parsed.env.conda_specs,
            vec![
                "https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2"
                    .to_string()
            ]
        );
    }

    #[test]
    fn parse_spec_file_supports_explicit_urls_with_hash_fragment() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("explicit.txt");
        fs::write(
            &file,
            r#"
@EXPLICIT
# platform: linux-64
https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2#deadbeef
"#,
        )
        .expect("write explicit");

        let parsed = parse_spec_file(&file).expect("parse explicit");
        assert_eq!(parsed.kind, SpecFileKind::Explicit);
        assert_eq!(
            parsed.env.conda_specs,
            vec![
                "https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2#deadbeef"
                    .to_string()
            ]
        );
    }

    #[test]
    fn parse_spec_file_rejects_empty_non_yaml_file() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("specs.txt");
        fs::write(&file, "  \n# comment only\n").expect("write empty-ish file");

        let err = parse_spec_file(&file).expect_err("must reject empty file");
        assert!(matches!(err, CoreError::InvalidEnvironmentFile(_)));
        assert!(err.to_string().contains("got an empty file"));
    }

    #[test]
    fn parse_lockfile_as_explicit_source() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("conda-lock.yml");
        fs::write(
            &file,
            r#"
package:
  - manager: conda
    url: https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2
  - manager: pip
    name: rich
    version: 13.7.1
"#,
        )
        .expect("write lockfile");

        let parsed = parse_spec_file(&file).expect("parse lockfile");
        assert_eq!(parsed.kind, SpecFileKind::Lock);
        assert_eq!(parsed.env.conda_specs.len(), 1);
        assert_eq!(parsed.env.pip_specs, vec!["rich==13.7.1".to_string()]);
    }

    #[test]
    fn parse_lockfile_rejects_conda_entry_without_url() {
        let tmp = tempdir().expect("create temp dir");
        let file = tmp.path().join("conda-lock.yml");
        fs::write(
            &file,
            r#"
package:
  - manager: conda
    name: python
    version: 3.12.0
"#,
        )
        .expect("write lockfile");

        let err = parse_spec_file(&file).expect_err("missing url must fail");
        assert!(matches!(err, CoreError::InvalidEnvironmentFile(_)));
        assert!(err.to_string().contains("missing required 'url'"));
    }

    #[test]
    fn parse_explicit_url_extracts_package_name() {
        let parsed = parse_explicit_url(
            "https://conda.anaconda.org/conda-forge/linux-64/python-3.12.0-0.tar.bz2#deadbeef",
        )
        .expect("parse explicit url");
        assert_eq!(parsed.name, "python");
        assert_eq!(parsed.version, "3.12.0");
        assert_eq!(parsed.build, "0");
    }

    #[test]
    fn parse_explicit_url_accepts_file_scheme_and_local_paths() {
        let file_scheme =
            parse_explicit_url("file:///tmp/local/linux-64/python-3.12.0-0.conda#deadbeef")
                .expect("parse file scheme");
        assert_eq!(file_scheme.base_url, "file:///tmp/local");
        assert_eq!(file_scheme.subdir, "linux-64");
        assert_eq!(file_scheme.name, "python");

        let local_path =
            parse_explicit_url("/tmp/local/noarch/zlib-1.2.13-h0.conda").expect("parse local path");
        assert_eq!(local_path.base_url, "/tmp/local");
        assert_eq!(local_path.subdir, "noarch");
        assert_eq!(local_path.name, "zlib");
    }
}

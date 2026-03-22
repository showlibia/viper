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
        let env = parse_env_file(path)?;
        return Ok(ParsedSpecFile {
            kind: SpecFileKind::Yaml,
            env,
        });
    }

    let content = fs::read_to_string(path)?;
    let lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return Ok(ParsedSpecFile {
            kind: SpecFileKind::Classic,
            env: EnvSpecFile {
                name: None,
                channels: Vec::new(),
                conda_specs: Vec::new(),
                pip_specs: Vec::new(),
            },
        });
    }

    if lines[0].eq_ignore_ascii_case("@EXPLICIT") {
        let mut conda_specs = Vec::new();
        for line in lines.into_iter().skip(1) {
            let spec = explicit_url_to_spec(line)?;
            parse_match_spec(&spec)?;
            conda_specs.push(spec);
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

fn explicit_url_to_spec(url: &str) -> Result<String, CoreError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(CoreError::InvalidEnvironmentFile(format!(
            "explicit entry must be a URL: {url}"
        )));
    }
    let filename = url.rsplit('/').next().ok_or_else(|| {
        CoreError::InvalidEnvironmentFile(format!("explicit entry has invalid URL: {url}"))
    })?;
    let stem = filename
        .strip_suffix(".tar.bz2")
        .or_else(|| filename.strip_suffix(".conda"))
        .ok_or_else(|| {
            CoreError::InvalidEnvironmentFile(format!(
                "explicit entry must end with .tar.bz2 or .conda: {filename}"
            ))
        })?;
    let mut parts = stem.rsplitn(3, '-');
    let build = parts.next().ok_or_else(|| {
        CoreError::InvalidEnvironmentFile(format!("explicit entry missing build: {filename}"))
    })?;
    let version = parts.next().ok_or_else(|| {
        CoreError::InvalidEnvironmentFile(format!("explicit entry missing version: {filename}"))
    })?;
    let name = parts.next().ok_or_else(|| {
        CoreError::InvalidEnvironmentFile(format!("explicit entry missing name: {filename}"))
    })?;
    Ok(format!("{name}={version}={build}"))
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
        assert_eq!(parsed.env.conda_specs, vec!["python=3.12.0=0".to_string()]);
    }
}

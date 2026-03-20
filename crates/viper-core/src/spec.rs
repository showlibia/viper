use std::fs;
use std::path::Path;

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

pub fn normalize_spec(spec: &str) -> Result<String, CoreError> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(CoreError::EmptySpec);
    }
    Ok(trimmed.to_string())
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
                if let Some(pip_section) = mapping.get(&pip_key).and_then(|x| x.as_sequence()) {
                    for entry in pip_section {
                        if let Some(s) = entry.as_str() {
                            pip_specs.push(normalize_spec(s)?);
                        }
                    }
                }
            }
        }
    }

    let name = parsed
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
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
}

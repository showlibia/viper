use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::error::CoreError;

#[derive(Debug, Deserialize)]
struct EnvFile {
    dependencies: Option<Vec<serde_yaml::Value>>,
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

pub fn parse_env_file(path: &Path) -> Result<Vec<String>, CoreError> {
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or_default();
    if !matches!(ext, "yml" | "yaml") {
        return Err(CoreError::UnsupportedEnvironmentFile);
    }

    let content = fs::read_to_string(path)?;
    let parsed: EnvFile = serde_yaml::from_str(&content)?;

    let mut out = Vec::new();
    if let Some(deps) = parsed.dependencies {
        for dep in deps {
            if let Some(s) = dep.as_str() {
                out.push(normalize_spec(s)?);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_name_from_spec() {
        let name = package_name_from_spec("python>=3.11").expect("valid spec");
        assert_eq!(name, "python");
    }
}

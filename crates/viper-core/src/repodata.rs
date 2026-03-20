use std::collections::HashMap;
use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;

use crate::error::CoreError;

#[derive(Debug, Clone)]
pub struct RepoPackage {
    pub name: String,
    pub version: String,
    pub build: String,
    pub channel: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct RepodataRecord {
    name: String,
    version: String,
    build: String,
}

#[derive(Debug, Default, Deserialize)]
struct RepodataFile {
    #[serde(default)]
    packages: HashMap<String, RepodataRecord>,
    #[serde(default, rename = "packages.conda")]
    packages_conda: HashMap<String, RepodataRecord>,
}

pub fn fetch_packages(
    channels: &[String],
    platform_subdir: &str,
    offline: bool,
) -> Result<Vec<RepoPackage>, CoreError> {
    if offline {
        return Err(CoreError::OfflineRepodataUnavailable);
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| CoreError::Network(e.to_string()))?;

    let mut out = Vec::new();
    for channel in channels {
        let normalized = normalize_channel(channel);
        for subdir in [platform_subdir, "noarch"] {
            let url = format!("{normalized}/{subdir}/current_repodata.json");
            let response = client
                .get(&url)
                .send()
                .map_err(|e| CoreError::Network(e.to_string()))?;
            if !response.status().is_success() {
                continue;
            }
            let repodata: RepodataFile = response
                .json()
                .map_err(|e| CoreError::InvalidRepodata(e.to_string()))?;
            out.extend(parse_records(&normalized, subdir, repodata));
        }
    }
    Ok(out)
}

fn parse_records(channel: &str, subdir: &str, repodata: RepodataFile) -> Vec<RepoPackage> {
    let mut out = Vec::new();
    for (filename, record) in repodata.packages {
        out.push(RepoPackage {
            name: record.name,
            version: record.version,
            build: record.build,
            channel: channel.to_string(),
            url: format!("{channel}/{subdir}/{filename}"),
        });
    }
    for (filename, record) in repodata.packages_conda {
        out.push(RepoPackage {
            name: record.name,
            version: record.version,
            build: record.build,
            channel: channel.to_string(),
            url: format!("{channel}/{subdir}/{filename}"),
        });
    }
    out
}

fn normalize_channel(channel: &str) -> String {
    let trimmed = channel.trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    format!("https://conda.anaconda.org/{trimmed}")
}

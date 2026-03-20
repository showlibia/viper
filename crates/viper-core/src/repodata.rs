use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use reqwest::header::{
    CACHE_CONTROL, ETAG, HeaderMap, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED,
};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheMeta {
    fetched_at_epoch_s: u64,
    cache_control: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
    url: Option<String>,
}

pub fn fetch_packages(
    channels: &[String],
    platform_subdir: &str,
    offline: bool,
    cache_root: &Path,
    local_repodata_ttl: usize,
    repodata_filename: &str,
) -> Result<Vec<RepoPackage>, CoreError> {
    let client = if offline {
        None
    } else {
        Some(
            Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|e| CoreError::Network(e.to_string()))?,
        )
    };

    let mut out = Vec::new();
    let mut loaded_any = false;
    let mut last_err = None;
    for channel in channels {
        let normalized = normalize_channel(channel);
        for subdir in [platform_subdir, "noarch"] {
            let entry = match fetch_subdir(
                client.as_ref(),
                &normalized,
                subdir,
                offline,
                cache_root,
                local_repodata_ttl,
                repodata_filename,
            ) {
                Ok(entry) => entry,
                Err(err) => {
                    last_err = Some(err);
                    continue;
                }
            };
            loaded_any = true;
            out.extend(parse_records(&normalized, subdir, entry));
        }
    }
    if !loaded_any {
        return Err(last_err.unwrap_or(CoreError::OfflineRepodataUnavailable));
    }
    Ok(out)
}

fn fetch_subdir(
    client: Option<&Client>,
    channel: &str,
    subdir: &str,
    offline: bool,
    cache_root: &Path,
    local_repodata_ttl: usize,
    repodata_filename: &str,
) -> Result<RepodataFile, CoreError> {
    let url = format!("{channel}/{subdir}/{repodata_filename}");
    let paths = cache_paths(cache_root, &url);
    let cached_json = read_cached_json(&paths.json);
    let cached_meta = read_cached_meta(&paths.meta);

    if offline {
        if let Some(raw) = cached_json {
            return parse_repodata(&raw);
        }
        return Err(CoreError::OfflineRepodataUnavailable);
    }

    if should_use_cache(local_repodata_ttl, cached_meta.as_ref(), &cached_json)
        && let Some(raw) = cached_json
    {
        return parse_repodata(&raw);
    }

    let client = client.ok_or_else(|| CoreError::Network("client is unavailable".to_string()))?;
    let mut request = client.get(&url);
    if let Some(meta) = cached_meta.as_ref() {
        if let Some(etag) = meta.etag.as_deref() {
            request = request.header(IF_NONE_MATCH, etag);
        }
        if let Some(last_modified) = meta.last_modified.as_deref() {
            request = request.header(IF_MODIFIED_SINCE, last_modified);
        }
    }
    let resp = request
        .send()
        .map_err(|e| CoreError::Network(e.to_string()))?;
    if resp.status().as_u16() == 304
        && let Some(raw) = cached_json.as_deref()
    {
        let meta = CacheMeta {
            fetched_at_epoch_s: now_epoch_s(),
            cache_control: header_value(resp.headers(), CACHE_CONTROL)
                .or_else(|| cached_meta.as_ref().and_then(|m| m.cache_control.clone())),
            etag: header_value(resp.headers(), ETAG)
                .or_else(|| cached_meta.as_ref().and_then(|m| m.etag.clone())),
            last_modified: header_value(resp.headers(), LAST_MODIFIED)
                .or_else(|| cached_meta.as_ref().and_then(|m| m.last_modified.clone())),
            url: Some(url.clone()),
        };
        write_meta(&paths.meta, &meta)?;
        return parse_repodata(raw);
    }

    if resp.status().is_success() {
        let cache_control = header_value(resp.headers(), CACHE_CONTROL);
        let etag = header_value(resp.headers(), ETAG);
        let last_modified = header_value(resp.headers(), LAST_MODIFIED);
        let raw = resp.text().map_err(|e| CoreError::Network(e.to_string()))?;
        write_cache(
            &paths,
            &raw,
            CacheMeta {
                fetched_at_epoch_s: now_epoch_s(),
                cache_control,
                etag,
                last_modified,
                url: Some(url),
            },
        )?;
        return parse_repodata(&raw);
    }

    if let Some(raw) = cached_json {
        return parse_repodata(&raw);
    }
    Err(CoreError::Network(format!(
        "repodata request failed with status {} for {url}",
        resp.status()
    )))
}

fn parse_repodata(raw: &str) -> Result<RepodataFile, CoreError> {
    serde_json::from_str(raw).map_err(|e| CoreError::InvalidRepodata(e.to_string()))
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

fn should_use_cache(
    local_repodata_ttl: usize,
    meta: Option<&CacheMeta>,
    raw: &Option<String>,
) -> bool {
    if raw.is_none() {
        return false;
    }
    if local_repodata_ttl == 0 {
        return false;
    }
    let meta = match meta {
        Some(x) => x,
        None => return false,
    };
    let now = now_epoch_s();
    if meta.fetched_at_epoch_s >= now {
        return true;
    }
    let age = now - meta.fetched_at_epoch_s;
    let max_age = if local_repodata_ttl == 1 {
        cache_control_max_age(meta.cache_control.as_deref()).unwrap_or(3600)
    } else {
        local_repodata_ttl as u64
    };
    age < max_age
}

fn write_cache(paths: &CachePaths, raw: &str, meta: CacheMeta) -> Result<(), CoreError> {
    if let Some(parent) = paths.json.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&paths.json, raw)?;
    write_meta(&paths.meta, &meta)?;
    Ok(())
}

fn write_meta(path: &Path, meta: &CacheMeta) -> Result<(), CoreError> {
    fs::write(path, serde_json::to_vec_pretty(meta)?)?;
    Ok(())
}

fn read_cached_json(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn read_cached_meta(path: &Path) -> Option<CacheMeta> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn cache_control_max_age(header: Option<&str>) -> Option<u64> {
    let header = header?;
    for part in header.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("max-age=")
            && let Ok(v) = value.parse::<u64>()
        {
            return Some(v);
        }
    }
    None
}

struct CachePaths {
    json: PathBuf,
    meta: PathBuf,
}

fn cache_paths(cache_root: &Path, repodata_url: &str) -> CachePaths {
    let key = cache_name_from_url(repodata_url);
    CachePaths {
        json: cache_root.join(format!("{key}.json")),
        meta: cache_root.join(format!("{key}.state.json")),
    }
}

fn cache_name_from_url(url: &str) -> String {
    let mut normalized = url.trim_end_matches('/').to_string();
    if normalized.ends_with("/repodata.json") {
        normalized.truncate(normalized.len().saturating_sub("/repodata.json".len()));
    }
    let digest = md5::compute(normalized.as_bytes());
    format!("{digest:x}")[..8].to_string()
}

fn header_value(headers: &HeaderMap, header: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(header)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}

fn normalize_channel(channel: &str) -> String {
    let trimmed = channel.trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    format!("https://conda.anaconda.org/{trimmed}")
}

fn now_epoch_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn cache_control_parses_max_age() {
        assert_eq!(
            cache_control_max_age(Some("public, max-age=7200, stale-while-revalidate=30")),
            Some(7200)
        );
        assert_eq!(cache_control_max_age(Some("no-cache")), None);
        assert_eq!(cache_control_max_age(None), None);
    }

    #[test]
    fn offline_uses_cached_repodata() {
        let tmp = tempdir().expect("temp dir");
        let cache_root = tmp.path().join("cache");
        fs::create_dir_all(&cache_root).expect("create cache dir");
        let channel = "https://conda.anaconda.org/conda-forge";
        let subdir = "linux-64";
        let paths = cache_paths(
            &cache_root,
            &format!("{channel}/{subdir}/current_repodata.json"),
        );
        fs::write(
            &paths.json,
            r#"{"packages":{"python-3.12.0-0.tar.bz2":{"name":"python","version":"3.12.0","build":"0"}}}"#,
        )
        .expect("write json cache");
        fs::write(
            &paths.meta,
            serde_json::to_vec(&CacheMeta {
                fetched_at_epoch_s: now_epoch_s().saturating_sub(10),
                cache_control: Some("max-age=300".to_string()),
                etag: None,
                last_modified: None,
                url: None,
            })
            .expect("serialize meta"),
        )
        .expect("write meta");

        let pkgs = fetch_packages(
            &[channel.to_string()],
            subdir,
            true,
            &cache_root,
            1,
            "current_repodata.json",
        )
        .expect("must load from cache");
        assert!(pkgs.iter().any(|p| p.name == "python"));
    }

    #[test]
    fn full_and_current_repodata_use_distinct_cache_files() {
        let cache_root = PathBuf::from("/tmp/cache");
        let channel = "https://conda.anaconda.org/conda-forge";
        let subdir = "linux-64";
        let full = cache_paths(&cache_root, &format!("{channel}/{subdir}/repodata.json"));
        let current = cache_paths(
            &cache_root,
            &format!("{channel}/{subdir}/current_repodata.json"),
        );
        assert_ne!(full.json, current.json);
        assert_ne!(full.meta, current.meta);
    }

    #[test]
    fn cache_name_matches_mamba_style_hash_shape() {
        let url = "https://conda.anaconda.org/conda-forge/linux-64/current_repodata.json";
        let key = cache_name_from_url(url);
        assert_eq!(key.len(), 8);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

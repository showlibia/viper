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
    pub build_number: i64,
    pub subdir: String,
    pub filename: String,
    pub depends: Vec<String>,
    pub constrains: Vec<String>,
    pub md5: Option<String>,
    pub sha256: Option<String>,
    pub channel: String,
    pub base_url: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct RepodataRecord {
    name: String,
    version: String,
    build: String,
    #[serde(default)]
    build_number: i64,
    #[serde(default)]
    subdir: Option<String>,
    #[serde(default)]
    depends: Vec<String>,
    #[serde(default)]
    constrains: Vec<String>,
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
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
    for channel in channels {
        let normalized = normalize_channel(channel);
        for subdir in [platform_subdir, "noarch"] {
            let entry = fetch_subdir(
                client.as_ref(),
                &normalized,
                subdir,
                offline,
                cache_root,
                local_repodata_ttl,
                repodata_filename,
            )?;
            out.extend(parse_records(&normalized, subdir, entry));
        }
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
        let subdir = record.subdir.unwrap_or_else(|| subdir.to_string());
        out.push(RepoPackage {
            name: record.name,
            version: record.version,
            build: record.build,
            build_number: record.build_number,
            subdir: subdir.clone(),
            filename: filename.clone(),
            depends: record.depends,
            constrains: record.constrains,
            md5: record.md5,
            sha256: record.sha256,
            channel: channel.to_string(),
            base_url: channel.to_string(),
            url: format!("{channel}/{subdir}/{filename}"),
        });
    }
    for (filename, record) in repodata.packages_conda {
        let subdir = record.subdir.unwrap_or_else(|| subdir.to_string());
        out.push(RepoPackage {
            name: record.name,
            version: record.version,
            build: record.build,
            build_number: record.build_number,
            subdir: subdir.clone(),
            filename: filename.clone(),
            depends: record.depends,
            constrains: record.constrains,
            md5: record.md5,
            sha256: record.sha256,
            channel: channel.to_string(),
            base_url: channel.to_string(),
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
        cache_control_max_age(meta.cache_control.as_deref())
            .or_else(|| {
                if cache_control_requires_revalidation(meta.cache_control.as_deref()) {
                    Some(0)
                } else {
                    None
                }
            })
            .unwrap_or(3600)
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

fn cache_control_requires_revalidation(header: Option<&str>) -> bool {
    let Some(header) = header else {
        return false;
    };
    header
        .split(',')
        .map(|part| part.trim().to_ascii_lowercase())
        .any(|directive| directive == "no-cache" || directive == "must-revalidate")
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
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration as StdDuration;
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
    fn no_cache_requires_immediate_revalidation_for_ttl_one() {
        let raw = Some("{}".to_string());
        let now = now_epoch_s();
        let meta = CacheMeta {
            fetched_at_epoch_s: now.saturating_sub(10),
            cache_control: Some("public, no-cache".to_string()),
            etag: None,
            last_modified: None,
            url: None,
        };
        assert!(!should_use_cache(1, Some(&meta), &raw));
    }

    #[test]
    fn offline_uses_cached_repodata() {
        let tmp = tempdir().expect("temp dir");
        let cache_root = tmp.path().join("cache");
        fs::create_dir_all(&cache_root).expect("create cache dir");
        let channel = "https://conda.anaconda.org/conda-forge";
        let subdir = "linux-64";
        for subdir in [subdir, "noarch"] {
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
        }

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

    #[test]
    fn parse_records_retains_dependency_hash_and_channel_metadata() {
        let raw = r#"{
            "packages": {
                "python-3.12.2-h123_2.conda": {
                    "name": "python",
                    "version": "3.12.2",
                    "build": "h123_2",
                    "build_number": 2,
                    "subdir": "linux-64",
                    "depends": ["libffi >=3.4,<4.0a0", "openssl >=3.0.0"],
                    "constrains": ["python_abi 3.12.* *_cp312"],
                    "md5": "0123456789abcdef0123456789abcdef",
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                }
            }
        }"#;
        let repodata = parse_repodata(raw).expect("parse repodata");
        let packages = parse_records(
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
            repodata,
        );
        let pkg = packages.first().expect("one package");
        assert_eq!(pkg.name, "python");
        assert_eq!(pkg.build_number, 2);
        assert_eq!(pkg.subdir, "linux-64");
        assert_eq!(pkg.filename, "python-3.12.2-h123_2.conda");
        assert_eq!(pkg.base_url, "https://conda.anaconda.org/conda-forge");
        assert_eq!(pkg.depends.len(), 2);
        assert_eq!(pkg.constrains.len(), 1);
        assert!(pkg.md5.is_some());
        assert!(pkg.sha256.is_some());
    }

    #[test]
    fn first_online_fetch_writes_cache_json_and_state_files() {
        let tmp = tempdir().expect("temp dir");
        let cache_root = tmp.path().join("cache");
        let request_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&request_count);
        let server = TestServer::spawn(move |_path, _headers| {
            counter.fetch_add(1, Ordering::SeqCst);
            TestResponse::json(
                200,
                r#"{"packages":{"python-3.12.0-0.tar.bz2":{"name":"python","version":"3.12.0","build":"0"}}}"#,
                vec![
                    ("Cache-Control".to_string(), "max-age=300".to_string()),
                    ("ETag".to_string(), "\"etag-v1\"".to_string()),
                ],
            )
        });
        let channel = format!("{}/conda-forge", server.base_url());

        let packages = fetch_packages(
            std::slice::from_ref(&channel),
            "linux-64",
            false,
            &cache_root,
            0,
            "current_repodata.json",
        )
        .expect("online fetch");
        assert!(packages.iter().any(|p| p.name == "python"));
        assert_eq!(request_count.load(Ordering::SeqCst), 2);

        for subdir in ["linux-64", "noarch"] {
            let paths = cache_paths(
                &cache_root,
                &format!("{channel}/{subdir}/current_repodata.json"),
            );
            assert!(paths.json.exists(), "missing json cache for {subdir}");
            assert!(paths.meta.exists(), "missing state cache for {subdir}");
        }
    }

    #[test]
    fn fresh_ttl_cache_reuses_local_repodata_without_network() {
        let tmp = tempdir().expect("temp dir");
        let cache_root = tmp.path().join("cache");
        let channel = "http://127.0.0.1:9/conda-forge";
        seed_cache_for_both_subdirs(
            &cache_root,
            channel,
            "current_repodata.json",
            now_epoch_s(),
            "max-age=3600",
            r#"{"packages":{"python-3.12.0-0.tar.bz2":{"name":"python","version":"3.12.0","build":"0"}}}"#,
        );

        let packages = fetch_packages(
            &[channel.to_string()],
            "linux-64",
            false,
            &cache_root,
            1,
            "current_repodata.json",
        )
        .expect("must use fresh cache");
        assert!(packages.iter().any(|p| p.name == "python"));
    }

    #[test]
    fn http_304_refreshes_cached_metadata_timestamp() {
        let tmp = tempdir().expect("temp dir");
        let cache_root = tmp.path().join("cache");
        let request_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&request_count);
        let server = TestServer::spawn(move |_path, headers| {
            counter.fetch_add(1, Ordering::SeqCst);
            assert!(headers.contains_key("if-none-match"));
            assert!(headers.contains_key("if-modified-since"));
            TestResponse::empty(
                304,
                vec![
                    ("Cache-Control".to_string(), "max-age=120".to_string()),
                    ("ETag".to_string(), "\"etag-v2\"".to_string()),
                    (
                        "Last-Modified".to_string(),
                        "Tue, 01 Jan 2030 00:00:00 GMT".to_string(),
                    ),
                ],
            )
        });
        let channel = format!("{}/conda-forge", server.base_url());
        seed_cache_for_both_subdirs(
            &cache_root,
            &channel,
            "current_repodata.json",
            1,
            "max-age=0",
            r#"{"packages":{"python-3.12.0-0.tar.bz2":{"name":"python","version":"3.12.0","build":"0"}}}"#,
        );
        set_cache_http_headers(
            &cache_root,
            &channel,
            "current_repodata.json",
            "\"etag-v1\"",
        );

        let before = read_cached_meta(
            &cache_paths(
                &cache_root,
                &format!("{channel}/linux-64/current_repodata.json"),
            )
            .meta,
        )
        .expect("cached meta")
        .fetched_at_epoch_s;

        let packages = fetch_packages(
            std::slice::from_ref(&channel),
            "linux-64",
            false,
            &cache_root,
            0,
            "current_repodata.json",
        )
        .expect("304 path should use cache");
        assert!(packages.iter().any(|p| p.name == "python"));
        assert_eq!(request_count.load(Ordering::SeqCst), 2);

        let after = read_cached_meta(
            &cache_paths(
                &cache_root,
                &format!("{channel}/linux-64/current_repodata.json"),
            )
            .meta,
        )
        .expect("updated meta")
        .fetched_at_epoch_s;
        assert!(after >= before);
    }

    #[test]
    fn remote_failure_falls_back_only_when_cache_exists() {
        let server = TestServer::spawn(move |_path, _headers| {
            TestResponse::empty(
                503,
                vec![("Cache-Control".to_string(), "max-age=0".to_string())],
            )
        });
        let channel = format!("{}/conda-forge", server.base_url());

        let tmp_with_cache = tempdir().expect("temp dir");
        let cache_with = tmp_with_cache.path().join("cache");
        seed_cache_for_both_subdirs(
            &cache_with,
            &channel,
            "current_repodata.json",
            1,
            "max-age=0",
            r#"{"packages":{"python-3.12.0-0.tar.bz2":{"name":"python","version":"3.12.0","build":"0"}}}"#,
        );
        let fallback = fetch_packages(
            std::slice::from_ref(&channel),
            "linux-64",
            false,
            &cache_with,
            0,
            "current_repodata.json",
        )
        .expect("fallback to cache on 5xx");
        assert!(fallback.iter().any(|p| p.name == "python"));

        let tmp_without_cache = tempdir().expect("temp dir");
        let err = fetch_packages(
            &[channel],
            "linux-64",
            false,
            &tmp_without_cache.path().join("cache"),
            0,
            "current_repodata.json",
        )
        .expect_err("must fail without cache");
        match err {
            CoreError::Network(msg) => assert!(msg.contains("503")),
            other => panic!("unexpected error: {other}"),
        }
    }

    fn seed_cache_for_both_subdirs(
        cache_root: &Path,
        channel: &str,
        repodata_filename: &str,
        fetched_at_epoch_s: u64,
        cache_control: &str,
        body: &str,
    ) {
        fs::create_dir_all(cache_root).expect("create cache root");
        for subdir in ["linux-64", "noarch"] {
            let paths = cache_paths(
                cache_root,
                &format!("{channel}/{subdir}/{repodata_filename}"),
            );
            fs::write(&paths.json, body).expect("write repodata json");
            write_meta(
                &paths.meta,
                &CacheMeta {
                    fetched_at_epoch_s,
                    cache_control: Some(cache_control.to_string()),
                    etag: None,
                    last_modified: None,
                    url: Some(format!("{channel}/{subdir}/{repodata_filename}")),
                },
            )
            .expect("write repodata state");
        }
    }

    fn set_cache_http_headers(
        cache_root: &Path,
        channel: &str,
        repodata_filename: &str,
        etag: &str,
    ) {
        for subdir in ["linux-64", "noarch"] {
            let paths = cache_paths(
                cache_root,
                &format!("{channel}/{subdir}/{repodata_filename}"),
            );
            let mut meta = read_cached_meta(&paths.meta).expect("existing meta");
            meta.etag = Some(etag.to_string());
            meta.last_modified = Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string());
            write_meta(&paths.meta, &meta).expect("write updated state");
        }
    }

    struct TestResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl TestResponse {
        fn json(status: u16, body: &str, headers: Vec<(String, String)>) -> Self {
            let mut all_headers = headers;
            all_headers.push(("Content-Type".to_string(), "application/json".to_string()));
            Self {
                status,
                headers: all_headers,
                body: body.to_string(),
            }
        }

        fn empty(status: u16, headers: Vec<(String, String)>) -> Self {
            Self {
                status,
                headers,
                body: String::new(),
            }
        }
    }

    struct TestServer {
        addr: String,
        shutdown: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn spawn<F>(handler: F) -> Self
        where
            F: Fn(String, BTreeMap<String, String>) -> TestResponse + Send + Sync + 'static,
        {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test server");
            listener
                .set_nonblocking(true)
                .expect("set nonblocking listener");
            let addr = format!("http://{}", listener.local_addr().expect("local addr"));
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_flag = Arc::clone(&shutdown);
            let handler = Arc::new(handler);
            let thread_handler = Arc::clone(&handler);
            let join = thread::spawn(move || {
                while !shutdown_flag.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            handle_connection(stream, &thread_handler);
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(StdDuration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                addr,
                shutdown,
                handle: Some(join),
            }
        }

        fn base_url(&self) -> &str {
            &self.addr
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(self.addr.trim_start_matches("http://"));
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn handle_connection<F>(mut stream: TcpStream, handler: &Arc<F>)
    where
        F: Fn(String, BTreeMap<String, String>) -> TestResponse + Send + Sync + 'static,
    {
        stream
            .set_read_timeout(Some(StdDuration::from_secs(1)))
            .expect("set read timeout");
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    request.extend_from_slice(&buf[..n]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => return,
            }
        }
        let req_text = String::from_utf8_lossy(&request);
        let mut lines = req_text.lines();
        let first = lines.next().unwrap_or_default();
        let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
        let mut headers = BTreeMap::new();
        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some((name, value)) = trimmed.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        let resp = handler(path, headers);
        let status_text = match resp.status {
            200 => "OK",
            304 => "Not Modified",
            503 => "Service Unavailable",
            _ => "Status",
        };
        let mut raw = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
            resp.status,
            status_text,
            resp.body.len()
        );
        for (name, value) in resp.headers {
            raw.push_str(&format!("{name}: {value}\r\n"));
        }
        raw.push_str("\r\n");
        raw.push_str(&resp.body);
        let _ = stream.write_all(raw.as_bytes());
    }
}

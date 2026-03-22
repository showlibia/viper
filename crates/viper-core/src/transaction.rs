use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PlannedLink {
    pub name: String,
    pub version: String,
    pub build: String,
    pub build_number: i64,
    pub dist_name: String,
    pub channel: String,
    pub base_url: String,
    pub url: String,
    pub md5: Option<String>,
    pub sha256: Option<String>,
    pub depends: Vec<String>,
    pub platform: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedUnlink {
    pub name: String,
    pub version: String,
    pub build: String,
    pub dist_name: String,
    pub source: String,
}

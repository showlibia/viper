use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PlannedLink {
    pub name: String,
    pub version: String,
    pub build: String,
    pub channel: String,
    pub url: String,
    pub source: String,
}

use std::collections::HashMap;

use serde::Serialize;

use crate::types::PackageRecord;

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

#[derive(Debug, Clone, Serialize)]
pub struct TransactionPlan {
    pub fetch: Vec<PlannedLink>,
    pub extract: Vec<PlannedLink>,
    pub link: Vec<PlannedLink>,
    pub unlink: Vec<PlannedUnlink>,
}

impl TransactionPlan {
    pub fn from_solved(installed: &[PackageRecord], solved: &[PlannedLink]) -> Self {
        let installed_by_name = installed
            .iter()
            .filter(|pkg| pkg.source == "conda")
            .map(|pkg| (pkg.name.clone(), pkg))
            .collect::<HashMap<_, _>>();
        let solved_by_name = solved
            .iter()
            .map(|pkg| (pkg.name.clone(), pkg))
            .collect::<HashMap<_, _>>();

        let mut link = Vec::new();
        for solved_pkg in solved {
            let same = installed_by_name
                .get(&solved_pkg.name)
                .is_some_and(|installed_pkg| {
                    installed_pkg.version == solved_pkg.version
                        && installed_pkg.build_string == solved_pkg.build
                });
            if !same {
                link.push(solved_pkg.clone());
            }
        }

        let mut unlink = Vec::new();
        for installed_pkg in installed.iter().filter(|pkg| pkg.source == "conda") {
            let keep_same = solved_by_name
                .get(&installed_pkg.name)
                .is_some_and(|solved_pkg| {
                    installed_pkg.version == solved_pkg.version
                        && installed_pkg.build_string == solved_pkg.build
                });
            if !keep_same {
                unlink.push(PlannedUnlink {
                    name: installed_pkg.name.clone(),
                    version: installed_pkg.version.clone(),
                    build: installed_pkg.build_string.clone(),
                    dist_name: if installed_pkg.dist_name.is_empty() {
                        format!(
                            "{}-{}-{}",
                            installed_pkg.name, installed_pkg.version, installed_pkg.build_string
                        )
                    } else {
                        installed_pkg.dist_name.clone()
                    },
                    source: installed_pkg.source.clone(),
                });
            }
        }

        Self {
            fetch: link.clone(),
            extract: link.clone(),
            link,
            unlink,
        }
    }
}

use std::str::FromStr;

use rattler_conda_types::{
    MatchSpec, Version,
    version_spec::{LogicalOperator, RangeOperator, VersionSpec},
};

use crate::repodata::RepoPackage;
use crate::spec::package_name_from_spec;
use crate::transaction::PlannedLink;

pub fn solve_to_actions(specs: &[String], packages: &[RepoPackage]) -> Vec<PlannedLink> {
    let mut actions = Vec::new();
    for spec in specs {
        let parsed = spec.parse::<MatchSpec>().ok();
        let name = requested_name(spec, parsed.as_ref());
        if let Some(best) = pick_best_candidate(&name, parsed.as_ref(), packages) {
            actions.push(PlannedLink {
                name: best.name.clone(),
                version: best.version.clone(),
                build: best.build.clone(),
                channel: best.channel.clone(),
                url: best.url.clone(),
                source: "conda".to_string(),
            });
            continue;
        }

        actions.push(PlannedLink {
            name,
            version: parsed
                .and_then(|ms| ms.version.map(|v| v.to_string()))
                .unwrap_or_else(|| "unknown".to_string()),
            build: "unknown".to_string(),
            channel: "unresolved".to_string(),
            url: String::new(),
            source: "conda".to_string(),
        });
    }
    actions
}

fn requested_name(spec: &str, parsed: Option<&MatchSpec>) -> String {
    if let Some(name) = parsed
        .and_then(|ms| ms.name.as_exact())
        .map(|name| name.as_normalized().to_string())
    {
        return name;
    }
    package_name_from_spec(spec).unwrap_or_else(|_| spec.to_string())
}

pub fn spec_requires_full_repodata(spec: &str) -> bool {
    if let Ok(ms) = spec.parse::<MatchSpec>() {
        let version_needs_full = ms.version.as_ref().is_some_and(version_spec_requires_full);
        return version_needs_full
            || ms.build.is_some()
            || ms.build_number.is_some()
            || ms.channel.is_some()
            || ms.subdir.is_some()
            || ms.file_name.is_some()
            || ms.url.is_some()
            || ms.md5.is_some()
            || ms.sha256.is_some();
    }
    spec.chars()
        .any(|c| matches!(c, '<' | '=' | '!' | '[' | ']' | ':'))
}

fn version_spec_requires_full(spec: &VersionSpec) -> bool {
    match spec {
        VersionSpec::Any => false,
        VersionSpec::Range(RangeOperator::Greater, _)
        | VersionSpec::Range(RangeOperator::GreaterEquals, _) => false,
        VersionSpec::Group(LogicalOperator::And, group)
        | VersionSpec::Group(LogicalOperator::Or, group) => {
            group.iter().any(version_spec_requires_full)
        }
        _ => true,
    }
}

fn pick_best_candidate<'a>(
    name: &str,
    spec: Option<&MatchSpec>,
    packages: &'a [RepoPackage],
) -> Option<&'a RepoPackage> {
    packages
        .iter()
        .filter(|p| p.name == name)
        .filter(|p| candidate_matches_spec(spec, p))
        .max_by(|a, b| compare_candidates(a, b))
}

fn candidate_matches_spec(spec: Option<&MatchSpec>, candidate: &RepoPackage) -> bool {
    let Some(spec) = spec else {
        return true;
    };

    if let Some(version_spec) = spec.version.as_ref() {
        let Ok(version) = Version::from_str(&candidate.version) else {
            return false;
        };
        if !version_spec.matches(&version) {
            return false;
        }
    }

    if let Some(build_matcher) = spec.build.as_ref()
        && !build_matcher.matches(&candidate.build)
    {
        return false;
    }

    if let Some(channel) = spec.channel.as_ref() {
        let spec_channel = channel.base_url.as_str().trim_end_matches('/');
        let candidate_channel = candidate.channel.trim_end_matches('/');
        if candidate_channel != spec_channel {
            return false;
        }
    }

    if let Some(subdir) = spec.subdir.as_ref()
        && candidate_subdir(candidate.url.as_str()) != Some(subdir.as_str())
    {
        return false;
    }

    true
}

fn candidate_subdir(url: &str) -> Option<&str> {
    let mut parts = url.split('/').filter(|s| !s.is_empty()).rev();
    let _filename = parts.next()?;
    parts.next()
}

fn compare_candidates(a: &RepoPackage, b: &RepoPackage) -> std::cmp::Ordering {
    match (Version::from_str(&a.version), Version::from_str(&b.version)) {
        (Ok(av), Ok(bv)) => av.cmp(&bv).then_with(|| a.build.cmp(&b.build)),
        (Ok(_), Err(_)) => std::cmp::Ordering::Greater,
        (Err(_), Ok(_)) => std::cmp::Ordering::Less,
        (Err(_), Err(_)) => a
            .version
            .cmp(&b.version)
            .then_with(|| a.build.cmp(&b.build)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, build: &str, channel: &str, subdir: &str) -> RepoPackage {
        RepoPackage {
            name: name.to_string(),
            version: version.to_string(),
            build: build.to_string(),
            build_number: 0,
            subdir: subdir.to_string(),
            filename: format!("{name}-{version}-{build}.conda"),
            depends: Vec::new(),
            constrains: Vec::new(),
            md5: None,
            sha256: None,
            channel: channel.to_string(),
            base_url: channel.to_string(),
            url: format!("{channel}/{subdir}/{name}-{version}-{build}.conda"),
        }
    }

    #[test]
    fn constrained_spec_filters_candidates() {
        let pkgs = vec![
            pkg(
                "python",
                "3.9.19",
                "h1",
                "https://conda.anaconda.org/conda-forge",
                "linux-64",
            ),
            pkg(
                "python",
                "3.11.9",
                "h2",
                "https://conda.anaconda.org/conda-forge",
                "linux-64",
            ),
        ];
        let actions = solve_to_actions(&["python<3.10".to_string()], &pkgs);
        assert_eq!(actions[0].version, "3.9.19");
    }

    #[test]
    fn version_order_uses_conda_semantics() {
        let pkgs = vec![
            pkg(
                "python",
                "3.9",
                "h1",
                "https://conda.anaconda.org/conda-forge",
                "linux-64",
            ),
            pkg(
                "python",
                "3.11",
                "h2",
                "https://conda.anaconda.org/conda-forge",
                "linux-64",
            ),
        ];
        let actions = solve_to_actions(&["python".to_string()], &pkgs);
        assert_eq!(actions[0].version, "3.11");
    }

    #[test]
    fn channel_and_build_constraints_are_applied() {
        let pkgs = vec![
            pkg(
                "numpy",
                "1.26.4",
                "py311_0",
                "https://conda.anaconda.org/conda-forge",
                "linux-64",
            ),
            pkg(
                "numpy",
                "1.26.4",
                "py310_0",
                "https://conda.anaconda.org/bioconda",
                "linux-64",
            ),
        ];
        let actions = solve_to_actions(
            &["conda-forge::numpy[build=\"py311_*\"]".to_string()],
            &pkgs,
        );
        assert_eq!(actions[0].channel, "https://conda.anaconda.org/conda-forge");
        assert_eq!(actions[0].build, "py311_0");
    }

    #[test]
    fn full_repodata_is_only_required_for_pinned_or_restrictive_specs() {
        assert!(!spec_requires_full_repodata("python>=3.11"));
        assert!(spec_requires_full_repodata("python<3.10"));
        assert!(spec_requires_full_repodata("numpy=1.26"));
    }
}

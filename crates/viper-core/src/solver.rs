use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;

use rattler_conda_types::{
    MatchSpec, Version,
    version_spec::{LogicalOperator, RangeOperator, VersionSpec},
};

use crate::repodata::RepoPackage;
use crate::spec::package_name_from_spec;
use crate::transaction::PlannedLink;

#[derive(Debug, Clone)]
pub struct SolveOptions {
    pub channels: Vec<String>,
    pub strict_channel_priority: bool,
    pub installed_preferred: HashMap<String, (String, String)>,
    pub user_requested: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct SolveResult {
    pub actions: Vec<PlannedLink>,
    pub trace: Vec<String>,
}

#[derive(Debug, Clone)]
struct PendingSpec {
    raw: String,
    required_by: Option<String>,
}

pub fn solve_to_actions(
    specs: &[String],
    packages: &[RepoPackage],
    options: &SolveOptions,
) -> Result<SolveResult, Vec<String>> {
    let channel_priority = build_channel_priority_map(&options.channels);
    let mut selected: BTreeMap<String, &RepoPackage> = BTreeMap::new();
    let mut trace = Vec::new();
    let mut conflicts = Vec::new();
    let mut pending = specs
        .iter()
        .map(|raw| PendingSpec {
            raw: raw.clone(),
            required_by: None,
        })
        .collect::<Vec<_>>();

    while let Some(next) = pending.pop() {
        let parsed = next.raw.parse::<MatchSpec>().ok();
        let name = requested_name(&next.raw, parsed.as_ref());

        if let Some(existing) = selected.get(&name) {
            if candidate_matches_spec(parsed.as_ref(), existing) {
                continue;
            }

            let requester = next.required_by.as_deref().unwrap_or("user-requested spec");
            conflicts.push(format!(
                "conflict: {requester} requires '{}' but selected {}={}",
                next.raw, existing.name, existing.version
            ));
            continue;
        }

        let Some(chosen) = pick_best_candidate(
            &name,
            parsed.as_ref(),
            packages,
            options.strict_channel_priority,
            &channel_priority,
            options.installed_preferred.get(&name),
            options.user_requested.contains(&name),
        ) else {
            let requester = next.required_by.as_deref().unwrap_or("user-requested spec");
            conflicts.push(format!(
                "unsatisfied: {requester} requires '{}' (package '{name}')",
                next.raw
            ));
            continue;
        };

        trace.push(format!(
            "selected {}={} build={} channel={} for spec '{}'",
            chosen.name, chosen.version, chosen.build, chosen.channel, next.raw
        ));
        selected.insert(name.clone(), chosen);

        for dep in chosen.depends.iter().rev() {
            let dep_name = package_name_from_spec(dep).unwrap_or_else(|_| dep.clone());
            if selected.contains_key(&dep_name) {
                continue;
            }
            pending.push(PendingSpec {
                raw: dep.clone(),
                required_by: Some(format!("{}={}", chosen.name, chosen.version)),
            });
        }
    }

    if !conflicts.is_empty() {
        let mut seen = HashSet::new();
        let deduped = conflicts
            .into_iter()
            .filter(|line| seen.insert(line.clone()))
            .collect::<Vec<_>>();
        return Err(deduped);
    }

    let actions = selected
        .into_values()
        .map(|best| PlannedLink {
            name: best.name.clone(),
            version: best.version.clone(),
            build: best.build.clone(),
            build_number: best.build_number,
            dist_name: package_dist_name(best),
            channel: best.channel.clone(),
            base_url: best.base_url.clone(),
            url: best.url.clone(),
            md5: best.md5.clone(),
            sha256: best.sha256.clone(),
            depends: best.depends.clone(),
            platform: best.subdir.clone(),
            source: "conda".to_string(),
        })
        .collect::<Vec<_>>();

    Ok(SolveResult { actions, trace })
}

fn package_dist_name(pkg: &RepoPackage) -> String {
    format!("{}-{}-{}", pkg.name, pkg.version, pkg.build)
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
    strict_channel_priority: bool,
    channel_priority: &HashMap<String, usize>,
    installed_preferred: Option<&(String, String)>,
    user_requested: bool,
) -> Option<&'a RepoPackage> {
    let filtered = packages
        .iter()
        .filter(|p| p.name == name)
        .filter(|p| candidate_matches_spec(spec, p))
        .collect::<Vec<_>>();

    if filtered.is_empty() {
        return None;
    }

    let candidates = if strict_channel_priority {
        let top_rank = filtered
            .iter()
            .map(|candidate| channel_rank(channel_priority, candidate.channel.as_str()))
            .min()
            .unwrap_or(usize::MAX);
        filtered
            .into_iter()
            .filter(|candidate| {
                channel_rank(channel_priority, candidate.channel.as_str()) == top_rank
            })
            .collect::<Vec<_>>()
    } else {
        filtered
    };

    if !user_requested
        && let Some((installed_version, installed_build)) = installed_preferred
        && let Some(existing) = candidates.iter().find(|candidate| {
            &candidate.version == installed_version && &candidate.build == installed_build
        })
    {
        return Some(*existing);
    }

    candidates.into_iter().max_by(|a, b| {
        compare_candidates(a, b).then_with(|| compare_channel_rank(a, b, channel_priority))
    })
}

fn compare_channel_rank(
    a: &RepoPackage,
    b: &RepoPackage,
    channel_priority: &HashMap<String, usize>,
) -> std::cmp::Ordering {
    let ar = channel_rank(channel_priority, a.channel.as_str());
    let br = channel_rank(channel_priority, b.channel.as_str());
    br.cmp(&ar)
}

fn channel_rank(channel_priority: &HashMap<String, usize>, channel: &str) -> usize {
    channel_priority
        .get(&normalize_channel(channel))
        .copied()
        .unwrap_or(usize::MAX)
}

fn build_channel_priority_map(channels: &[String]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for (idx, channel) in channels.iter().enumerate() {
        out.entry(normalize_channel(channel)).or_insert(idx);
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
        (Ok(av), Ok(bv)) => av
            .cmp(&bv)
            .then_with(|| a.build_number.cmp(&b.build_number))
            .then_with(|| a.build.cmp(&b.build)),
        (Ok(_), Err(_)) => std::cmp::Ordering::Greater,
        (Err(_), Ok(_)) => std::cmp::Ordering::Less,
        (Err(_), Err(_)) => a
            .version
            .cmp(&b.version)
            .then_with(|| a.build_number.cmp(&b.build_number))
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

    fn options(channels: &[&str], strict_channel_priority: bool) -> SolveOptions {
        SolveOptions {
            channels: channels.iter().map(ToString::to_string).collect(),
            strict_channel_priority,
            installed_preferred: HashMap::new(),
            user_requested: HashSet::new(),
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
        let result = solve_to_actions(
            &["python<3.10".to_string()],
            &pkgs,
            &options(&["conda-forge"], false),
        )
        .expect("solver must resolve");
        let python = result
            .actions
            .iter()
            .find(|action| action.name == "python")
            .expect("python action");
        assert_eq!(python.version, "3.9.19");
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
        let result = solve_to_actions(
            &["python".to_string()],
            &pkgs,
            &options(&["conda-forge"], false),
        )
        .expect("solver must resolve");
        let python = result
            .actions
            .iter()
            .find(|action| action.name == "python")
            .expect("python action");
        assert_eq!(python.version, "3.11");
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
        let result = solve_to_actions(
            &["conda-forge::numpy[build=\"py311_*\"]".to_string()],
            &pkgs,
            &options(&["conda-forge", "bioconda"], false),
        )
        .expect("solver must resolve");
        let numpy = result
            .actions
            .iter()
            .find(|action| action.name == "numpy")
            .expect("numpy action");
        assert_eq!(numpy.channel, "https://conda.anaconda.org/conda-forge");
        assert_eq!(numpy.build, "py311_0");
    }

    #[test]
    fn resolves_transitive_dependencies() {
        let mut python = pkg(
            "python",
            "3.11.9",
            "h123",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );
        python.depends = vec!["openssl >=3.2,<4.0a0".to_string()];

        let openssl = pkg(
            "openssl",
            "3.2.2",
            "h456",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );

        let result = solve_to_actions(
            &["python".to_string()],
            &[python, openssl],
            &options(&["conda-forge"], false),
        )
        .expect("solver must resolve closure");

        assert!(result.actions.iter().any(|action| action.name == "python"));
        assert!(result.actions.iter().any(|action| action.name == "openssl"));
        assert!(!result.trace.is_empty());
    }

    #[test]
    fn strict_channel_priority_prefers_higher_priority_channel() {
        let pkgs = vec![
            pkg(
                "zlib",
                "1.2.13",
                "h1",
                "https://conda.anaconda.org/conda-forge",
                "linux-64",
            ),
            pkg(
                "zlib",
                "1.3.1",
                "h2",
                "https://conda.anaconda.org/defaults",
                "linux-64",
            ),
        ];

        let result = solve_to_actions(
            &["zlib".to_string()],
            &pkgs,
            &options(&["conda-forge", "defaults"], true),
        )
        .expect("solver must resolve");

        let zlib = result
            .actions
            .iter()
            .find(|action| action.name == "zlib")
            .expect("zlib action");
        assert_eq!(zlib.channel, "https://conda.anaconda.org/conda-forge");
    }

    #[test]
    fn reports_conflicts_for_unsatisfied_dependencies() {
        let mut python = pkg(
            "python",
            "3.11.9",
            "h123",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );
        python.depends = vec!["openssl >=3.2,<4.0a0".to_string()];
        let openssl = pkg(
            "openssl",
            "1.1.1",
            "h456",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );

        let err = solve_to_actions(
            &["python".to_string()],
            &[python, openssl],
            &options(&["conda-forge"], false),
        )
        .expect_err("solver must report conflict");

        assert!(err.iter().any(|line| line.contains("unsatisfied")));
        assert!(err.iter().any(|line| line.contains("openssl")));
    }

    #[test]
    fn full_repodata_is_only_required_for_pinned_or_restrictive_specs() {
        assert!(!spec_requires_full_repodata("python>=3.11"));
        assert!(spec_requires_full_repodata("python<3.10"));
        assert!(spec_requires_full_repodata("numpy=1.26"));
    }
}

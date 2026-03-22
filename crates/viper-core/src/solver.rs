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
struct SpecConstraint {
    raw: String,
    parsed: Option<MatchSpec>,
    required_by: Option<String>,
}

#[derive(Debug, Clone)]
struct SearchState {
    selected: BTreeMap<String, usize>,
    constraints: BTreeMap<String, Vec<SpecConstraint>>,
    trace: Vec<String>,
}

pub fn solve_to_actions(
    specs: &[String],
    packages: &[RepoPackage],
    options: &SolveOptions,
) -> Result<SolveResult, Vec<String>> {
    let channel_priority = build_channel_priority_map(&options.channels);
    let mut state = SearchState {
        selected: BTreeMap::new(),
        constraints: BTreeMap::new(),
        trace: Vec::new(),
    };

    for spec in specs {
        let parsed = spec.parse::<MatchSpec>().ok();
        let name = requested_name(spec, parsed.as_ref());
        push_constraint(
            &mut state.constraints,
            name,
            SpecConstraint {
                raw: spec.clone(),
                parsed,
                required_by: None,
            },
        );
    }

    let solved = solve_recursive(packages, options, &channel_priority, state)?;
    let actions = solved
        .selected
        .values()
        .map(|idx| {
            let best = &packages[*idx];
            PlannedLink {
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
            }
        })
        .collect::<Vec<_>>();

    Ok(SolveResult {
        actions,
        trace: solved.trace,
    })
}

fn solve_recursive(
    packages: &[RepoPackage],
    options: &SolveOptions,
    channel_priority: &HashMap<String, usize>,
    state: SearchState,
) -> Result<SearchState, Vec<String>> {
    let Some(next_name) = state
        .constraints
        .keys()
        .find(|name| !state.selected.contains_key(*name))
        .cloned()
    else {
        return Ok(state);
    };

    let constraints = state
        .constraints
        .get(&next_name)
        .cloned()
        .unwrap_or_default();
    let candidates = ranked_candidates(
        &next_name,
        &constraints,
        packages,
        options,
        channel_priority,
    );

    if candidates.is_empty() {
        return Err(explain_unsatisfied(&next_name, &constraints));
    }

    let mut branch_errors = Vec::new();
    for idx in candidates {
        let candidate = &packages[idx];
        let mut next_state = state.clone();
        next_state.selected.insert(next_name.clone(), idx);
        next_state.trace.push(format!(
            "selected {}={} build={} channel={} for constraints [{}]",
            candidate.name,
            candidate.version,
            candidate.build,
            candidate.channel,
            constraints
                .iter()
                .map(|constraint| constraint.raw.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ));

        let mut immediate_conflicts = Vec::new();
        for dep in &candidate.depends {
            let dep_name = package_name_from_spec(dep).unwrap_or_else(|_| dep.clone());
            let dep_constraint = SpecConstraint {
                raw: dep.clone(),
                parsed: dep.parse::<MatchSpec>().ok(),
                required_by: Some(format!("{}={}", candidate.name, candidate.version)),
            };
            push_constraint(
                &mut next_state.constraints,
                dep_name.clone(),
                dep_constraint,
            );

            if let Some(selected_dep) = next_state.selected.get(&dep_name) {
                let selected_pkg = &packages[*selected_dep];
                let dep_constraints = next_state
                    .constraints
                    .get(&dep_name)
                    .expect("constraint entry must exist");
                if !matches_all_constraints(dep_constraints, selected_pkg) {
                    immediate_conflicts.push(format!(
                        "conflict: {}={} requires '{}' but selected {}={}",
                        candidate.name,
                        candidate.version,
                        dep,
                        selected_pkg.name,
                        selected_pkg.version
                    ));
                }
            }
        }

        if !immediate_conflicts.is_empty() {
            branch_errors.extend(immediate_conflicts);
            continue;
        }

        match solve_recursive(packages, options, channel_priority, next_state) {
            Ok(solved) => return Ok(solved),
            Err(errs) => branch_errors.extend(errs),
        }
    }

    if branch_errors.is_empty() {
        Err(explain_unsatisfied(&next_name, &constraints))
    } else {
        let mut seen = HashSet::new();
        let deduped = branch_errors
            .into_iter()
            .filter(|line| seen.insert(line.clone()))
            .collect::<Vec<_>>();
        Err(deduped)
    }
}

fn ranked_candidates(
    name: &str,
    constraints: &[SpecConstraint],
    packages: &[RepoPackage],
    options: &SolveOptions,
    channel_priority: &HashMap<String, usize>,
) -> Vec<usize> {
    let mut candidates = packages
        .iter()
        .enumerate()
        .filter(|(_, package)| package.name == name)
        .filter(|(_, package)| matches_all_constraints(constraints, package))
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return candidates;
    }

    if options.strict_channel_priority {
        let top_rank = candidates
            .iter()
            .map(|idx| channel_rank(channel_priority, packages[*idx].channel.as_str()))
            .min()
            .unwrap_or(usize::MAX);
        candidates.retain(|idx| {
            channel_rank(channel_priority, packages[*idx].channel.as_str()) == top_rank
        });
    }

    candidates.sort_by(|a, b| {
        compare_candidate_indexes(name, *b, *a, packages, options, channel_priority)
    });
    candidates
}

fn compare_candidate_indexes(
    name: &str,
    a_idx: usize,
    b_idx: usize,
    packages: &[RepoPackage],
    options: &SolveOptions,
    channel_priority: &HashMap<String, usize>,
) -> std::cmp::Ordering {
    let a = &packages[a_idx];
    let b = &packages[b_idx];
    let user_requested = options.user_requested.contains(name);

    if !user_requested
        && let Some((installed_version, installed_build)) = options.installed_preferred.get(name)
    {
        let a_installed = &a.version == installed_version && &a.build == installed_build;
        let b_installed = &b.version == installed_version && &b.build == installed_build;
        if a_installed != b_installed {
            return a_installed.cmp(&b_installed);
        }
    }

    compare_candidates(a, b).then_with(|| compare_channel_rank(a, b, channel_priority))
}

fn matches_all_constraints(constraints: &[SpecConstraint], candidate: &RepoPackage) -> bool {
    constraints
        .iter()
        .all(|constraint| candidate_matches_spec(constraint.parsed.as_ref(), candidate))
}

fn explain_unsatisfied(name: &str, constraints: &[SpecConstraint]) -> Vec<String> {
    let mut errors = Vec::new();
    for constraint in constraints {
        let requester = constraint
            .required_by
            .as_deref()
            .unwrap_or("user-requested spec");
        errors.push(format!(
            "unsatisfied: {requester} requires '{}' (package '{name}')",
            constraint.raw
        ));
    }
    if constraints.len() > 1 {
        let summary = constraints
            .iter()
            .map(|constraint| {
                let requester = constraint
                    .required_by
                    .as_deref()
                    .unwrap_or("user-requested spec");
                format!("{requester}: {}", constraint.raw)
            })
            .collect::<Vec<_>>()
            .join(" && ");
        errors.push(format!(
            "conflict: no candidate for package '{name}' satisfies all constraints ({summary})"
        ));
    }
    errors
}

fn push_constraint(
    constraints: &mut BTreeMap<String, Vec<SpecConstraint>>,
    name: String,
    new_constraint: SpecConstraint,
) {
    let entry = constraints.entry(name).or_default();
    let exists = entry.iter().any(|existing| {
        existing.raw == new_constraint.raw && existing.required_by == new_constraint.required_by
    });
    if !exists {
        entry.push(new_constraint);
    }
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
    fn backtracks_to_find_environment_level_solution() {
        let mut a_v2 = pkg(
            "a",
            "2.0.0",
            "0",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );
        a_v2.depends = vec!["b <2".to_string()];

        let mut a_v1 = pkg(
            "a",
            "1.0.0",
            "0",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );
        a_v1.depends = vec!["b >=2".to_string()];

        let mut c = pkg(
            "c",
            "1.0.0",
            "0",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );
        c.depends = vec!["b >=2".to_string()];

        let b_v1 = pkg(
            "b",
            "1.5.0",
            "0",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );
        let b_v2 = pkg(
            "b",
            "2.1.0",
            "0",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );

        let solved = solve_to_actions(
            &["a".to_string(), "c".to_string()],
            &[a_v2, a_v1, b_v1, b_v2, c],
            &options(&["conda-forge"], false),
        )
        .expect("solver must backtrack to environment-level solution");

        let a = solved
            .actions
            .iter()
            .find(|pkg| pkg.name == "a")
            .expect("a selected");
        let b = solved
            .actions
            .iter()
            .find(|pkg| pkg.name == "b")
            .expect("b selected");

        assert_eq!(a.version, "1.0.0");
        assert_eq!(b.version, "2.1.0");
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
    fn reports_conflicts_when_selected_dependency_violates_new_constraint() {
        let mut a_pkg = pkg(
            "a",
            "1.0.0",
            "0",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );
        a_pkg.depends = vec!["b >=2".to_string()];
        let b1 = pkg(
            "b",
            "1.0.0",
            "0",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );
        let b2 = pkg(
            "b",
            "2.0.0",
            "0",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );

        let err = solve_to_actions(
            &["a".to_string(), "b=1".to_string()],
            &[a_pkg, b1, b2],
            &options(&["conda-forge"], false),
        )
        .expect_err("solver must report contradictory constraints");
        assert!(err.iter().any(|line| line.contains("conflict")));
        assert!(err.iter().any(|line| line.contains("b >=2")));
    }

    #[test]
    fn full_repodata_is_only_required_for_pinned_or_restrictive_specs() {
        assert!(!spec_requires_full_repodata("python>=3.11"));
        assert!(spec_requires_full_repodata("python<3.10"));
        assert!(spec_requires_full_repodata("numpy=1.26"));
    }
}

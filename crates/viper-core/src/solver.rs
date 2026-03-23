use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use rattler_conda_types::{
    MatchSpec, Version,
    version_spec::{LogicalOperator, RangeOperator, VersionSpec},
};
use resolvo::utils::{Pool, VersionSet};
use resolvo::{
    Candidates, Dependencies, DependencyProvider, Interner, KnownDependencies, NameId, Problem,
    Requirement, SolvableId, Solver, SolverCache, StringId, VersionSetId, VersionSetUnionId,
};

use crate::repodata::RepoPackage;
use crate::spec::package_name_from_spec;
use crate::transaction::PlannedLink;

pub const PRODUCTION_SOLVER_ENGINE: &str = "resolvo";

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

pub fn production_solver_engine() -> &'static str {
    PRODUCTION_SOLVER_ENGINE
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CondaVersionSet {
    spec: String,
}

impl Display for CondaVersionSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.spec)
    }
}

impl VersionSet for CondaVersionSet {
    type V = usize;
}

struct CondaProvider {
    pool: Pool<CondaVersionSet>,
    packages: Vec<RepoPackage>,
    by_name: HashMap<String, Vec<usize>>,
    options: SolveOptions,
    channel_priority: HashMap<String, usize>,
}

impl CondaProvider {
    fn new(packages: &[RepoPackage], options: SolveOptions) -> Self {
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, pkg) in packages.iter().enumerate() {
            by_name.entry(pkg.name.clone()).or_default().push(idx);
        }
        Self {
            pool: Pool::new(),
            packages: packages.to_vec(),
            by_name,
            channel_priority: build_channel_priority_map(&options.channels),
            options,
        }
    }

    fn intern_requirement(&self, spec: &str) -> Result<Requirement, String> {
        let parsed = parse_constraint_spec(spec)?;
        let name = requested_name(spec, &parsed);
        let name_id = self.pool.intern_package_name(name);
        let version_set = self.pool.intern_version_set(
            name_id,
            CondaVersionSet {
                spec: spec.to_string(),
            },
        );
        Ok(version_set.into())
    }
}

impl Interner for CondaProvider {
    fn display_solvable(&self, solvable: SolvableId) -> impl Display + '_ {
        let idx = self.pool.resolve_solvable(solvable).record;
        let pkg = &self.packages[idx];
        format!("{}={}={}", pkg.name, pkg.version, pkg.build)
    }

    fn display_name(&self, name: NameId) -> impl Display + '_ {
        self.pool.resolve_package_name(name)
    }

    fn display_version_set(&self, version_set: VersionSetId) -> impl Display + '_ {
        self.pool.resolve_version_set(version_set)
    }

    fn display_string(&self, string_id: StringId) -> impl Display + '_ {
        self.pool.resolve_string(string_id)
    }

    fn version_set_name(&self, version_set: VersionSetId) -> NameId {
        self.pool.resolve_version_set_package_name(version_set)
    }

    fn solvable_name(&self, solvable: SolvableId) -> NameId {
        self.pool.resolve_solvable(solvable).name
    }

    fn version_sets_in_union(
        &self,
        version_set_union: VersionSetUnionId,
    ) -> impl Iterator<Item = VersionSetId> {
        self.pool.resolve_version_set_union(version_set_union)
    }
}

impl DependencyProvider for CondaProvider {
    async fn filter_candidates(
        &self,
        candidates: &[SolvableId],
        version_set: VersionSetId,
        inverse: bool,
    ) -> Vec<SolvableId> {
        let spec = self.pool.resolve_version_set(version_set);
        let Ok(parsed) = parse_constraint_spec(&spec.spec) else {
            return Vec::new();
        };
        candidates
            .iter()
            .copied()
            .filter(|id| {
                let idx = self.pool.resolve_solvable(*id).record;
                let matched = candidate_matches_spec(&parsed, &self.packages[idx]);
                matched != inverse
            })
            .collect()
    }

    async fn get_candidates(&self, name: NameId) -> Option<Candidates> {
        let name_str = self.pool.resolve_package_name(name);
        let indices = self.by_name.get(name_str)?;

        let filtered = if self.options.strict_channel_priority {
            let top_rank = indices
                .iter()
                .map(|idx| {
                    channel_rank(&self.channel_priority, self.packages[*idx].channel.as_str())
                })
                .min()
                .unwrap_or(usize::MAX);
            indices
                .iter()
                .copied()
                .filter(|idx| {
                    channel_rank(&self.channel_priority, self.packages[*idx].channel.as_str())
                        == top_rank
                })
                .collect::<Vec<_>>()
        } else {
            indices.clone()
        };

        let mut candidates = Vec::with_capacity(filtered.len());
        let mut favored = None;
        let user_requested = self.options.user_requested.contains(name_str);
        for idx in filtered {
            let solvable = self.pool.intern_solvable(name, idx);
            if !user_requested
                && let Some((installed_version, installed_build)) =
                    self.options.installed_preferred.get(name_str)
            {
                let pkg = &self.packages[idx];
                if &pkg.version == installed_version && &pkg.build == installed_build {
                    favored = Some(solvable);
                }
            }
            candidates.push(solvable);
        }

        if candidates.is_empty() {
            return None;
        }

        Some(Candidates {
            hint_dependencies_available: candidates.clone(),
            candidates,
            favored,
            ..Candidates::default()
        })
    }

    async fn sort_candidates(&self, _solver: &SolverCache<Self>, solvables: &mut [SolvableId]) {
        if solvables.is_empty() {
            return;
        }
        let name_id = self.pool.resolve_solvable(solvables[0]).name;
        let name = self.pool.resolve_package_name(name_id);
        solvables.sort_by(|a, b| {
            let a_idx = self.pool.resolve_solvable(*a).record;
            let b_idx = self.pool.resolve_solvable(*b).record;
            compare_candidate_indexes(
                name,
                b_idx,
                a_idx,
                &self.packages,
                &self.options,
                &self.channel_priority,
            )
        });
    }

    async fn get_dependencies(&self, solvable: SolvableId) -> Dependencies {
        let idx = self.pool.resolve_solvable(solvable).record;
        let pkg = &self.packages[idx];

        let mut requirements = Vec::new();
        for dep in &pkg.depends {
            match self.intern_requirement(dep) {
                Ok(req) => requirements.push(req),
                Err(err) => return Dependencies::Unknown(self.pool.intern_string(err)),
            }
        }

        let mut constrains = Vec::new();
        for con in &pkg.constrains {
            let Ok(parsed) = parse_constraint_spec(con) else {
                continue;
            };
            let name = requested_name(con, &parsed);
            let name_id = self.pool.intern_package_name(name);
            let version_set = self
                .pool
                .intern_version_set(name_id, CondaVersionSet { spec: con.clone() });
            constrains.push(version_set);
        }

        Dependencies::Known(KnownDependencies {
            requirements,
            constrains,
        })
    }
}

pub fn solve_to_actions(
    specs: &[String],
    packages: &[RepoPackage],
    options: &SolveOptions,
) -> Result<SolveResult, Vec<String>> {
    let provider = CondaProvider::new(packages, options.clone());
    let mut requirements = Vec::new();
    for spec in specs {
        requirements.push(provider.intern_requirement(spec).map_err(|err| vec![err])?);
    }

    let mut solver = Solver::new(provider);
    let problem = Problem::new().requirements(requirements);

    match solver.solve(problem) {
        Ok(solved) => {
            let actions = solved
                .into_iter()
                .map(|solvable| {
                    let idx = solver.provider().pool.resolve_solvable(solvable).record;
                    let best = &solver.provider().packages[idx];
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

            let trace = actions
                .iter()
                .map(|action| {
                    format!(
                        "selected {}={} build={} channel={}",
                        action.name, action.version, action.build, action.channel
                    )
                })
                .collect::<Vec<_>>();

            Ok(SolveResult { actions, trace })
        }
        Err(resolvo::UnsolvableOrCancelled::Unsolvable(conflict)) => {
            let mut errors = vec![format!(
                "conflict: {}",
                conflict.display_user_friendly(&solver)
            )];
            for spec in specs {
                errors.push(format!("unsatisfied: requested spec '{spec}'"));
            }
            let requested_names = specs
                .iter()
                .filter_map(|spec| package_name_from_spec(spec).ok())
                .collect::<HashSet<_>>();
            let mut dep_specs = HashSet::new();
            for pkg in packages {
                if requested_names.contains(&pkg.name) {
                    for dep in &pkg.depends {
                        dep_specs.insert(dep.clone());
                    }
                }
            }
            for dep in dep_specs {
                errors.push(format!("unsatisfied: dependency spec '{dep}'"));
            }
            Err(errors)
        }
        Err(resolvo::UnsolvableOrCancelled::Cancelled(_)) => {
            Err(vec!["solver cancelled".to_string()])
        }
    }
}

pub fn solve_with_production_solver(
    specs: &[String],
    packages: &[RepoPackage],
    options: &SolveOptions,
) -> Result<SolveResult, Vec<String>> {
    let _engine = production_solver_engine();
    solve_to_actions(specs, packages, options)
}

fn package_dist_name(pkg: &RepoPackage) -> String {
    format!("{}-{}-{}", pkg.name, pkg.version, pkg.build)
}

fn requested_name(spec: &str, parsed: &MatchSpec) -> String {
    if let Some(name) = parsed
        .name
        .as_exact()
        .map(|name| name.as_normalized().to_string())
    {
        return name;
    }
    package_name_from_spec(spec).unwrap_or_else(|_| spec.to_string())
}

fn parse_constraint_spec(spec: &str) -> Result<MatchSpec, String> {
    spec.parse::<MatchSpec>()
        .map_err(|err| format!("{spec}: {err}"))
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

fn candidate_matches_spec(spec: &MatchSpec, candidate: &RepoPackage) -> bool {
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
    fn user_requested_root_is_not_pinned_to_installed_build() {
        let pkgs = vec![
            pkg(
                "python",
                "3.11.0",
                "0",
                "https://conda.anaconda.org/conda-forge",
                "linux-64",
            ),
            pkg(
                "python",
                "3.12.0",
                "0",
                "https://conda.anaconda.org/conda-forge",
                "linux-64",
            ),
        ];
        let mut opts = options(&["conda-forge"], false);
        opts.installed_preferred.insert(
            "python".to_string(),
            ("3.11.0".to_string(), "0".to_string()),
        );
        opts.user_requested.insert("python".to_string());

        let result = solve_to_actions(&["python".to_string()], &pkgs, &opts)
            .expect("solver must allow requested upgrade");
        let python = result
            .actions
            .iter()
            .find(|action| action.name == "python")
            .expect("python action");
        assert_eq!(python.version, "3.12.0");
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
        assert!(spec_requires_full_repodata("python[build=\"py311_*\"]"));
        assert!(spec_requires_full_repodata("conda-forge::python>=3.11"));
        assert!(spec_requires_full_repodata("python[subdir=linux-64]"));
        assert!(spec_requires_full_repodata(
            "python[md5=deadbeefdeadbeefdeadbeefdeadbeef]"
        ));
        assert!(spec_requires_full_repodata(
            "python[sha256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef]"
        ));
    }

    #[test]
    fn production_solver_engine_is_fixed() {
        assert_eq!(production_solver_engine(), "resolvo");
    }

    #[test]
    fn production_solver_entry_matches_direct_solver_behavior() {
        let python = pkg(
            "python",
            "3.12.2",
            "0",
            "https://conda.anaconda.org/conda-forge",
            "linux-64",
        );
        let opts = options(&["conda-forge"], false);
        let specs = vec!["python>=3.11".to_string()];

        let direct =
            solve_to_actions(&specs, std::slice::from_ref(&python), &opts).expect("direct solve");
        let via_production =
            solve_with_production_solver(&specs, &[python], &opts).expect("production solve");
        let direct_actions = direct
            .actions
            .iter()
            .map(|a| (&a.name, &a.version, &a.build, &a.channel, &a.url))
            .collect::<Vec<_>>();
        let production_actions = via_production
            .actions
            .iter()
            .map(|a| (&a.name, &a.version, &a.build, &a.channel, &a.url))
            .collect::<Vec<_>>();
        assert_eq!(direct_actions, production_actions);
        assert_eq!(direct.trace, via_production.trace);
    }
}

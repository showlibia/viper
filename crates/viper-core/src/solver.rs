use rattler_conda_types::MatchSpec;

use crate::repodata::RepoPackage;
use crate::spec::package_name_from_spec;
use crate::transaction::PlannedLink;

pub fn solve_to_actions(specs: &[String], packages: &[RepoPackage]) -> Vec<PlannedLink> {
    let mut actions = Vec::new();
    for spec in specs {
        let name = package_name_from_spec(spec).unwrap_or_else(|_| spec.clone());
        let parsed = spec.parse::<MatchSpec>().ok();
        if let Some(best) = pick_best_candidate(&name, packages) {
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

fn pick_best_candidate<'a>(name: &str, packages: &'a [RepoPackage]) -> Option<&'a RepoPackage> {
    packages.iter().filter(|p| p.name == name).max_by(|a, b| {
        a.version
            .cmp(&b.version)
            .then_with(|| a.build.cmp(&b.build))
    })
}

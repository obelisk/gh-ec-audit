use std::collections::HashSet;

use colored::Colorize;

use crate::{make_paginated_github_request, Bootstrap, Repository};

/// Fetch all the repos for a given team and the permission it confers
pub fn run_team_repo_audit(bootstrap: Bootstrap, team: String) {
    let team_repos: HashSet<Repository> = match make_paginated_github_request(
        &bootstrap.token,
        25,
        &format!("/orgs/{}/teams/{}/repos", &bootstrap.org, team),
        3,
        None,
    ) {
        Ok(t) => t,
        Err(e) => {
            panic!(
                "{} {}: {e}",
                team.white(),
                "I couldn't fetch the team's repositories".red()
            );
        }
    };

    println!(
        "{} {} {}",
        "I found".green(),
        team_repos.len(),
        "repositories".green()
    );

    for repo in team_repos {
        println!("{}: {}", repo.name, repo.permissions.highest_perm());
    }
}

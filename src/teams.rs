use std::collections::HashSet;

use colored::Colorize;

use crate::{make_paginated_github_request, Bootstrap, Repository, Team};

/// Returns the repos that a team has access to
fn get_team_repos(bootstrap: &Bootstrap, team: String) -> HashSet<Repository> {
    let team_repos: HashSet<Repository> = match make_paginated_github_request(
        &bootstrap.token,
        25,
        &format!("/orgs/{}/teams/{}/repos", bootstrap.org, team),
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
    team_repos
}

/// Fetch all the repos for a given team and the permission it confers
pub fn run_team_repo_audit(bootstrap: Bootstrap, team: String) {
    let team_repos = get_team_repos(&bootstrap, team);

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

/// Get a list of all teams in the org
pub fn get_org_teams(bootstrap: &Bootstrap) -> HashSet<Team> {
    let teams: HashSet<Team> = match make_paginated_github_request(
        &bootstrap.token,
        25,
        &format!("/orgs/{}/teams", &bootstrap.org),
        3,
        None,
    ) {
        Ok(t) => t,
        Err(e) => {
            panic!(
                "{}: {e}",
                "I couldn't fetch the list of teams in the org".red()
            );
        }
    };

    teams
}

/// Fetch all empty teams, i.e., teams with no members
pub fn run_empty_teams_audit(bootstrap: Bootstrap) {
    println!(
        "{}",
        "I am going to fetch all teams from the org...".yellow()
    );
    let teams = get_org_teams(&bootstrap);

    println!(
        "{} {} {}",
        "Done: I found".green(),
        teams.len().to_string().white(),
        "teams".green()
    );
    println!("{}", "Now I will check for empty teams...".yellow());

    // For each team, see if it's empty.
    for team in teams {
        if team.is_empty(&bootstrap) {
            // The team is empty: we want to see to how many repos it has access
            let team_repos = get_team_repos(&bootstrap, team.slug);
            println!(
                "{}: {}. {} {} {}",
                "Found an empty GH team".yellow(),
                team.name.white(),
                "This team has access to".yellow(),
                team_repos.len().to_string().white(),
                "repositories".yellow()
            );
        }
    }
}

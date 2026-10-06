use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::{GitHubClient, Repository, Team};

/// Returns the repos that a team has access to
fn get_team_repos(client: &GitHubClient, team: String) -> HashSet<Repository> {
    let team_repos: HashSet<Repository> = match client.get_paginated(
        &format!("/orgs/{}/teams/{}/repos", client.org(), team),
        crate::constants::DEFAULT_PAGE_SIZE,
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
pub fn run_team_repo_audit(client: GitHubClient, team: String) {
    let team_repos = get_team_repos(&client, team);

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
pub fn get_org_teams(client: &GitHubClient) -> HashSet<Team> {
    match client.org_teams_set() {
        Ok(t) => t,
        Err(e) => {
            panic!(
                "{}: {e}",
                "I couldn't fetch the list of teams in the org".red()
            );
        }
    }
}

/// Get a list of all teams in the org, indexed by the team slug
pub fn get_indexed_org_teams(client: &GitHubClient) -> HashMap<String, Team> {
    match client.org_teams() {
        Ok(t) => t,
        Err(e) => {
            panic!(
                "{}: {e}",
                "I couldn't fetch the list of teams in the org".red()
            );
        }
    }
}

/// Fetch all empty teams, i.e., teams with no members
pub fn run_empty_teams_audit(client: GitHubClient) {
    println!(
        "{}",
        "I am going to fetch all teams from the org...".yellow()
    );
    let teams = get_org_teams(&client);

    println!(
        "{} {} {}",
        "Done: I found".green(),
        teams.len().to_string().white(),
        "teams".green()
    );
    println!("{}", "Now I will check for empty teams...".yellow());

    // For each team, see if it's empty.
    for team in teams {
        match team.is_empty(&client) {
            Ok(true) => {
                // The team is empty: we want to see to how many repos it has access
                let team_repos = get_team_repos(&client, team.slug);
                println!(
                    "{}: {}. {} {} {}",
                    "Found an empty GH team".yellow(),
                    team.name.white(),
                    "This team has access to".yellow(),
                    team_repos.len().to_string().white(),
                    "repositories".yellow()
                );
            }
            Ok(false) => {
                // The team is not empty: nothing to report
            }
            Err(e) => {
                println!(
                    "{} {} {} {}",
                    "Warning! I could not determine if team".yellow(),
                    team.slug.white(),
                    "is empty. I will continue with other teams. Error:".yellow(),
                    e.white()
                );
            }
        }
    }
}

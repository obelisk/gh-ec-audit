use std::collections::HashSet;

use colored::Colorize;

use crate::{make_github_request, make_paginated_github_request, Bootstrap, Repository, Team};

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

/// Fetch all empty teams, i.e., teams with no members
pub fn run_empty_teams_audit(bootstrap: Bootstrap) {
    println!(
        "{}",
        "I am going to fetch all teams from the org...".yellow()
    );
    // Get a list of all teams in the org
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
    println!(
        "{} {} {}",
        "Done: I found".green(),
        teams.len().to_string().white(),
        "teams".green()
    );
    println!("{}", "Now I will check for empty teams...".yellow());

    // For each team, get a list of its members and see if it's empty.
    // NOTE - We don't make a paginated request on purpose: we only want
    // to see if a team is empty or not. As soon as we have some members,
    // we want to move to the next team instead of fetching _all_ members.
    for team in teams {
        let members = match make_github_request(
            &bootstrap.token,
            &format!("/orgs/{}/teams/{}/members", bootstrap.org, team.slug),
            3,
            None,
        ) {
            Ok(members) => members,
            Err(e) => {
                panic!(
                    "{} {}: {}",
                    "I couldn't fetch the members of team".red(),
                    team.name,
                    e
                );
            }
        };
        let members = members.as_array().expect(&format!(
            "{}",
            "The value returned by GH is not an array".red()
        ));

        if members.is_empty() {
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

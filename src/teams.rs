use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::{
    make_paginated_github_request, make_paginated_github_request_with_index, members, Bootstrap,
    Member, Team,
};

pub fn run_maintainer_audit(bootstrap: Bootstrap, teams: Option<Vec<String>>) {
    println!("{}", "I'm going to fetch all teams from the org".yellow());

    // Fetch all teams
    let mut github_teams: HashMap<String, Team> = match make_paginated_github_request_with_index(
        &bootstrap.token,
        100,
        &format!("/orgs/{}/teams", &bootstrap.org),
        3,
        None,
    ) {
        Ok(teams) => teams,
        Err(e) => {
            panic!("{}: {e}", "I couldn't fetch the organization teams".red());
        }
    };

    // If teams are provided, filter the teams from GitHub
    // to only include the ones that are in the provided list
    if let Some(teams) = teams {
        github_teams = github_teams
            .into_iter()
            .filter(|(slug, _)| teams.contains(slug))
            .map(|(slug, team)| (slug.to_lowercase(), team))
            .collect();
    };

    println!(
        "{} {} {}",
        "I have".green(),
        github_teams.len().to_string().white(),
        "team(s)".green()
    );

    let organization_admins = bootstrap.fetch_organization_admins().unwrap();

    for (_, team) in github_teams {
        let members: HashSet<Member> = match make_paginated_github_request(
            &bootstrap.token,
            100,
            &format!("/orgs/{}/teams/{}/members", &bootstrap.org, team.slug),
            3,
            Some("role=maintainer"),
        ) {
            Ok(members) => members,
            Err(e) => {
                panic!("{}: {e}", "I couldn't fetch the team members".red());
            }
        };

        for member in members {
            if organization_admins.contains_key(&member.login) {
                continue;
            }

            println!(
                "{} {} {}",
                member.login.white(),
                "is a maintainer of".red(),
                team.slug.white(),
            );
        }
    }
}

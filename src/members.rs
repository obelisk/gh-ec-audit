use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::{
    get_repo_teams, make_paginated_github_request, make_paginated_github_request_with_index,
    Bootstrap, Collaborator, Member, Team,
};

pub fn get_org_members(bootstrap: &Bootstrap) -> HashSet<Member> {
    match make_paginated_github_request(
        &bootstrap.token,
        100,
        &format!("/orgs/{}/members", &bootstrap.org),
        3,
        None,
    ) {
        Ok(mem) => mem,
        Err(e) => {
            panic!("{}: {e}", "I couldn't fetch the organization members".red());
        }
    }
}

pub fn get_indexed_org_members(bootstrap: &Bootstrap) -> HashMap<String, Member> {
    match make_paginated_github_request_with_index(
        &bootstrap.token,
        100,
        &format!("/orgs/{}/members", &bootstrap.org),
        3,
        None,
    ) {
        Ok(mem) => mem,
        Err(e) => {
            panic!("{}: {e}", "I couldn't fetch the organization members".red());
        }
    }
}

pub fn get_org_admins(bootstrap: &Bootstrap) -> HashMap<String, Member> {
    let organization_admins: HashMap<String, Member> =
        match make_paginated_github_request_with_index(
            &bootstrap.token,
            100,
            &format!("/orgs/{}/members", &bootstrap.org),
            3,
            Some("role=admin"),
        ) {
            Ok(org_admins) => org_admins,
            Err(e) => {
                panic!("{}: {e}", "I couldn't fetch the organization members".red());
            }
        };
    organization_admins
}

pub fn run_audit(bootstrap: Bootstrap) {
    for member in get_org_members(&bootstrap) {
        println!("{}", member.avatar_url);
    }
}

pub fn run_admin_audit(bootstrap: Bootstrap, repos: Option<Vec<String>>) {
    let organization_admins = get_org_admins(&bootstrap);

    let repositories = repos.unwrap_or_else(|| {
        bootstrap
            .fetch_all_repositories(75)
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<String>>()
    });

    let mut team_cache: HashMap<String, HashMap<String, Member>> = HashMap::new();

    let one_percent = (repositories.len() as f64 * 0.01).ceil() as usize;
    let mut progress = 0;

    for repository in repositories {
        // Get the teams that have access to the repository
        let repo_teams = match get_repo_teams(&bootstrap, &repository) {
            Ok(rt) => rt,
            Err(_) => {
                println!(
                    "{} {} {}",
                    "I couldn't fetch teams with access to repository".yellow(),
                    repository.white(),
                    ". I will continue with other repositories.".yellow()
                );
                continue;
            }
        };

        let repo_admin_teams = repo_teams
            .iter()
            .filter(|t| t.permissions.as_ref().unwrap().admin)
            .collect::<Vec<&Team>>();

        for repo_admin_team in &repo_admin_teams {
            println!(
                "{} {} {} {}",
                "I found an admin team:".yellow(),
                repo_admin_team.slug.white(),
                "on".yellow(),
                repository.white()
            );
        }

        for team in &repo_teams {
            if !team_cache.contains_key(&team.slug) {
                let team_members: HashMap<String, Member> =
                    match team.fetch_team_members(&bootstrap) {
                        Ok(x) => x,
                        Err(e) => {
                            panic!(
                                "{} {}: {e}",
                                repository.white(),
                                "I couldn't fetch the repository collaborators".red()
                            )
                        }
                    };
                team_cache.insert(team.slug.clone(), team_members);
            }
        }

        let collaborators: HashSet<Collaborator> = match make_paginated_github_request(
            &bootstrap.token,
            25,
            &format!("/repos/{}/{}/collaborators", &bootstrap.org, repository),
            3,
            None,
        ) {
            Ok(collaborators) => collaborators,
            Err(e) => {
                panic!(
                    "{} {}: {e}",
                    repository.white(),
                    "I couldn't fetch the repository collaborators".red()
                );
            }
        };

        for collaborator in collaborators {
            // If this person is a repository admin and not an organization admin
            if collaborator.permissions.admin
                && !organization_admins.contains_key(&collaborator.login)
            {
                // Check to see if they are a member of a team that gives them admin access
                if repo_admin_teams.iter().fold(false, |acc, t| {
                    acc || team_cache[&t.slug].contains_key(&collaborator.login)
                }) {
                    continue;
                }

                println!(
                    "{} {} {} {}",
                    "I found an admin user:".yellow(),
                    collaborator.login.white(),
                    "on".yellow(),
                    repository.white()
                );
            }
        }
        progress += 1;
        if progress % one_percent == 0 {
            println!("Processed {} reposistories", progress.to_string().blue());
        }
    }
}

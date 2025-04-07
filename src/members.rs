use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::{
    make_paginated_github_request, make_paginated_github_request_with_index, Bootstrap,
    Collaborator, Member, Repository, Team,
};

pub fn run_audit(bootstrap: Bootstrap) {
    let members: HashSet<Member> = match make_paginated_github_request(
        &bootstrap.token,
        100,
        &format!("/orgs/{}/members", &bootstrap.org),
        3,
        None,
    ) {
        Ok(outside_collaborators) => outside_collaborators,
        Err(e) => {
            panic!("{}: {e}", "I couldn't fetch the organization members".red());
        }
    };

    for member in members {
        println!("{}", member.avatar_url);
    }
}

pub fn run_admin_audit(bootstrap: Bootstrap, repos: Option<Vec<String>>, public: bool) {
    let repos = repos.map(|r| {
        r.into_iter()
            .map(|x| x.to_lowercase())
            .collect::<HashSet<String>>()
    });

    let organization_admins = bootstrap.fetch_organization_admins().unwrap();

    let all_repositories: HashMap<_, _> = bootstrap
        .fetch_all_repositories(75)
        .unwrap()
        .into_iter()
        .filter(|r| if public { !r.1.private } else { true })
        .collect();

    let repositories: HashMap<String, Repository> = match &repos {
        Some(repos) => all_repositories
            .into_iter()
            .filter(|r| repos.contains(&r.1.name))
            .collect(),
        None => all_repositories,
    };

    match &repos {
        Some(repos) => {
            for repo in repos {
                if !repositories.contains_key(repo) {
                    println!(
                        "{} {} {}",
                        "I couldn't find".red(),
                        repo.white(),
                        "in the organization".red()
                    );
                }
            }
        }
        None => {}
    }

    println!(
        "{} {} {}",
        "I'm going to check".green(),
        repositories.len().to_string().white(),
        "repositories".green()
    );

    let mut team_cache: HashMap<String, HashMap<String, Member>> = HashMap::new();

    let one_percent = (repositories.len() as f64 * 0.01).ceil() as usize;
    let mut progress = 0;

    for repository in repositories {
        // Get the teams that have access to the repository
        let repo_teams: HashSet<Team> = match make_paginated_github_request(
            &bootstrap.token,
            25,
            &format!("/repos/{}/{}/teams", &bootstrap.org, repository.1.name),
            3,
            None,
        ) {
            Ok(t) => t,
            Err(e) => {
                panic!(
                    "{} {}: {e}",
                    repository.1.name.white(),
                    "I couldn't fetch the repository collaborators".red()
                );
            }
        };

        let repo_admin_teams = repo_teams
            .iter()
            .filter(|t| t.permissions.admin)
            .collect::<Vec<&Team>>();

        for repo_admin_team in &repo_admin_teams {
            println!(
                "{} {} {} {}",
                "I found an admin team:".yellow(),
                repo_admin_team.slug.white(),
                "on".yellow(),
                repository.1.name.white()
            );
        }

        for team in &repo_teams {
            if !team_cache.contains_key(&team.slug) {
                let team_members: HashMap<String, Member> =
                    match make_paginated_github_request_with_index(
                        &bootstrap.token,
                        25,
                        &format!("/orgs/{}/teams/{}/members", &bootstrap.org, team.slug),
                        3,
                        None,
                    ) {
                        Ok(t) => t,
                        Err(e) => {
                            panic!(
                                "{} {}: {e}",
                                repository.1.name.white(),
                                "I couldn't fetch the repository collaborators".red()
                            );
                        }
                    };
                team_cache.insert(team.slug.clone(), team_members);
            }
        }

        let collaborators: HashSet<Collaborator> = match make_paginated_github_request(
            &bootstrap.token,
            25,
            &format!(
                "/repos/{}/{}/collaborators",
                &bootstrap.org, repository.1.name
            ),
            3,
            None,
        ) {
            Ok(collaborators) => collaborators,
            Err(e) => {
                panic!(
                    "{} {}: {e}",
                    repository.1.name.white(),
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
                    repository.1.name.white()
                );
            }
        }
        progress += 1;
        if progress % one_percent == 0 {
            println!("Processed {} reposistories", progress.to_string().blue());
        }
    }
}

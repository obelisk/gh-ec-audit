use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::{make_paginated_github_request, Bootstrap, Collaborator, Permissions, Repository};

pub fn run_audit(bootstrap: Bootstrap) {
    #[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
    struct Member {
        avatar_url: String,
    }

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

pub fn run_admin_audit(bootstrap: Bootstrap, repos: Option<Vec<String>>) {
    #[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
    struct Member {
        login: String,
    }

    #[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
    struct Team {
        slug: String,
        permissions: Permissions,
    }

    let organization_admins: HashSet<Member> = match make_paginated_github_request(
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

    let repositories: HashSet<Repository> = match repos {
        Some(repos) => repos
            .into_iter()
            .map(|r| Repository {
                name: r,
                private: false,
            })
            .collect(),
        None => bootstrap.fetch_all_repositories(75).unwrap(),
    };

    let mut team_cache: HashMap<String, HashSet<Member>> = HashMap::new();

    let one_percent = (repositories.len() as f64 * 0.01).ceil() as usize;
    let mut progress = 0;

    for repository in repositories {
        // Get the teams that have access to the repository
        let repo_teams: HashSet<Team> = match make_paginated_github_request(
            &bootstrap.token,
            25,
            &format!("/repos/{}/{}/teams", &bootstrap.org, repository.name),
            3,
            None,
        ) {
            Ok(t) => t,
            Err(e) => {
                panic!(
                    "{} {}: {e}",
                    repository.name.white(),
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
                repository.name.white()
            );
        }

        for team in &repo_teams {
            if !team_cache.contains_key(&team.slug) {
                let team_members: HashSet<Member> = match make_paginated_github_request(
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
                            repository.name.white(),
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
                &bootstrap.org, repository.name
            ),
            3,
            None,
        ) {
            Ok(collaborators) => collaborators,
            Err(e) => {
                panic!(
                    "{} {}: {e}",
                    repository.name.white(),
                    "I couldn't fetch the repository collaborators".red()
                );
            }
        };

        for collaborator in collaborators {
            // If this person is a repository admin and not an organization admin
            if collaborator.permissions.admin
                && !organization_admins.contains(&Member {
                    login: collaborator.login.clone(),
                })
            {
                // Check to see if they are a member of a team that gives them admin access
                if repo_admin_teams.iter().fold(false, |acc, t| {
                    acc || team_cache[&t.slug].contains(&Member {
                        login: collaborator.login.clone(),
                    })
                }) {
                    continue;
                }

                println!(
                    "{} {} {} {}",
                    "I found an admin user:".yellow(),
                    collaborator.login.white(),
                    "on".yellow(),
                    repository.name.white()
                );
            }
        }
        progress += 1;
        if progress % one_percent == 0 {
            println!("Processed {} reposistories", progress.to_string().blue());
        }
    }
}

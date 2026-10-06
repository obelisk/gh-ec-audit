use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::{fetch_team_members, Collaborator, GitHubClient, Member, ProgressTracker, Team};

pub fn get_org_members(client: &GitHubClient) -> HashSet<Member> {
    match client.org_members(None) {
        Ok(mem) => mem.into_values().collect(),
        Err(e) => {
            panic!("{}: {e}", "I couldn't fetch the organization members".red());
        }
    }
}

pub fn get_indexed_org_members(client: &GitHubClient) -> HashMap<String, Member> {
    match client.org_members(None) {
        Ok(mem) => mem,
        Err(e) => {
            panic!("{}: {e}", "I couldn't fetch the organization members".red());
        }
    }
}

pub fn get_org_admins(client: &GitHubClient) -> HashMap<String, Member> {
    match client.org_members(Some("admin")) {
        Ok(org_admins) => org_admins,
        Err(e) => {
            panic!("{}: {e}", "I couldn't fetch the organization members".red());
        }
    }
}

pub fn run_audit(client: GitHubClient) {
    for member in get_org_members(&client) {
        println!("{}", member.avatar_url);
    }
}

pub fn run_admin_audit(client: GitHubClient, repos: Option<Vec<String>>) {
    let organization_admins = get_org_admins(&client);

    let repositories = client.resolve_repos(repos);

    let mut team_cache: HashMap<String, HashMap<String, Member>> = HashMap::new();

    let mut tracker = ProgressTracker::new(repositories.len());

    for repository in repositories {
        // Get the teams that have access to the repository
        let repo_teams = match client.repo_teams(&repository) {
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
                    match fetch_team_members(&client, &team.slug) {
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

        let collaborators: HashSet<Collaborator> = match client.repo_collaborators(&repository) {
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
            if collaborator.permissions().admin
                && !organization_admins.contains_key(collaborator.login())
            {
                // Check to see if they are a member of a team that gives them admin access
                if repo_admin_teams
                    .iter()
                    .any(|t| team_cache[&t.slug].contains_key(collaborator.login()))
                {
                    continue;
                }

                println!(
                    "{} {} {} {}",
                    "I found an admin user:".yellow(),
                    collaborator.login().white(),
                    "on".yellow(),
                    repository.white()
                );
            }
        }
        tracker.tick();
    }
}

mod csv;

use std::collections::{HashMap, HashSet};

use colored::Colorize;
use csv::{repo_audit_to_csv, team_access_to_csv};

use crate::{
    codeowners::{iterate::get_co_file, CodeownersFile},
    get_repo_collaborators, get_repo_teams, Bootstrap, Collaborator, Permissions, Team,
};

/// Run the User Access Review on the given repos (mandatory argument).
///
/// For each repo, fetch the CODEOWNERS file, if one is present, and collect
/// a team-to-repos mapping. If no CODEOWNERS file is found, perform a
/// traditional access review by looking at which users/team have access
/// to the repo and with which permissions.
///
/// Finally, dump all members of all teams we have encountered.
pub fn run_uar_audit(bootstrap: Bootstrap, repos: Vec<String>, csv: bool) {
    println!("{}", "Performing the UAR on the given repos...".yellow());
    let (users_to_repos, teams_to_repos) = repos_uar(&bootstrap, &repos, csv);
    teams_uar(&bootstrap, &teams_to_repos, csv);

    if !csv {
        // Print the mappings for a complete picture
        // Users
        println!("{}", "USERS' ACCESS".green());
        for (user, repos_with_perms) in &users_to_repos {
            println!("{} {}", "Username:".green(), user.white());
            for (repo, perms) in repos_with_perms {
                println!(
                    "\t{} {}, {} {}",
                    "Repository".green(),
                    repo.white(),
                    "Permissions".green(),
                    perms.highest_perm().white()
                );
            }
        }
    }
}

/// Gather information about who has access to the repos in scope for the UAR
fn repos_uar(
    bootstrap: &Bootstrap,
    repos: &[String],
    csv: bool,
) -> (
    HashMap<String, HashSet<(String, Permissions)>>,
    HashMap<String, HashSet<(String, Permissions)>>,
) {
    // Mappings to store which repos users and teams have access to
    // { user/team --> (repo, permissions) }
    let mut users_to_repos: HashMap<String, HashSet<(String, Permissions)>> = HashMap::new();
    let mut teams_to_repos: HashMap<String, HashSet<(String, Permissions)>> = HashMap::new();

    for repo in repos {
        // Find all the users and teams that have access to this repo.
        // If we have a CODEOWNERS file, we focus on that one, otherwise
        // we fall back to looking at the repo's permissions.
        let ((users, teams), using_codeowners) =
            if let Ok(Some(co_file)) = get_co_file(&bootstrap, &repo) {
                // We have a CODEOWNERS file: parse it and collect all users and teams
                // mentioned in the file, then perform a UAR on those users / teams.
                println!(
                    "{} {}{}",
                    "CODEOWNERS file found for repo".green(),
                    repo.white(),
                    ": using that for the UAR".green()
                );
                (co_uar(&bootstrap, &repo, co_file), true)
            } else {
                // No CODEOWNERS file found: proceed with traditional UAR
                println!(
                    "{} {}{}",
                    "No CODEOWNERS file found for repo".yellow(),
                    repo.white(),
                    ": proceeding with traditional UAR".yellow()
                );
                (traditional_uar(&bootstrap, &repo), false)
            };

        // Add everything we found to the mappings
        for u in &users {
            users_to_repos
                .entry(u.login.to_string())
                .or_insert_with(HashSet::new)
                .insert((repo.clone(), u.permissions.clone()));
        }
        for t in &teams {
            teams_to_repos
                .entry(t.slug.clone())
                .or_insert_with(HashSet::new)
                .insert((repo.clone(), t.permissions.clone().unwrap()));
        }

        if csv {
            // Create a CSV file for this repo. The CSV goes into a specific folder,
            // depending on whether we are using a CODEOWNERS file or not for the audit.
            let (folder, format) = if using_codeowners {
                ("codeowners", csv::CsvFormat::CodeOwners)
            } else {
                ("traditional", csv::CsvFormat::Traditional)
            };
            repo_audit_to_csv(
                &bootstrap,
                format!("output/{folder}/{repo}.csv"),
                &users,
                &teams,
                format,
            );
        } else {
            // Print out the access to this repo
            let repo_users: Vec<String> = users.iter().map(|c| c.login.clone()).collect();
            let repo_teams: Vec<String> = teams.iter().map(|t| t.slug.clone()).collect();
            println!(
                "{} {:?} {} {:?}",
                "Users:".green(),
                repo_users,
                "\nTeams:".green(),
                repo_teams
            );
        }
    }

    (users_to_repos, teams_to_repos)
}

/// Gather information about teams and team membership
fn teams_uar(
    bootstrap: &Bootstrap,
    teams_to_repos: &HashMap<String, HashSet<(String, Permissions)>>,
    csv: bool,
) {
    if csv {
        team_access_to_csv(format!("output/teams_access.csv"), &teams_to_repos);
        csv::team_members_to_csv(
            &bootstrap,
            format!("output/teams_membership.csv"),
            &teams_to_repos,
        );
    } else {
        // Teams
        println!("\n{}", "TEAMS' ACCESS".green());
        for (team, repos_with_perms) in teams_to_repos {
            println!("{} {}", "Team name:".green(), team.white());
            for (repo, perms) in repos_with_perms {
                println!(
                    "\t{} {}, {} {}",
                    "Repository".green(),
                    repo.white(),
                    "Permissions".green(),
                    perms.highest_perm().white()
                );
            }
        }

        // All members of all teams we encountered
        println!("\n{}", "TEAMS' MEMBERS".green());
        for (team, _) in teams_to_repos {
            // A temporary team object just to be able to call the fetch_members method
            let tmp_team = Team {
                slug: team.to_string(),
                name: team.to_string(),
                permissions: None,
            };
            let members: Vec<String> = tmp_team
                .fetch_team_members(&bootstrap)
                .unwrap()
                .keys()
                .map(|v| v.to_string())
                .collect();
            println!("{}:{:?}", team.white(), members);
        }
    }
}

/// Extract in-scope collaborators and teams from the given CODEOWNERS file.
fn co_uar(
    bootstrap: &Bootstrap,
    repo: &str,
    co_file: CodeownersFile,
) -> (HashSet<Collaborator>, HashSet<Team>) {
    // Get all users and teams that have access to this repo.
    // Then we will filter and keep only those that appear in the CO file.
    let users = get_repo_collaborators(bootstrap, repo);
    let teams = get_repo_teams(bootstrap, repo);

    let filtered_users = users
        .unwrap() // TODO fix
        .into_iter()
        .filter(|u| co_file.users.contains(&u.login))
        .collect();
    let filtered_teams = teams
        .unwrap() // TODO fix
        .into_iter()
        .filter(|t| co_file.teams.contains(&t.slug))
        .collect();

    (filtered_users, filtered_teams)
}

/// Extract in-scope collaborators and teams by looking at who has access to the repo.
fn traditional_uar(bootstrap: &Bootstrap, repo: &str) -> (HashSet<Collaborator>, HashSet<Team>) {
    let users = get_repo_collaborators(bootstrap, repo);
    let teams = get_repo_teams(bootstrap, repo);
    (users.unwrap(), teams.unwrap()) // TODO fix
}

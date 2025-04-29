mod csv;

use std::collections::{HashMap, HashSet};

use colored::Colorize;

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
pub fn run_uar_audit(bootstrap: Bootstrap, repos: Vec<String>, csv: Option<String>) {
    println!("{}", "Performing the UAR on the given repos...".yellow());

    // Mappings to store which repos users and teams have access to
    let mut users_to_repos: HashMap<String, HashSet<(String, Permissions)>> = HashMap::new();
    let mut teams_to_repos: HashMap<String, HashSet<(String, Permissions)>> = HashMap::new();

    let mut repo_access: HashMap<String, (HashSet<Collaborator>, HashSet<Team>)> = HashMap::new();

    for repo in repos {
        // Find all the users and teams that have access to this repo.
        // If we have a CODEOWNERS file, we focus on that one, otherwise
        // we fall back to looking at the repo's permissions.
        let (users, teams) = if let Ok(Some(co_file)) = get_co_file(&bootstrap, &repo) {
            // We have a CODEOWNERS file: parse it and collect all users and teams
            // mentioned in the file, then perform a UAR on those users / teams.
            println!(
                "{} {}{}",
                "CODEOWNERS file found for repo".green(),
                repo.white(),
                ": using that for the UAR".green()
            );
            co_uar(&bootstrap, &repo, co_file)
        } else {
            // No CODEOWNERS file found: proceed with traditional UAR
            println!(
                "{} {}{}",
                "No CODEOWNERS file found for repo".yellow(),
                repo.white(),
                ": proceeding with traditional UAR".yellow()
            );
            traditional_uar(&bootstrap, &repo)
        };

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

        // Store the access to this repo: this is useful if we want to export everything in CSV format
        repo_access.insert(repo, (users, teams));
    }

    // Print the mappings for a complete picture

    // Users
    println!("{}", "USERS' ACCESS".green());
    for (user, repos_with_perms) in users_to_repos {
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

    // Teams
    println!("\n{}", "TEAMS' ACCESS".green());
    for (team, repos_with_perms) in &teams_to_repos {
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
    for (team, _) in &teams_to_repos {
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

    // If we were asked to export the result to a CSV file, we do it here
    if let Some(csv_file) = csv {
        csv::export_to_csv(&csv_file, &repo_access);
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

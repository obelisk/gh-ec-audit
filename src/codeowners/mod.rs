/// Use an iterative approach, i.e., scan repos one by one
mod iterate;
/// Leverage the GH search API to find relevant information
mod search;

use std::collections::HashSet;

use crate::{members::get_org_members, teams::get_org_teams, Bootstrap};
use colored::Colorize;
use regex::Regex;

/// Represents a CODEOWNERS file
#[derive(Debug)]
struct CodeownersFile {
    /// Repository this CO file belongs to
    repo: String,
    /// HTML URL, to give the user a quick way to reach the file
    url: String,
    /// List of all users mentioned in the file, for further analysis
    users: HashSet<String>,
    /// List of all teams mentioned in the file, for further analysis
    teams: HashSet<String>,
}

/// Run the audit on CODEOWNERS files
pub fn run_codeowners_audit(
    bootstrap: Bootstrap,
    team: Option<String>,
    repos: Option<Vec<String>>,
    search: bool,
) {
    if search && repos.is_some() {
        panic!("{}", "Using --search assumes an org-wide search, and it is not supported in conjunction with a list of repos (i.e., --repos).".red());
    }

    if team.is_none() {
        // If we don't receive a team, then we audit all CODEOWNERS files in the org.
        // This means we will look at the CODEOWNERS files and determine
        // * If all users mentioned in the file exist and are members of the org
        // * If all teams mentioned in the file exist
        // We will also alert if a team is empty.
        println!(
            "{}",
            "Searching for CODEOWNERS files across the org...".yellow()
        );

        let codeowners_files = match search {
            true => search::find_codeowners_in_org(&bootstrap),
            false => iterate::find_codeowners_in_org(&bootstrap, repos),
        };

        println!(
            "{} {} {}",
            "Done! I found".green(),
            codeowners_files.len().to_string().white(),
            "CODEOWNERS files".green()
        );

        println!(
            "{}",
            "Preparing to analyze these CODEOWNERS files...".yellow()
        );

        // Get all members and teams in the org, so that we can efficiently
        // cross-check the content of all the CODEOWNERS files we have found.
        let org_members = get_org_members(&bootstrap);
        let org_teams = get_org_teams(&bootstrap);

        // Keep a growing cache of empty and non-empty teams to make checks more efficient.
        let mut non_empty_teams = HashSet::<String>::new();
        let mut empty_teams = HashSet::<String>::new();

        // Analyze each CO file we found
        for co_file in codeowners_files {
            // Fetch all users and teams that have access to this repo.
            // We will use this info to check if users/teams mentioned in the CO file have sufficient access.
            /*let repo_collabs = get_repo_collaborators(&bootstrap, &co_file.repo);
            let repo_teams = get_repo_teams(&bootstrap, &co_file.repo);*/

            // Check if all the users mentioned in the CO file are in the org, have access to the repo, and have sufficient permissions
            for user in co_file.users {
                if org_members.iter().find(|u| u.login == user).is_none() {
                    println!(
                        "{} {} {} {} {}",
                        "Error in CODEOWNERS file".red(),
                        co_file.url.white(),
                        "User".red(),
                        user.white(),
                        "does not belong to the org".red()
                    );
                }
                /*match repo_collabs.iter().find(|rc| rc.login == user) {
                    None => {
                        println!(
                            "{} {} {} {} {}",
                            "Error in CODEOWNERS file".red(),
                            co_file.url.white(),
                            "User".red(),
                            user.white(),
                            "does not have access to the repo".red()
                        );
                    }
                    Some(rc) => {
                        if !rc.permissions.write_or_higher() {
                            println!(
                                "{} {} {} {} {}",
                                "Error in CODEOWNERS file".red(),
                                co_file.url.white(),
                                "User".red(),
                                user.white(),
                                "does not have sufficient permissions on repo".red()
                            );
                        }
                    }
                }*/
            }

            // Check if all the teams mentioned in the CO file exist and alert if a team is empty.
            // Also check if teams have access to the repo and have sufficient permissions.
            for team in co_file.teams {
                match org_teams.iter().find(|t| t.slug == team) {
                    None => {
                        println!(
                            "{} {} {} {} {}",
                            "Error in CODEOWNERS file".red(),
                            co_file.url.white(),
                            "Team".red(),
                            team.white(),
                            "does not exist in the org".red()
                        );
                    }
                    Some(t) => {
                        // Check if the team is empty, starting from looking into the cache: if
                        // it's not there, let's call GH API and update the cache accordingly.
                        // Note - the || operator short-circuits, so we are making the call to GH API
                        // only if the team is not in our cache.
                        if empty_teams.contains(&t.slug) || t.is_empty(&bootstrap) {
                            println!(
                                "{} {} {} {}",
                                "Warning! CODEOWNERS file".red(),
                                co_file.url.white(),
                                "contains an empty team:".red(),
                                t.slug.white(),
                            );
                            empty_teams.insert(t.slug.clone());
                        } else {
                            // The team is not empty. Let's insert it into the non-empty cache (repeated
                            // insertions don't matter because it's a HashSet).
                            non_empty_teams.insert(t.slug.clone());

                            /*match repo_teams.iter().find(|rt| rt.slug == t.slug) {
                                None => {
                                    println!(
                                        "{} {} {} {} {}",
                                        "Error in CODEOWNERS file".red(),
                                        co_file.url.white(),
                                        "Team".red(),
                                        t.slug.white(),
                                        "does not have access to the repo".red()
                                    );
                                }
                                Some(rt) => {
                                    if !rt.permissions.write_or_higher() {
                                        println!(
                                            "{} {} {} {} {}",
                                            "Error in CODEOWNERS file".red(),
                                            co_file.url.white(),
                                            "Team".red(),
                                            t.slug.white(),
                                            "does not have sufficient permissions on repo".red()
                                        );
                                    }
                                }
                            }*/
                        }
                    }
                }
            }
        }
    } else {
        // If we do receive a team, then we will look for all occurrences of that team in CODEOWNERS files across the org.
        // This is useful to estimate the impact on CODEOWNERS that removing or renaming a team would have.
        let team = team.unwrap();
        println!(
            "{} {} {}",
            "Searching for occurrences of team".yellow(),
            team.white(),
            "in CODEOWNERS files...".yellow()
        );
        match search {
            true => search::find_team_in_codeowners(&bootstrap, team),
            false => iterate::find_team_in_codeowners(&bootstrap, team, repos),
        }
    }
}

/// Process the content of a CO file and turn it into a CodeownersFile struct
fn codeowner_content_to_obj(
    bootstrap: &Bootstrap,
    content: String,
    html_url: &str,
    repo: &str,
) -> CodeownersFile {
    // Get users and teams mentioned in this CODEOWNERS file
    // Find matches that look like @something, and then decide if it's a user or a team
    let regex = Regex::new(r"@\S+").unwrap();

    let mut users = HashSet::new();
    let mut teams = HashSet::new();

    // The prefix that teams have in CO files
    let team_prefix = format!("@{}/", bootstrap.org);

    for line in content.split('\n') {
        if line.starts_with('#') {
            // Skip comments
            continue;
        }

        for m in regex.find_iter(line) {
            let matched = m.as_str();
            if matched.starts_with(&team_prefix) {
                // It's a team
                teams.insert(matched.trim_start_matches(&team_prefix).to_string());
            } else {
                // It's a user
                users.insert(matched.trim_start_matches("@").to_string());
            }
        }
    }

    CodeownersFile {
        repo: repo.to_string(),
        url: html_url.to_string(),
        users,
        teams,
    }
}

use std::collections::HashSet;

use crate::{make_github_request, Bootstrap, Member, Team};

use super::CodeownersFile;
use colored::Colorize;

/// Run the audit on a given list of CO files
pub fn audit_co_files(
    bootstrap: &Bootstrap,
    codeowners_files: &[CodeownersFile],
    org_members: &HashSet<Member>,
    org_teams: &HashSet<Team>,
) {
    // Keep a growing cache of empty and non-empty teams to make checks more efficient.
    let mut non_empty_teams = HashSet::<String>::new();
    let mut empty_teams = HashSet::<String>::new();

    for co_file in codeowners_files {
        let mut no_errors = true;

        // Check if all the users mentioned in the CO file are in the org
        for user in &co_file.users {
            if org_members.iter().find(|u| u.login == *user).is_none() {
                no_errors = false;
                println!(
                    "{} {} {} {} {}",
                    "Error in CODEOWNERS file".red(),
                    co_file.url.white(),
                    "User".red(),
                    user.white(),
                    "does not belong to the org".red()
                );
            }
        }

        // Check if all the teams mentioned in the CO file exist and alert if a team is empty.
        for team in &co_file.teams {
            match org_teams.iter().find(|t| t.slug == *team) {
                None => {
                    no_errors = false;
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
                    // Check if the team is empty, by first looking into the cache: if
                    // it's not there, we call GH API and update the cache accordingly.
                    // Note - the || operator short-circuits, so we are making the call to GH API
                    // only if the team is not in our cache.
                    if empty_teams.contains(&t.slug) || t.is_empty(&bootstrap) {
                        no_errors = false;
                        println!(
                            "{} {} {} {}",
                            "Warning! CODEOWNERS file".yellow(),
                            co_file.url.white(),
                            "contains an empty team:".yellow(),
                            t.slug.white(),
                        );
                        empty_teams.insert(t.slug.clone());
                    } else {
                        // The team is not empty. Let's insert it into the non-empty cache (repeated
                        // insertions don't matter because it's a HashSet).
                        non_empty_teams.insert(t.slug.clone());
                    }
                }
            }
        }

        if no_errors {
            println!(
                "{} {}",
                "No errors detected in CODEOWNERS file for repo".green(),
                co_file.repo.white()
            );
        }
    }
}

/// Audit CODEOWNERS files for errors by asking the GH REST API.  
/// For more info, see https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#list-codeowners-errors
pub fn audit_co_files_with_gh_api(bootstrap: &Bootstrap, repos: &[String]) {
    for repo in repos {
        // Call the GH API
        match get_codeowners_errors(&bootstrap, &repo) {
            Ok(kinds) => {
                if kinds.is_empty() {
                    println!(
                        "{} {}",
                        "No errors detected in CODEOWNERS file for repo".green(),
                        repo.white()
                    );
                } else {
                    println!(
                        "{} {}: {:?}",
                        "Errors detected in CODEOWNERS file for repo".red(),
                        repo.white(),
                        kinds
                    );
                }
            }
            Err(e) => {
                println!(
                    "{} {} {} {}",
                    "Warning!".yellow(),
                    e.white(),
                    "for repo".yellow(),
                    repo.white()
                );
            }
        }
    }
}

/// Call the GH API and retrieve errors detected in the CODEOWNERS file.
fn get_codeowners_errors(bootstrap: &Bootstrap, repo: &str) -> Result<Vec<String>, String> {
    let url = format!("/repos/{}/{repo}/codeowners/errors", bootstrap.org);
    let res = make_github_request(&bootstrap.token, &url, 3, None).unwrap();
    match res.get("errors") {
        None => {
            // If this field is not present, it means a CO file has not been found
            return Err("CODEOWNERS file not found".to_string());
        }
        Some(errors) => {
            let kinds: Vec<String> = errors
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e.get("kind").unwrap().as_str().unwrap().to_string())
                .collect();
            Ok(kinds)
        }
    }
}

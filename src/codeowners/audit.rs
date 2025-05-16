use std::collections::{HashMap, HashSet};

use crate::{make_github_request, Bootstrap, Member, Team};

use super::{CodeownersFile, CodeownersFileProblem};
use colored::Colorize;

/// Run the audit on a given list of CO files
pub fn audit_co_files(
    bootstrap: &Bootstrap,
    codeowners_files: &[CodeownersFile],
    org_members: &HashMap<String, Member>,
    org_teams: &HashMap<String, Team>,
    also_gh_api: bool,
    verbose: bool,
) {
    // Keep a growing cache of empty and non-empty teams to make checks more efficient.
    let mut non_empty_teams = HashSet::new();
    let mut empty_teams = HashSet::new();

    for co_file in codeowners_files {
        // Check if all the users mentioned in the CO file are in the org
        let mut co_problems = co_file
            .users
            .iter()
            .filter_map(|user| {
                if !org_members.contains_key(user) {
                    Some(CodeownersFileProblem::UserNotInOrg {
                        user: user.to_string(),
                        co_file_url: co_file.url.clone(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<CodeownersFileProblem>>();

        // Check if all the teams mentioned in the CO file exist and alert if a team is empty.
        co_problems.extend(
            co_file
                .teams
                .iter()
                .filter_map(|team| {
                    if !org_teams.contains_key(team) {
                        Some(CodeownersFileProblem::TeamNotInOrg {
                            team: team.to_string(),
                            co_file_url: co_file.url.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect::<Vec<CodeownersFileProblem>>(),
        );

        // See if we have any warnings
        co_problems.extend(
            co_file
                .teams
                .iter()
                .filter_map(|team| {
                    // Check if the team is empty, by first looking into the cache: if
                    // it's not there, we call the GH API and update the cache accordingly.
                    // Note - the || operator short-circuits, so we are making the call to GH API
                    // only if the team is not in our cache.
                    match org_teams.get(team) {
                        Some(t) => {
                            if empty_teams.contains(&t.slug) || {
                                match t.is_empty(bootstrap) {
                                    Ok(empty) => empty,
                                    Err(e) => {
                                        // For some reason, we could not establish if the team is empty. Log a warning and return false (i.e., not empty),
                                        // which is equivalent to ignoring it (because we are not alerting on non-empty teams).
                                        // This should anyway be a very rare circumstance.
                                        println!(
                                            "{} {} {} {}",
                                            "Warning! I could not determine if team".yellow(),
                                            t.slug.white(),
                                            "is empty. I am going to ignore it. The error was"
                                                .yellow(),
                                            e.white()
                                        );
                                        false
                                    }
                                }
                            } {
                                // The team is empty
                                empty_teams.insert(t.slug.clone());
                                Some(CodeownersFileProblem::EmptyTeam {
                                    team: team.to_string(),
                                    co_file_url: co_file.url.clone(),
                                })
                            } else {
                                // The team is not empty. Let's insert it into the non-empty cache (repeated
                                // insertions don't matter because it's a HashSet).
                                non_empty_teams.insert(t.slug.clone());
                                None
                            }
                        }
                        None => {
                            // This means the team does not exist in the org. This is actually an error
                            // which will have been caught above. So we can safely ignore it here.
                            None
                        }
                    }
                })
                .collect::<Vec<CodeownersFileProblem>>(),
        );

        // Print all the errors and warnings, if any
        if co_problems.is_empty() {
            // By default, we print nothing if there was nothing wrong. But we can print if a verbose flag was passed.
            if verbose {
                println!(
                    "{} {}",
                    "No problems detected in CODEOWNERS file for repo".green(),
                    co_file.repo.white()
                );
            }
        } else {
            for problem in co_problems {
                println!("{problem}");
            }
        }

        // If we were told to also use the GH API, we do it here
        if also_gh_api {
            println!("This is what the GitHub API has to say about this CODEOWNERS file...");
            audit_co_files_with_gh_api(bootstrap, &co_file.repo);
        }
    }
}

/// Audit CODEOWNERS files for errors by asking the GH REST API.  
/// For more info, see https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#list-codeowners-errors
fn audit_co_files_with_gh_api(bootstrap: &Bootstrap, repo: &str) {
    // Call the GH API
    match get_codeowners_errors(&bootstrap, &repo) {
        Ok(Some(kinds)) => {
            // A CODEOWNERS file was found and a (possibly empty) Vec of errors was returned
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
        Ok(None) => {
            // A CODEOWNERS file was not found
            println!(
                "{} {}",
                "CODEOWNERS file not found for repository".yellow(),
                repo.white()
            );
        }
        Err(e) => {
            // The call to GH failed
            println!(
                "{} {} {} {}",
                "Warning! Call to GitHub API failed with error".yellow(),
                e.white(),
                "for repo".yellow(),
                repo.white()
            );
        }
    }
}

/// Call the GH API and retrieve errors detected in the CODEOWNERS file.
fn get_codeowners_errors(bootstrap: &Bootstrap, repo: &str) -> Result<Option<Vec<String>>, String> {
    let url = format!("/repos/{}/{repo}/codeowners/errors", bootstrap.org);
    let res = make_github_request(&bootstrap.token, &url, 3, None)?;
    match res.get("errors") {
        None => {
            // If this field is not present, it means a CO file has not been found
            return Ok(None);
        }
        Some(errors) => {
            let kinds: Vec<String> = errors
                .as_array()
                .ok_or("Unexpected format received: it's not an array".to_string())?
                .iter()
                .map(|e| {
                    e.get("kind")
                        .and_then(|k| k.as_str())
                        .and_then(|s| Some(s.to_string()))
                        .unwrap_or("Unknown".to_string())
                })
                .collect();
            Ok(Some(kinds))
        }
    }
}

mod audit;
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

/// Run the audit on CODEOWNERS files to determine
/// * If all users mentioned in the file exist and are members of the org
/// * If all teams mentioned in the file exist
///
/// We will also alert if a team is empty.
pub fn run_codeowners_audit(
    bootstrap: Bootstrap,
    repos: Option<Vec<String>>,
    search: bool,
    also_gh_api: bool,
) {
    // Immediately stop if we received incompatible options
    if search && repos.is_some() {
        panic!("{}", "Using --search assumes an org-wide search, and it is not supported in conjunction with a list of repos (i.e., --repos).".red());
    }

    println!("{}", "Searching for CODEOWNERS files...".yellow());

    // Build a list of CO files we will audit
    let codeowners_files = match search {
        true => search::find_codeowners_in_org(&bootstrap),
        false => iterate::find_codeowners_in_org(&bootstrap, repos.clone()),
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

    audit::audit_co_files(&bootstrap, &codeowners_files, &org_members, &org_teams);

    // If we were told to also use the GH API, we do it here
    if also_gh_api {
        println!(
            "{}",
            "\nNow auditing CODEOWNERS using the GH REST API...".yellow()
        );
        // The repos we will scan are those that were passed or (if nothing was passed)
        // all those that we collected CO files for in the previous steps.
        let repos = repos.unwrap_or_else(|| {
            codeowners_files
                .iter()
                .map(|file| file.repo.clone())
                .collect()
        });

        audit::audit_co_files_with_gh_api(&bootstrap, &repos);
    }
}

/// Look for all occurrences of that team in CODEOWNERS files across the org.
/// This is useful to estimate the impact on CODEOWNERS that removing or renaming a team would have.
pub fn run_team_in_codeowners_audit(bootstrap: Bootstrap, team: String, search: bool) {
    println!(
        "{} {} {}",
        "Searching for occurrences of team".yellow(),
        team.white(),
        "in CODEOWNERS files...".yellow()
    );
    if search {
        search::find_team_in_codeowners(&bootstrap, team)
    } else {
        iterate::find_team_in_codeowners(&bootstrap, team, None)
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
        if line.trim().starts_with('#') {
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

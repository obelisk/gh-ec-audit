use colored::Colorize;

use crate::constants::CO_LOCATIONS;
use crate::{utils::process_fetch_file_result, GitHubClient};

use super::CodeownersFile;

/// Search for a CO file in the possible locations and download the file, returning its content and HTML URL. Stop as soon as a matching file is found.  
/// From GH docs: "If CODEOWNERS files exist in more than one of those locations, GitHub will search for them in that order and use the first one it finds.""
pub fn get_co_file(client: &GitHubClient, repo: &str) -> Result<Option<CodeownersFile>, String> {
    for location in CO_LOCATIONS {
        // Try to download the file and fill in `content` and `html_url`
        let url = format!("/repos/{}/{}/contents/{}", client.org(), repo, location);
        let res = client.get_json(&url, None);
        match res {
            Err(_) => continue, // The call for this location failed, keep going
            Ok(v) => {
                // If we have a status, check it to see if GH is telling us the file does not exist
                if let Some(s) = v.get("status") {
                    match s.as_str() {
                        Some(status) => {
                            if status == "404" {
                                // This means the file was not found
                                continue;
                            }
                        }
                        None => {
                            // This is very strange
                            continue;
                        }
                    }
                }
                let html_url = v
                    .get("html_url")
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or("Not available".to_string());
                if let Ok(content) = process_fetch_file_result(v) {
                    return Ok(Some(CodeownersFile::parse_from_content(
                        client.org(),
                        content,
                        &html_url,
                        repo,
                    )));
                } else {
                    // We return instead of continuing the for-loop because a file was found:
                    // we just did not manage to get its content, for some reason.
                    return Err("Could not read the content of CODEOWNERS file".to_string());
                }
            }
        }
    }
    // If we are here, then no CO file was found
    Ok(None)
}

/// Find all the codeowners files in an organization
pub fn find_codeowners_in_org(
    client: &GitHubClient,
    repos: Option<Vec<String>>,
) -> Result<Vec<CodeownersFile>, String> {
    let repos = match repos {
        Some(v) => v,
        None => client.repo_names(false)?,
    };

    let mut all_results = vec![];

    for repo in repos {
        if let Ok(Some(co_file)) = get_co_file(client, &repo) {
            all_results.push(co_file);
        } else {
            // We did not manage to fill the content: this means we did not find a CODEOWNERS for this repo
            println!(
                "{} {}",
                "Warning! CODEOWNERS file not found for repository".red(),
                repo.white()
            );
            continue;
        }
    }

    Ok(all_results)
}

/// Find all occurrences of a given team in an organization's codeowners files
pub fn find_team_in_codeowners(client: &GitHubClient, team: String, repos: Option<Vec<String>>) {
    if let Ok(code_owners) = find_codeowners_in_org(client, repos) {
        for co in code_owners {
            if co.teams.contains(&team) {
                println!(
                    "{} {:<50} - {} {}",
                    "Repository:".yellow(),
                    co.repo.white(),
                    "URL:".yellow(),
                    co.url.white()
                )
            }
        }
    }
}

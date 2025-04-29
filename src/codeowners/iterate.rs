use colored::Colorize;

use crate::{make_github_request, utils::process_fetch_file_result, Bootstrap};

use super::{codeowner_content_to_obj, CodeownersFile};

/// Locations where a CODEOWNERS file can be placed, sorted by priority.
/// For more info, see https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners#codeowners-file-location
const CO_LOCATIONS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];

/// Search for a CO file in the possible locations and download the file, returning its content and HTML URL. Stop as soon as a matching file is found.  
/// From GH docs: "If CODEOWNERS files exist in more than one of those locations, GitHub will search for them in that order and use the first one it finds.""
fn get_co_file(bootstrap: &Bootstrap, repo: &str) -> Option<(String, String)> {
    for location in CO_LOCATIONS {
        // Try to download the file and fill in `content` and `html_url`
        let url = format!("/repos/{}/{}/contents/{}", bootstrap.org, repo, location);
        let res = make_github_request(&bootstrap.token, &url, 3, None);
        match res {
            Err(_) => continue, // This call failed, keep going
            Ok(v) => {
                match v.get("status") {
                    None => {}
                    Some(s) => {
                        if s.as_str().unwrap() == "404" {
                            // This means the file was not found
                            continue;
                        }
                    }
                }
                let html_url = v.get("html_url").unwrap().as_str().unwrap().to_string();
                let content = process_fetch_file_result(v);
                return Some((content, html_url));
            }
        }
    }
    // If we are here, then no CO file was found
    None
}

/// Find all the codeowners files in an organization
pub fn find_codeowners_in_org(
    bootstrap: &Bootstrap,
    repos: Option<Vec<String>>,
) -> Vec<CodeownersFile> {
    let repos = repos.unwrap_or_else(|| {
        bootstrap
            .fetch_all_repositories(75)
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<String>>()
    });

    let mut all_results = vec![];

    for repo in repos {
        if let Some((content, html_url)) = get_co_file(bootstrap, &repo) {
            all_results.push(codeowner_content_to_obj(
                bootstrap, content, &html_url, &repo,
            ));
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

    all_results
}

/// Find all occurrences of a given team in an organization's codeowners files
pub fn find_team_in_codeowners(bootstrap: &Bootstrap, team: String, repos: Option<Vec<String>>) {
    let code_owners = find_codeowners_in_org(bootstrap, repos);
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

use colored::Colorize;

use crate::{make_github_request, Bootstrap};

use super::CodeownersFile;

/// Find all the codeowners files in an organization
pub fn find_codeowners_in_org(bootstrap: &Bootstrap) -> Result<Vec<CodeownersFile>, String> {
    let query = format!("org:{} filename:CODEOWNERS", bootstrap.org);
    let query = urlencoding::encode(&query).to_string();

    let mut page = 1;
    let mut all_results = vec![];

    loop {
        // !!! NOTE - This endpoint has a custom rate limitation !!!
        // https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#rate-limit
        let address = format!("/search/code?q={query}&per_page=100&page={page}");
        let res = make_github_request(&bootstrap.token, &address, 3, None)?;
        let items = res
            .get("items")
            .and_then(|i| i.as_array())
            .and_then(|a| Some(a.clone()))
            .unwrap_or_default();
        if items.is_empty() {
            // We are past the last page
            break;
        }
        for item in items {
            // Filter out all files that are not called exactly CODEOWNERS
            let name = item
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("Not available");
            if name != "CODEOWNERS" {
                continue;
            }

            let repo = item
                .get("repository")
                .and_then(|r| r.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("Not available");
            let html_url = item
                .get("html_url")
                .and_then(|u| u.as_str())
                .unwrap_or("Not available");
            let api_url = item
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("Not available");
            if let Ok(content) = crate::utils::fetch_file_content(&bootstrap, api_url) {
                all_results.push(CodeownersFile::parse_from_content(
                    bootstrap, content, html_url, repo,
                ));
            }
        }
        page += 1;
    }

    Ok(all_results)
}

/// Find all occurrences of a given team in an organization's codeowners files
pub fn find_team_in_codeowners(bootstrap: &Bootstrap, team: String) {
    let query = format!(
        "org:{} filename:CODEOWNERS @{}/{}",
        bootstrap.org, bootstrap.org, team
    );
    let query = urlencoding::encode(&query).to_string();

    let mut page = 1;

    loop {
        // !!! NOTE - This endpoint has a custom rate limitation !!!
        // https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#rate-limit
        let address = format!("/search/code?q={query}&per_page=100&page={page}");
        let res = make_github_request(&bootstrap.token, &address, 3, None).unwrap();
        let items = res
            .get("items")
            .and_then(|i| i.as_array())
            .and_then(|a| Some(a.clone()))
            .unwrap_or_default();
        if items.is_empty() {
            break;
        }
        for item in items {
            // Filter out all files that are not called exactly CODEOWNERS
            let name = item
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("Not available");
            if name != "CODEOWNERS" {
                continue;
            }

            let repo = item
                .get("repository")
                .and_then(|r| r.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("Not available");
            let html_url = item
                .get("html_url")
                .and_then(|u| u.as_str())
                .unwrap_or("Not available");

            println!(
                "{} {:<50} - {} {}",
                "Repository:".yellow(),
                repo.white(),
                "URL:".yellow(),
                html_url.white()
            )
        }
        page += 1;
    }
}

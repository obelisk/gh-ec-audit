use colored::Colorize;

use crate::GitHubClient;

use super::CodeownersFile;

/// Run a GitHub code search for the given query, returning the raw items.
/// Both search functions below share this pagination logic.
fn search_code(client: &GitHubClient, query: &str) -> Result<Vec<serde_json::Value>, String> {
    let query = urlencoding::encode(query).to_string();
    // !!! NOTE - This endpoint has a custom rate limitation !!!
    // https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#rate-limit
    client.get_paginated_values(
        &format!("/search/code?q={query}"),
        "items",
        crate::constants::MEMBERS_PAGE_SIZE,
        None,
    )
}

/// Find all the codeowners files in an organization
pub fn find_codeowners_in_org(client: &GitHubClient) -> Result<Vec<CodeownersFile>, String> {
    let query = format!("org:{} filename:CODEOWNERS", client.org());
    let items = search_code(client, &query)?;

    let mut all_results = vec![];
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
        if let Ok(content) = crate::utils::fetch_file_content(client, api_url) {
            all_results.push(CodeownersFile::parse_from_content(
                client.org(),
                content,
                html_url,
                repo,
            ));
        }
    }

    Ok(all_results)
}

/// Find all occurrences of a given team in an organization's codeowners files
pub fn find_team_in_codeowners(client: &GitHubClient, team: String) {
    let query = format!(
        "org:{} filename:CODEOWNERS @{}/{}",
        client.org(),
        client.org(),
        team
    );
    let items = match search_code(client, &query) {
        Ok(items) => items,
        Err(e) => {
            println!("{}: {e}", "Search failed".red());
            return;
        }
    };

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
}

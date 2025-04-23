use colored::Colorize;

use crate::{make_github_request, Bootstrap};

use super::{codeowner_content_to_obj, CodeownersFile};

/// Find all the codeowners files in an organization
pub fn find_codeowners_in_org(bootstrap: &Bootstrap) -> Vec<CodeownersFile> {
    let query = format!("org:{} filename:CODEOWNERS", bootstrap.org);
    let query = urlencoding::encode(&query).to_string();

    let mut page = 1;
    let mut all_results = vec![];

    loop {
        // !!! NOTE - This endpoint has a custom rate limitation !!!
        // https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#rate-limit
        let address = format!("/search/code?q={query}&per_page=100&page={page}");
        let res = make_github_request(&bootstrap.token, &address, 3, None).unwrap();
        let items = res.get("items").unwrap().as_array().unwrap();
        if items.is_empty() {
            // We are past the last page
            break;
        }
        for item in items {
            // Filter out all files that are not called exactly CODEOWNERS
            let name = item.get("name").unwrap().as_str().unwrap();
            if name != "CODEOWNERS" {
                continue;
            }

            let repo = item
                .get("repository")
                .unwrap()
                .get("name")
                .unwrap()
                .as_str()
                .unwrap();
            let html_url = item.get("html_url").unwrap().as_str().unwrap();
            let api_url = item.get("url").unwrap().as_str().unwrap();
            let content = crate::utils::fetch_file_content(&bootstrap, api_url);

            all_results.push(codeowner_content_to_obj(bootstrap, content, html_url, repo));
        }
        page += 1;
    }

    all_results
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
        let items = res.get("items").unwrap().as_array().unwrap();
        if items.is_empty() {
            break;
        }
        for item in items {
            // Filter out all files that are not called exactly CODEOWNERS
            let name = item.get("name").unwrap().as_str().unwrap();
            if name != "CODEOWNERS" {
                continue;
            }

            let repo = item
                .get("repository")
                .unwrap()
                .get("name")
                .unwrap()
                .as_str()
                .unwrap();
            let html_url = item.get("html_url").unwrap().as_str().unwrap();

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

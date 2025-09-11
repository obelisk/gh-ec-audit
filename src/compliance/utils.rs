use std::collections::HashSet;

use crate::{
    compliance::{Check, CodeownersStatus, Errors},
    make_github_request, Bootstrap,
};

// Try the common CODEOWNERS locations and return the repository-relative path if found
pub fn find_codeowners_path(bootstrap: &Bootstrap, repo: &str) -> Option<String> {
    const CO_LOCATIONS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];
    for location in CO_LOCATIONS {
        let url = format!("/repos/{}/{}/contents/{}", bootstrap.org, repo, location);
        match make_github_request(&bootstrap.token, &url, 2, None) {
            Ok(v) => {
                if v.get("status").and_then(|s| s.as_str()) == Some("404") {
                    continue;
                }
                // If GitHub returned an object for this path, it exists
                if v.get("path").and_then(|p| p.as_str()).is_some() {
                    return Some(location.to_string());
                }
            }
            Err(_) => continue,
        }
    }
    None
}

pub fn read_existing_csv_repos(path: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut rdr = match csv::Reader::from_path(path) {
        Ok(r) => r,
        Err(_) => return set,
    };
    let headers = match rdr.headers() {
        Ok(h) => h.clone(),
        Err(_) => return set,
    };
    let repo_idx = headers.iter().position(|h| h == "repository").unwrap_or(0);
    for result in rdr.records() {
        if let Ok(record) = result {
            if let Some(repo) = record.get(repo_idx) {
                set.insert(repo.to_string());
            }
        }
    }
    set
}

/// Check CODEOWNERS state via GitHub API
pub fn codeowners_exists_and_is_valid(
    bootstrap: &Bootstrap,
    repo: &str,
) -> Result<CodeownersStatus, Errors> {
    let url = format!("/repos/{}/{repo}/codeowners/errors", bootstrap.org);
    match make_github_request(&bootstrap.token, &url, 3, None) {
        Ok(res) => {
            if res.get("status").and_then(|v| v.as_str()) == Some("403") {
                return Err(Errors::NoAccess403);
            }
            match res.get("errors") {
                None => Ok(CodeownersStatus::Missing),
                Some(errors) => match errors.as_array() {
                    Some(arr) if arr.is_empty() => Ok(CodeownersStatus::Valid),
                    Some(_) => Ok(CodeownersStatus::Invalid),
                    None => Err(Errors::MissingOrError),
                },
            }
        }
        Err(_) => Err(Errors::MissingOrError),
    }
}

pub fn check_symbol(v: Check) -> String {
    match v {
        None => "? (403)".to_string(),
        Some(true) => "✅".to_string(),
        _ => "❌".to_string(),
    }
}

pub fn check_csv_value(v: Check) -> String {
    match v {
        None => "403".to_string(),
        Some(true) => "pass".to_string(),
        _ => "fail".to_string(),
    }
}

pub fn check_csv_value_named(v: Check, name: &str, selected: Option<&HashSet<String>>) -> String {
    match selected {
        None => check_csv_value(v),
        Some(s) => {
            if !s.contains(&name.to_lowercase()) {
                return "n/a".to_string();
            }
            check_csv_value(v)
        }
    }
}

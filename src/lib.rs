use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    thread::sleep,
    time::Duration,
};

use colored::Colorize;

pub mod bpr;
pub mod codeowners;
pub mod compliance;
pub mod deploy_key;
pub mod external_collaborator;
pub mod members;
pub mod teams;
pub mod utils;

pub trait GitHubIndex {
    fn index(&self) -> String;
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
pub struct Permissions {
    pull: bool,
    triage: bool,
    push: bool,
    maintain: bool,
    admin: bool,
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
pub struct Collaborator {
    login: String,
    permissions: Permissions,
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
pub struct Member {
    pub avatar_url: String,
    pub login: String,
}

impl GitHubIndex for Member {
    fn index(&self) -> String {
        self.login.clone()
    }
}

impl Permissions {
    fn highest_perm(&self) -> String {
        if self.admin {
            return "admin".to_string();
        }
        if self.maintain {
            return "maintain".to_string();
        }
        if self.push {
            return "push".to_string();
        }
        if self.triage {
            return "triage".to_string();
        }
        if self.pull {
            return "pull".to_string();
        }
        "none".to_string()
    }
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
struct GitHubError {
    pub message: String,
    pub documentation_url: String,
    pub status: String,
}

impl Display for GitHubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GitHubError: {} {} {}",
            self.message, self.documentation_url, self.status
        )
    }
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
pub struct Repository {
    pub name: String,
    pub private: bool,
    pub archived: bool,
    pub disabled: bool,
    pub permissions: Permissions,
}

#[derive(serde::Deserialize, Hash, Eq, PartialEq)]
pub struct Team {
    pub name: String,
    pub slug: String,
    pub permissions: Option<Permissions>,
}

impl GitHubIndex for Team {
    fn index(&self) -> String {
        self.slug.clone()
    }
}

impl Team {
    /// Return whether a team is empty, i.e., if the team has no members,
    /// including its sub-teams.
    fn is_empty(&self, bootstrap: &Bootstrap) -> Result<bool, String> {
        // NOTE - We don't make a paginated request on purpose: we only want
        // to see if a team is empty or not, and we don't need to fetch _all_ members.
        let members = make_github_request(
            &bootstrap.token,
            &format!("/orgs/{}/teams/{}/members", bootstrap.org, self.slug),
            3,
            None,
        )?;

        match members.as_array() {
            Some(v) => Ok(v.is_empty()),
            None => Err("The value returned by GitHub is not an array".to_string()),
        }
    }
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
#[serde(untagged)]
enum GitHubResponse<T> {
    Data(Vec<T>),
    Error(GitHubError),
}

fn make_github_request(
    gh_token: &str,
    url: &str,
    retries: u8,
    params: Option<&str>,
) -> Result<serde_json::Value, String> {
    let params = match params {
        Some(params) => format!("?{params}"),
        None => String::new(),
    };

    let mut tries = 0;
    loop {
        tries += 1;
        let response = reqwest::blocking::Client::new()
            .get(&format!("https://api.github.com{url}{params}",))
            .header("User-Agent", "GitHub EC Audit")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Authorization", format!("Bearer {}", gh_token))
            .send()
            .map(|response| response.text());

        // Handle communication issues with GitHub
        let content = match response {
            Ok(Ok(content)) => content,
            Ok(Err(e)) => {
                if tries >= retries {
                    println!("{}", "Retries exhausted".red());
                    return Err(e.to_string());
                }

                println!(
                    "{}: {}",
                    "Going to retry because couldn't read response from GitHub:".yellow(),
                    e.to_string().red()
                );

                continue;
            }
            Err(e) => {
                if tries >= retries {
                    println!("{}", "Retries exhausted".red());
                    return Err(e.to_string());
                }

                println!(
                    "{}: {}",
                    "Going to retry because couldn't make request to GitHub:".yellow(),
                    e.to_string().red()
                );

                continue;
            }
        };

        let value = serde_json::from_str::<serde_json::Value>(&content)
            .map_err(|e| format!("Could not deserialize GitHub's response. Error: {e}"))?;

        // break the loop and return the value we got
        return Ok(value);
    }
}

fn make_paginated_github_request<T>(
    gh_token: &str,
    page_size: u8,
    url: &str,
    retries: u8,
    params: Option<&str>,
) -> Result<HashSet<T>, String>
where
    T: serde::de::DeserializeOwned + std::hash::Hash + std::cmp::Eq,
{
    let params = match params {
        Some(params) => format!("&{params}"),
        None => String::new(),
    };

    let mut page = 1;
    let mut all_items = HashSet::new();
    let mut tries = 0;
    loop {
        tries += 1;
        let response = reqwest::blocking::Client::new()
            .get(&format!(
                "https://api.github.com{url}?per_page={page_size}&page={page}{params}",
            ))
            .header("User-Agent", "GitHub EC Audit")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Authorization", format!("Bearer {}", gh_token))
            .send()
            .map(|response| response.text());

        // Handle communication issues with GitHub
        let content = match response {
            Ok(Ok(content)) => content,
            Ok(Err(e)) => {
                if tries >= retries {
                    println!("{}", "Retries exhausted".red());
                    return Err(e.to_string());
                }

                println!(
                    "{}: {}",
                    "Going to retry because couldn't read response from GitHub:".yellow(),
                    e.to_string().red()
                );

                continue;
            }
            Err(e) => {
                if tries >= retries {
                    println!("{}", "Retries exhausted".red());
                    return Err(e.to_string());
                }

                println!(
                    "{}: {}",
                    "Going to retry because couldn't make request to GitHub:".yellow(),
                    e.to_string().red()
                );

                continue;
            }
        };

        // Handle GitHub errors
        match serde_json::from_str::<GitHubResponse<T>>(content.as_str()) {
            Ok(GitHubResponse::Data(data)) => {
                // When we go past the end (an unneeded page), we'll get an empty response so we can break
                if data.is_empty() {
                    break;
                }

                // The page is full so we need to add all these users to our set and grab the next page
                page += 1;
                tries = 0;
                all_items.extend(data);
            }
            Ok(GitHubResponse::Error(e)) => {
                // GitHub threw an error and if it's a ratelimit we can wait and retry
                if e.message.contains("API rate limit exceeded") {
                    sleep(Duration::from_secs(60));
                } else {
                    // This doesn't look like the expected data or a ratelimit error

                    // We're out of retries so we need to stop
                    if tries >= retries {
                        println!("{}", "Retries exhausted".red());
                        return Err(e.to_string());
                    }

                    // We have retries remaining so we'll try again
                    println!(
                        "{}: {}",
                        "Going to retry because couldn't deserialize response from GitHub:"
                            .yellow(),
                        e.to_string().red()
                    );
                    tries += 1;
                    println!("{}", content.yellow());
                }
            }
            Err(e) => {
                // This doesn't look like the expected data or an error
                if tries >= retries {
                    println!("{}", "Retries exhausted".red());
                    return Err(e.to_string());
                }

                println!(
                    "{}: {}",
                    "Going to retry because couldn't deserialize response from GitHub:".yellow(),
                    e.to_string().red()
                );

                println!("{}", content.yellow());
            }
        }
    }

    Ok(all_items)
}

fn make_paginated_github_request_with_index<T>(
    gh_token: &str,
    page_size: u8,
    url: &str,
    retries: u8,
    params: Option<&str>,
) -> Result<HashMap<String, T>, String>
where
    T: serde::de::DeserializeOwned + std::hash::Hash + std::cmp::Eq + GitHubIndex,
{
    let results: HashSet<T> =
        make_paginated_github_request(gh_token, page_size, url, retries, params)?;

    Ok(results
        .into_iter()
        .map(|item| (item.index(), item))
        .collect::<HashMap<String, T>>())
}

pub struct Bootstrap {
    token: String,
    org: String,
}

impl Bootstrap {
    pub fn new() -> Result<Self, String> {
        println!(
            "{}",
            "I'm checking there is a GitHub FPAT in the GH_TOKEN environment variable...".yellow()
        );

        let token = match std::env::var("GH_TOKEN") {
            Ok(token) => token,
            Err(_) => {
                return Err("GH_TOKEN not found".to_string());
            }
        };
        println!("{} {}...", "I have token:".green(), &token[..20]);

        let org = match std::env::var("GH_ORG") {
            Ok(org) => org,
            Err(_) => {
                return Err("GH_ORG not found".to_string());
            }
        };
        println!("{} {}", "I have organization:".green(), org.white());

        Ok(Self { token, org })
    }

    pub fn fetch_all_repositories(
        &self,
        page_size: u8,
        active_only: bool,
    ) -> Result<HashSet<Repository>, String> {
        if active_only {
            println!(
                "{}",
                "I'm going to fetch all active repositories from the org".yellow()
            );
        } else {
            println!(
                "{}",
                "I'm going to fetch all repositories from the org".yellow()
            );
        }

        let repositories: HashSet<Repository> = match make_paginated_github_request(
            &self.token,
            page_size,
            &format!("/orgs/{}/repos", &self.org),
            3,
            None,
        ) {
            Ok(repositories) => repositories,
            Err(e) => {
                return Err(format!(
                    "{}: {}",
                    "I couldn't fetch the repositories".red(),
                    e
                ));
            }
        };

        println!("{} {}", "Success! I found: ".green(), repositories.len());
        if !repositories
            .iter()
            .fold(false, |acc, repo| acc || repo.private)
        {
            println!("{}", "I didn't find any private repositories. Make sure you have permission to read private repositories.".red());
        }

        Ok(repositories)
    }
}

/// Get collaborators for a given repository
fn get_repo_collaborators(
    bootstrap: &Bootstrap,
    repo: &str,
) -> Result<HashSet<Collaborator>, String> {
    make_paginated_github_request(
        &bootstrap.token,
        25,
        &format!("/repos/{}/{}/collaborators", &bootstrap.org, repo),
        3,
        None,
    )
}

/// Get the teams that have access to the repo
fn get_repo_teams(bootstrap: &Bootstrap, repo: &str) -> Result<HashSet<Team>, String> {
    make_paginated_github_request(
        &bootstrap.token,
        25,
        &format!("/repos/{}/{}/teams", &bootstrap.org, repo),
        3,
        None,
    )
}

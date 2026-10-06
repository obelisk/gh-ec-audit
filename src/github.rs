//! The GitHub HTTP layer.
//!
//! [`GitHubClient`] owns the HTTP client, the token and the organization, and
//! exposes both low-level request helpers and a handful of endpoint helpers.
//! Centralizing the request boilerplate (headers, pagination, retries) removes
//! the duplication that used to be spread across every audit module.

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    thread::sleep,
    time::Duration,
};

use colored::Colorize;
use serde::de::DeserializeOwned;

use crate::constants::{
    API_BASE, API_VERSION, DEFAULT_PAGE_SIZE, DEFAULT_RETRIES, GRAPHQL_URL, MEMBERS_PAGE_SIZE,
    RATE_LIMIT_SLEEP_SECS, REPOS_PAGE_SIZE, USER_AGENT,
};
use crate::{Collaborator, GitHubIndex, Member, Repository, Team};

/// An error returned by the GitHub API
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

/// A GitHub response that is either a list of items or an error object
#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
#[serde(untagged)]
enum GitHubResponse<T> {
    Data(Vec<T>),
    Error(GitHubError),
}

/// A GraphQL query with its variables
#[derive(serde::Serialize)]
pub struct GraphQLQuery {
    query: String,
    variables: HashMap<String, String>,
}

/// Client for the GitHub REST and GraphQL APIs.
///
/// It owns the HTTP client, the token and the organization, so callers no
/// longer need to thread those three values through every function.
pub struct GitHubClient {
    client: reqwest::blocking::Client,
    token: String,
    org: String,
}

impl GitHubClient {
    /// Build a client from the `GH_TOKEN` and `GH_ORG` environment variables.
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
        println!("{}", "I have a token".green());

        let org = match std::env::var("GH_ORG") {
            Ok(org) => org,
            Err(_) => {
                return Err("GH_ORG not found".to_string());
            }
        };
        println!("{} {}", "I have organization:".green(), org.white());

        // A timeout avoids hanging forever on a stalled connection.
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("Could not build the HTTP client: {e}"))?;

        Ok(Self { client, token, org })
    }

    /// The organization this client is scoped to
    pub fn org(&self) -> &str {
        &self.org
    }

    /// Build a request with the common GitHub headers already set
    fn request(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client
            .get(url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header("Authorization", format!("Bearer {}", self.token))
    }

    /// Send a GET request, retrying on transient transport errors, and return
    /// the raw response body.
    fn get_raw(&self, full_url: &str) -> Result<String, String> {
        let mut tries = 0;
        loop {
            tries += 1;
            match self.request(full_url).send().and_then(|r| r.text()) {
                Ok(content) => return Ok(content),
                Err(e) => {
                    if tries >= DEFAULT_RETRIES {
                        println!("{}", "Retries exhausted".red());
                        return Err(e.to_string());
                    }
                    println!(
                        "{}: {}",
                        "Going to retry because of a GitHub request error:".yellow(),
                        e.to_string().red()
                    );
                }
            }
        }
    }

    /// Build a URL with an optional query string
    fn build_url(&self, url: &str, params: Option<&str>) -> String {
        match params {
            Some(params) => format!("{API_BASE}{url}?{params}"),
            None => format!("{API_BASE}{url}"),
        }
    }

    /// Build a paginated URL with an optional query string
    fn build_paginated_url(
        &self,
        url: &str,
        page_size: u8,
        page: u32,
        params: Option<&str>,
    ) -> String {
        let params = match params {
            Some(params) => format!("&{params}"),
            None => String::new(),
        };
        format!("{API_BASE}{url}?per_page={page_size}&page={page}{params}")
    }

    /// Perform a single GET request and deserialize the JSON response
    pub fn get_json(&self, url: &str, params: Option<&str>) -> Result<serde_json::Value, String> {
        let full_url = self.build_url(url, params);
        let content = self.get_raw(&full_url)?;
        serde_json::from_str::<serde_json::Value>(&content)
            .map_err(|e| format!("Could not deserialize GitHub's response. Error: {e}"))
    }

    /// Perform a paginated GET request and collect all the items into a set.
    ///
    /// The endpoint is expected to return a top-level JSON array.
    pub fn get_paginated<T>(
        &self,
        url: &str,
        page_size: u8,
        params: Option<&str>,
    ) -> Result<HashSet<T>, String>
    where
        T: DeserializeOwned + std::hash::Hash + std::cmp::Eq,
    {
        let mut page = 1;
        let mut all_items = HashSet::new();
        let mut tries: u8 = 0;
        loop {
            let full_url = self.build_paginated_url(url, page_size, page, params);
            let content = self.get_raw(&full_url)?;

            match serde_json::from_str::<GitHubResponse<T>>(content.as_str()) {
                Ok(GitHubResponse::Data(data)) => {
                    // When we go past the end (an unneeded page), we get an
                    // empty response so we can break.
                    if data.is_empty() {
                        break;
                    }
                    page += 1;
                    tries = 0;
                    all_items.extend(data);
                }
                Ok(GitHubResponse::Error(e)) => {
                    // GitHub threw an error: if it's a rate limit we wait and retry
                    if e.message.contains("API rate limit exceeded") {
                        sleep(Duration::from_secs(RATE_LIMIT_SLEEP_SECS));
                    } else if tries >= DEFAULT_RETRIES {
                        println!("{}", "Retries exhausted".red());
                        return Err(e.to_string());
                    } else {
                        tries += 1;
                        println!(
                            "{}: {}",
                            "Going to retry because couldn't deserialize response from GitHub:"
                                .yellow(),
                            e.to_string().red()
                        );
                        println!("{}", content.yellow());
                    }
                }
                Err(e) => {
                    if tries >= DEFAULT_RETRIES {
                        println!("{}", "Retries exhausted".red());
                        return Err(e.to_string());
                    }
                    tries += 1;
                    println!(
                        "{}: {}",
                        "Going to retry because couldn't deserialize response from GitHub:"
                            .yellow(),
                        e.to_string().red()
                    );
                    println!("{}", content.yellow());
                }
            }
        }

        Ok(all_items)
    }

    /// Like [`Self::get_paginated`], but index the results by [`GitHubIndex`]
    pub fn get_paginated_indexed<T>(
        &self,
        url: &str,
        page_size: u8,
        params: Option<&str>,
    ) -> Result<HashMap<String, T>, String>
    where
        T: DeserializeOwned + std::hash::Hash + std::cmp::Eq + GitHubIndex,
    {
        let results: HashSet<T> = self.get_paginated(url, page_size, params)?;
        Ok(results
            .into_iter()
            .map(|item| (item.index(), item))
            .collect::<HashMap<String, T>>())
    }

    /// Paginate an endpoint whose array is nested under a JSON key (e.g. the
    /// search API's `items` or the installations API's `installations`).
    ///
    /// Unlike [`Self::get_paginated`], the array is not at the top level, so we
    /// cannot use the `GitHubResponse` enum directly. We therefore detect error
    /// bodies explicitly: without that check, a rate-limit or permission error
    /// (which has no array under `key`) would be mistaken for "no more pages"
    /// and silently reported as an empty result.
    pub fn get_paginated_values(
        &self,
        url: &str,
        key: &str,
        page_size: u8,
        params: Option<&str>,
    ) -> Result<Vec<serde_json::Value>, String> {
        let mut page = 1;
        let mut all_items = vec![];
        let mut tries: u8 = 0;
        loop {
            let full_url = self.build_paginated_url(url, page_size, page, params);
            let content = self.get_raw(&full_url)?;
            let value: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| format!("Could not deserialize GitHub's response. Error: {e}"))?;

            // GitHub error bodies carry a `message` and no array under `key`.
            if let Some(message) = value.get("message").and_then(|m| m.as_str()) {
                if message.contains("API rate limit exceeded") {
                    sleep(Duration::from_secs(RATE_LIMIT_SLEEP_SECS));
                    continue;
                }
                if tries >= DEFAULT_RETRIES {
                    println!("{}", "Retries exhausted".red());
                    return Err(message.to_string());
                }
                tries += 1;
                println!(
                    "{}: {}",
                    "Going to retry because GitHub returned an error:".yellow(),
                    message.red()
                );
                continue;
            }

            let items = value
                .get(key)
                .and_then(|i| i.as_array())
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                break;
            }
            tries = 0;
            all_items.extend(items);
            page += 1;
        }
        Ok(all_items)
    }

    /// Make a GraphQL query on GitHub
    pub fn graphql(&self, query: GraphQLQuery) -> Result<serde_json::Value, String> {
        let body = serde_json::to_string(&query)
            .map_err(|e| format!("Could not serialize the GraphQL query: {e}"))?;

        let mut tries = 0;
        let content = loop {
            tries += 1;
            match self
                .client
                .post(GRAPHQL_URL)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", API_VERSION)
                .header("Authorization", format!("Bearer {}", self.token))
                .body(body.clone())
                .send()
                .and_then(|response| response.text())
            {
                Ok(content) => break content,
                Err(e) => {
                    if tries >= DEFAULT_RETRIES {
                        println!("{}", "Retries exhausted".red());
                        return Err(e.to_string());
                    }
                    println!(
                        "{}: {}",
                        "Going to retry because of a GitHub request error:".yellow(),
                        e.to_string().red()
                    );
                }
            }
        };

        serde_json::from_str::<serde_json::Value>(&content)
            .map_err(|e| format!("Could not deserialize GitHub's response. Error: {e}"))
    }

    // ------------------------------------------------------------------
    // Endpoint helpers
    // ------------------------------------------------------------------

    /// Fetch all repositories in the org.
    ///
    /// * `page_size` - the size of each page when making requests to the GH API
    /// * `active_only` - consider only active repositories (non-archived and not disabled)
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

        let repositories: HashSet<Repository> =
            match self.get_paginated(&format!("/orgs/{}/repos", self.org), page_size, None) {
                Ok(repositories) => repositories,
                Err(e) => {
                    return Err(format!(
                        "{}: {}",
                        "I couldn't fetch the repositories".red(),
                        e
                    ));
                }
            };

        let total = repositories.len();
        let repositories: HashSet<Repository> = if active_only {
            repositories
                .into_iter()
                .filter(|r| !r.archived && !r.disabled)
                .collect()
        } else {
            repositories
        };

        println!(
            "{} {} (active_only = {}, total = {})",
            "Success! I found: ".green(),
            repositories.len(),
            active_only,
            total
        );
        if !repositories.iter().any(|repo| repo.private) {
            println!("{}", "I didn't find any private repositories. Make sure you have permission to read private repositories.".red());
        }

        Ok(repositories)
    }

    /// Fetch the names of all repositories in the org, sorted for stable output.
    pub fn repo_names(&self, active_only: bool) -> Result<Vec<String>, String> {
        let mut names: Vec<String> = self
            .fetch_all_repositories(REPOS_PAGE_SIZE, active_only)?
            .into_iter()
            .map(|r| r.name)
            .collect();
        names.sort();
        Ok(names)
    }

    /// Resolve an optional list of repos: if `None`, fetch all repos from the org.
    pub fn resolve_repos(&self, repos: Option<Vec<String>>) -> Vec<String> {
        repos.unwrap_or_else(|| self.repo_names(false).unwrap())
    }

    /// Fetch all organization members, optionally filtered by role
    /// (e.g. `Some("admin")`), indexed by login.
    pub fn org_members(&self, role: Option<&str>) -> Result<HashMap<String, Member>, String> {
        let params = role.map(|r| format!("role={r}"));
        self.get_paginated_indexed(
            &format!("/orgs/{}/members", self.org),
            MEMBERS_PAGE_SIZE,
            params.as_deref(),
        )
    }

    /// Fetch all teams in the org, indexed by slug
    pub fn org_teams(&self) -> Result<HashMap<String, Team>, String> {
        self.get_paginated_indexed(
            &format!("/orgs/{}/teams", self.org),
            DEFAULT_PAGE_SIZE,
            None,
        )
    }

    /// Fetch all teams in the org as a set
    pub fn org_teams_set(&self) -> Result<HashSet<Team>, String> {
        self.get_paginated(
            &format!("/orgs/{}/teams", self.org),
            DEFAULT_PAGE_SIZE,
            None,
        )
    }

    /// Get collaborators for a given repository
    pub fn repo_collaborators(&self, repo: &str) -> Result<HashSet<Collaborator>, String> {
        self.get_paginated(
            &format!("/repos/{}/{}/collaborators", self.org, repo),
            DEFAULT_PAGE_SIZE,
            None,
        )
    }

    /// Get the teams that have access to the repo
    pub fn repo_teams(&self, repo: &str) -> Result<HashSet<Team>, String> {
        self.get_paginated(
            &format!("/repos/{}/{}/teams", self.org, repo),
            DEFAULT_PAGE_SIZE,
            None,
        )
    }

    /// Get the visibility of a repository, given its name
    pub fn repo_visibility(&self, repo: &str) -> Result<String, String> {
        let res = self.get_json(&format!("/repos/{}/{repo}", self.org), None)?;
        res.get("visibility")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
            .ok_or("Missing visibility in the response from GitHub".to_string())
    }

    /// Get the email address associated to a username, if available, through
    /// the configured SAML IdP.
    pub fn email_from_gh_username(&self, user: impl Display) -> Option<String> {
        let q = GraphQLQuery {
            query: crate::gql_queries::USER2EMAIL.to_string(),
            variables: [
                ("org".to_string(), self.org.clone()),
                ("user".to_string(), user.to_string()),
            ]
            .into(),
        };
        self.graphql(q)
            .ok()?
            .get("data")
            .and_then(|v| v.get("organization"))
            .and_then(|v| v.get("samlIdentityProvider"))
            .and_then(|v| v.get("externalIdentities"))
            .and_then(|v| v.get("edges"))
            .and_then(|v| v.as_array())
            .and_then(|v| v.first())
            .and_then(|v| v.get("node"))
            .and_then(|v| v.get("samlIdentity"))
            .and_then(|v| v.get("nameId"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
    }
}

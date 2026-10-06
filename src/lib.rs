use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    fs::File,
    io::BufWriter,
    path::Path,
    thread::sleep,
    time::Duration,
};

use colored::Colorize;

pub mod alerts;
pub mod bpr;
pub mod codeowners;
pub mod compliance;
pub mod deploy_key;
pub mod external_collaborator;
pub mod gql_queries;
pub mod members;
pub mod repos;
pub mod teams;
pub mod uar;
pub mod utils;

const GRAPHQL_URL: &str = "https://api.github.com/graphql";
const API_BASE: &str = "https://api.github.com";

pub trait GitHubIndex {
    fn index(&self) -> String;
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq, Clone)]
pub struct Permissions {
    pull: bool,
    triage: bool,
    push: bool,
    maintain: bool,
    admin: bool,
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq, Clone)]
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
    pub visibility: String,
    // We leave this as a generic Value because its contents seem to change
    // depending on some org-level settings (e.g., whether GHAS is enabled).
    pub security_and_analysis: Option<serde_json::Value>,
}

impl Repository {
    /// Returns whether a given security property is available and enabled.
    fn is_security_property_enabled(&self, property: &str) -> bool {
        self.security_and_analysis
            .as_ref()
            .and_then(|sa| sa.get(property))
            .and_then(|v| v.get("status"))
            .and_then(|st| st.as_str())
            .unwrap_or("")
            == "enabled"
    }
}

#[derive(serde::Deserialize, Hash, Eq, PartialEq, Clone)]
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
            &bootstrap.client,
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

    /// Fetch members of this team, including members of child teams
    fn fetch_team_members(&self, bootstrap: &Bootstrap) -> Result<HashMap<String, Member>, String> {
        make_paginated_github_request_with_index(
            &bootstrap.client,
            &bootstrap.token,
            25,
            &format!("/orgs/{}/teams/{}/members", &bootstrap.org, self.slug),
            3,
            None,
        )
    }
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
#[serde(untagged)]
enum GitHubResponse<T> {
    Data(Vec<T>),
    Error(GitHubError),
}

/// Execute an HTTP request with retry logic. The `request_fn` closure builds
/// and sends the request; this helper handles transient failures and retries.
fn execute_with_retries<F>(retries: u8, mut request_fn: F) -> Result<String, String>
where
    F: FnMut() -> Result<String, reqwest::Error>,
{
    let mut tries = 0;
    loop {
        tries += 1;
        match request_fn() {
            Ok(content) => return Ok(content),
            Err(e) => {
                if tries >= retries {
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

fn make_github_request(
    client: &reqwest::blocking::Client,
    gh_token: &str,
    url: &str,
    retries: u8,
    params: Option<&str>,
) -> Result<serde_json::Value, String> {
    let params = match params {
        Some(params) => format!("?{params}"),
        None => String::new(),
    };

    let full_url = format!("{API_BASE}{url}{params}");
    let content = execute_with_retries(retries, || {
        client
            .get(&full_url)
            .header("User-Agent", "GitHub EC Audit")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Authorization", format!("Bearer {}", gh_token))
            .send()
            .and_then(|response| response.text())
    })?;

    serde_json::from_str::<serde_json::Value>(&content)
        .map_err(|e| format!("Could not deserialize GitHub's response. Error: {e}"))
}

fn make_paginated_github_request<T>(
    client: &reqwest::blocking::Client,
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
    let mut tries: u8 = 0;
    loop {
        tries += 1;
        let full_url = format!("{API_BASE}{url}?per_page={page_size}&page={page}{params}");

        let content = match execute_with_retries(retries, || {
            client
                .get(&full_url)
                .header("User-Agent", "GitHub EC Audit")
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header("Authorization", format!("Bearer {}", gh_token))
                .send()
                .and_then(|response| response.text())
        }) {
            Ok(content) => content,
            Err(e) => return Err(e),
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
    client: &reqwest::blocking::Client,
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
        make_paginated_github_request(client, gh_token, page_size, url, retries, params)?;

    Ok(results
        .into_iter()
        .map(|item| (item.index(), item))
        .collect::<HashMap<String, T>>())
}

pub struct Bootstrap {
    token: String,
    org: String,
    client: reqwest::blocking::Client,
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

        let client = reqwest::blocking::Client::new();

        Ok(Self { token, org, client })
    }

    /// Fetch all repositories in an org.
    /// Args:
    /// * `page_size` - The size of each page when making requests to the GH API
    /// * `active_only` - Consider only active repositories (non-archived and not disabled)
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
            &self.client,
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
        if !repositories
            .iter()
            .fold(false, |acc, repo| acc || repo.private)
        {
            println!("{}", "I didn't find any private repositories. Make sure you have permission to read private repositories.".red());
        }

        Ok(repositories)
    }

    /// Resolve an optional list of repos: if None, fetch all repos from the org.
    pub fn resolve_repos(&self, repos: Option<Vec<String>>) -> Vec<String> {
        repos.unwrap_or_else(|| {
            self.fetch_all_repositories(75, false)
                .unwrap()
                .into_iter()
                .map(|r| r.name)
                .collect()
        })
    }
}

/// Get collaborators for a given repository
fn get_repo_collaborators(
    bootstrap: &Bootstrap,
    repo: &str,
) -> Result<HashSet<Collaborator>, String> {
    make_paginated_github_request(
        &bootstrap.client,
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
        &bootstrap.client,
        &bootstrap.token,
        25,
        &format!("/repos/{}/{}/teams", &bootstrap.org, repo),
        3,
        None,
    )
}

/// Get the visibility of a repository, given its name
fn get_repo_visibility(bootstrap: &Bootstrap, repo: &str) -> Result<String, String> {
    let res = make_github_request(
        &bootstrap.client,
        &bootstrap.token,
        &format!("/repos/{}/{repo}", bootstrap.org),
        3,
        None,
    )?;
    res.get("visibility")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .ok_or("Missing visibility in the response from GitHub".to_string())
}

#[derive(serde::Serialize)]
pub struct GraphQLQuery {
    query: String,
    variables: HashMap<String, String>,
}

/// Make a GraphQL query on GitHub
fn make_graphql_query(
    client: &reqwest::blocking::Client,
    gh_token: &str,
    query: GraphQLQuery,
    retries: u8,
) -> Result<serde_json::Value, String> {
    let query = serde_json::to_string(&query).unwrap();

    let content = execute_with_retries(retries, || {
        client
            .post(GRAPHQL_URL)
            .header("User-Agent", "GitHub EC Audit")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Authorization", format!("Bearer {}", gh_token))
            .body(query.clone())
            .send()
            .and_then(|response| response.text())
    })?;

    serde_json::from_str::<serde_json::Value>(&content)
        .map_err(|e| format!("Could not deserialize GitHub's response. Error: {e}"))
}

/// Get the email address associated to a username, if available, through the configured SAML IdP.
fn email_from_gh_username(bootstrap: &Bootstrap, user: impl Display) -> Option<String> {
    let q = GraphQLQuery {
        query: crate::gql_queries::USER2EMAIL.to_string(),
        variables: [
            ("org".to_string(), bootstrap.org.clone()),
            ("user".to_string(), user.to_string()),
        ]
        .into(),
    };
    make_graphql_query(&bootstrap.client, &bootstrap.token, q, 3)
        .ok()?
        .get("data")
        .and_then(|v| v.get("organization"))
        .and_then(|v| v.get("samlIdentityProvider"))
        .and_then(|v| v.get("externalIdentities"))
        .and_then(|v| v.get("edges"))
        .and_then(|v| v.as_array())
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("node"))
        .and_then(|v| v.get("samlIdentity"))
        .and_then(|v| v.get("nameId"))
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
}

/// Progress tracker for repo-iteration audits
pub struct ProgressTracker {
    one_percent: usize,
    progress: usize,
}

impl ProgressTracker {
    pub fn new(total: usize) -> Self {
        Self {
            one_percent: (total as f64 * 0.01).ceil() as usize,
            progress: 0,
        }
    }

    pub fn tick(&mut self) {
        self.progress += 1;
        if self.progress % self.one_percent == 0 {
            println!(
                "Processed {} repositories",
                self.progress.to_string().blue()
            );
        }
    }
}

/// Create a BufWriter to a CSV file, creating parent directories as needed.
pub fn create_csv_writer(csv_file: &str) -> BufWriter<File> {
    let path = Path::new(csv_file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect(&"Could not create folders".red());
    }
    let file = File::create(path).expect(&"Could not create CSV file".red());
    BufWriter::new(file)
}

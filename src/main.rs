use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    path::Path,
    thread::sleep,
    time::Duration,
};

use colored::Colorize;

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq, Clone)]
struct OutsideCollaborator {
    login: String,
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
struct Permissions {
    pull: bool,
    triage: bool,
    push: bool,
    maintain: bool,
    admin: bool,
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

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Hash, Eq, PartialEq)]
struct ExternalCollaboratorPermission {
    #[serde(rename = "GitHub User")]
    login: String,
    #[serde(rename = "Repo")]
    repository: String,
    #[serde(rename = "Access")]
    access: String,
    #[serde(rename = "Status")]
    status: Option<String>,
    #[serde(rename = "JIRA Ticket")]
    ticket: Option<String>,
    #[serde(rename = "Quorum Proposal")]
    proposal: Option<String>,
}

impl ExternalCollaboratorPermission {
    fn new(login: String, repository: String, access: String) -> Self {
        Self {
            login,
            repository,
            access,
            status: None,
            ticket: None,
            proposal: None,
        }
    }
}

type ExternalCollaboratorPermissions = HashMap<(String, String), ExternalCollaboratorPermission>;

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
struct Collaborator {
    login: String,
    permissions: Permissions,
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
struct Repository {
    name: String,
    private: bool,
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
#[serde(untagged)]
enum GitHubResponse<T> {
    Data(Vec<T>),
    Error(GitHubError),
}

fn make_paginated_github_request<T>(
    gh_token: &str,
    page_size: u8,
    url: &str,
    retries: u8,
) -> Result<HashSet<T>, String>
where
    T: serde::de::DeserializeOwned + std::hash::Hash + std::cmp::Eq,
{
    let mut page = 1;
    let mut all_items = HashSet::new();
    let mut tries = 0;
    loop {
        tries += 1;
        let response = reqwest::blocking::Client::new()
            .get(&format!(
                "https://api.github.com{}?per_page={page_size}&page={}",
                url, page
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

fn parse_previous_run_csv(file: impl AsRef<Path>) -> ExternalCollaboratorPermissions {
    let mut reader = csv::Reader::from_path(file).unwrap();
    reader
        .deserialize()
        .into_iter()
        .filter_map(|x: Result<ExternalCollaboratorPermission, _>| {
            if let Ok(x) = x {
                Some(((x.login.clone(), x.repository.clone()), x))
            } else {
                println!("{}: {:?}", "Couldn't parse a row".red(), x);
                None
            }
        })
        .collect()
}

fn generate_csv(ec_permissions: ExternalCollaboratorPermissions) -> String {
    let mut writer = csv::Writer::from_writer(vec![]);
    for (_, permission) in ec_permissions {
        writer.serialize(permission).unwrap();
    }
    String::from_utf8(writer.into_inner().unwrap()).unwrap()
}

fn main() {
    println!("{}", "GitHub EC Audit".white().bold());
    println!(
        "{}",
        "I'm checking there is a GitHub FPAT in the GH_TOKEN environment variable...".yellow()
    );

    // Get GH_TOKEN from the environment
    let gh_token = match std::env::var("GH_TOKEN") {
        Ok(token) => token,
        Err(_) => {
            panic!("{}", "GH_TOKEN not found so I've gotta stop".red());
        }
    };
    println!("{} {}...", "I have token:".green(), &gh_token[..20]);

    println!("{}", "I'm checking there is an org in GH_ORG.".yellow());

    // Get GH_ORG from the environment
    let gh_org = match std::env::var("GH_ORG") {
        Ok(org) => org,
        Err(_) => {
            panic!("{}", "GH_ORG not found so I've gotta stop".red());
        }
    };

    println!("{} {}", "I have organization:".green(), gh_org.white());

    let previous_ec_permissions = match std::env::args().len() {
        1 => {
            println!(
                "{}",
                "I don't see any arguments, I'm going to assume this is the first run.".yellow()
            );
            ExternalCollaboratorPermissions::new()
        }
        2 => {
            println!(
                "{}",
                "I see an argument, I'm going to assume that's a CSV with the output from a previous run.".yellow()
            );
            parse_previous_run_csv(std::env::args().nth(1).unwrap())
        }
        _ => {
            panic!(
                "{}",
                "I see too many arguments, I'm going to assume that's a mistake.".red()
            );
        }
    };

    println!(
        "{} {}",
        "I've got this many people from previous runs:".green(),
        previous_ec_permissions.len()
    );

    println!(
        "{}",
        "I'm going to fetch all external collaborators from the org".yellow(),
    );

    let outside_collaborators: HashSet<OutsideCollaborator> = match make_paginated_github_request(
        &gh_token,
        100,
        &format!("/orgs/{gh_org}/outside_collaborators"),
        3,
    ) {
        Ok(outside_collaborators) => outside_collaborators,
        Err(e) => {
            panic!(
                "{}: {e}",
                "I couldn't fetch the outside collaborators".red()
            );
        }
    };

    println!(
        "{} {}",
        "Success! I found: ".green(),
        outside_collaborators.len()
    );

    println!(
        "{}",
        "Alright! Now I need to fetch all repositories so I can check for their access.".yellow()
    );

    let repositories: HashSet<Repository> =
        match make_paginated_github_request(&gh_token, 75, &format!("/orgs/{gh_org}/repos"), 3) {
            Ok(repositories) => repositories,
            Err(e) => {
                panic!(
                    "{}: {e}",
                    "I couldn't fetch the outside collaborators".red()
                );
            }
        };

    println!("{} {}", "Success! I found: ".green(), repositories.len());
    if !repositories
        .iter()
        .fold(false, |acc, repo| acc || repo.private)
    {
        println!("{}", "I didn't find any private repositories. Make sure you have permission to read private repositories.".red());
    }

    println!("{}", "Finally the big one, I'm going to check each repository one by one to find external collaborators and their access. This is going to take a while...".yellow());

    let one_percent = (repositories.len() as f64 * 0.01).ceil() as usize;
    let mut progress = 0;
    let mut never_seen_outside_collaborators = outside_collaborators.clone();

    let mut ec_permissions = ExternalCollaboratorPermissions::new();

    for repository in repositories {
        let collaborators: HashSet<Collaborator> = match make_paginated_github_request(
            &gh_token,
            25,
            &format!("/repos/{gh_org}/{}/collaborators", repository.name),
            3,
        ) {
            Ok(collaborators) => collaborators,
            Err(e) => {
                panic!(
                    "{} {}: {e}",
                    repository.name.white(),
                    "I couldn't fetch the repository collaborators".red()
                );
            }
        };

        for collaborator in collaborators {
            if outside_collaborators.contains(&OutsideCollaborator {
                login: collaborator.login.clone(),
            }) {
                match previous_ec_permissions
                    .get(&(collaborator.login.clone(), repository.name.clone()))
                {
                    Some(ec_perm) => {
                        if ec_perm.access != collaborator.permissions.highest_perm() {
                            println!(
                                "{}: {} {} {}",
                                "I found a change in access so clearing approvals for".yellow(),
                                collaborator.login.white(),
                                "in".yellow(),
                                repository.name.white(),
                            );
                            ec_permissions.insert(
                                (collaborator.login.clone(), repository.name.clone()),
                                ExternalCollaboratorPermission::new(
                                    collaborator.login.clone(),
                                    repository.name.clone(),
                                    collaborator.permissions.highest_perm(),
                                ),
                            );
                        } else {
                            ec_permissions.insert(
                                (collaborator.login.clone(), repository.name.clone()),
                                ec_perm.clone(),
                            );
                        }
                    }
                    None => {
                        ec_permissions.insert(
                            (collaborator.login.clone(), repository.name.clone()),
                            ExternalCollaboratorPermission::new(
                                collaborator.login.clone(),
                                repository.name.clone(),
                                collaborator.permissions.highest_perm(),
                            ),
                        );
                    }
                };
                never_seen_outside_collaborators.remove(&OutsideCollaborator {
                    login: collaborator.login.clone(),
                });
            }
        }
        progress += 1;
        if progress % one_percent == 0 {
            println!("Processed {} reposistories", progress.to_string().blue());
        }
    }

    println!(
        "{}: {} different access permissions",
        "I'm done and I found:".green(),
        ec_permissions.len()
    );

    println!(
        "{} {:?}",
        "These external collaborators have no access to any repository weirdly enough".yellow(),
        never_seen_outside_collaborators
    );

    println!("{}", "Here's your updated CSV".green());
    println!("{}", generate_csv(ec_permissions));
}

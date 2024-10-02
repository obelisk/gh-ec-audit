use std::collections::HashSet;

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
struct Collaborator {
    login: String,
    permissions: Permissions,
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
struct Repository {
    name: String,
    private: bool,
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
            .map(|response| response.json::<Vec<T>>());

        match response {
            Ok(Ok(response)) => {
                // When we go past the end (an unneeded page), we'll get an empty response so we can break
                if response.is_empty() {
                    break;
                }

                // The page is full so we need to add all these users to our set and grab the next page
                page += 1;
                tries = 0;
                all_items.extend(response);
            }
            Ok(Err(e)) => {
                // This occurs if deserialization fails because the structure is wrong type or GitHub returns
                // something wonky to us
                if tries >= retries {
                    println!("{}", "Tries exhausted".red());
                    return Err(e.to_string());
                }
            }
            Err(e) => {
                // If we fail to send to request for some reason
                if tries >= retries {
                    println!("{}", "Tries exhausted".red());
                    return Err(e.to_string());
                }
            }
        }
    }

    Ok(all_items)
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
                println!(
                    "EC: {} {}: {}",
                    collaborator.login.white(),
                    repository.name.white(),
                    collaborator.permissions.highest_perm().red(),
                );
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
        "{} {:?}",
        "These external collaborators have no access to any repository weirdly enough".yellow(),
        never_seen_outside_collaborators
    );
}

use std::collections::HashSet;

use colored::Colorize;

pub mod bpr;
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
pub struct Repository {
    pub name: String,
    pub private: bool,
    pub permissions: Permissions,
}

pub struct Bootstrap {
    pub token: String,
    pub org: String,
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

    pub fn fetch_all_repositories(&self, page_size: u8) -> Result<HashSet<Repository>, String> {
        println!(
            "{}",
            "I'm going to fetch all repositories from the org".yellow()
        );

        let repositories: HashSet<Repository> = match utils::make_paginated_github_request(
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

use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::constants::{MEMBERS_PAGE_SIZE, REPOS_PAGE_SIZE};
use crate::{GitHubClient, Member, ProgressTracker, Repository};

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
struct DeployKey {
    id: u64,
    key: String,
    url: String,
    title: String,
    verified: bool,
    created_at: String,
    read_only: bool,
    added_by: String,
    last_used: Option<String>,
    enabled: bool,
}

pub fn run_audit(client: GitHubClient, _previous_csv: Option<String>, all: bool) {
    println!("{}", "GitHub Deploy Key Audit".white().bold());

    println!("{}", "Fetching all organization members".yellow());
    let members: HashMap<String, Member> = match client.org_members(None) {
        Ok(members) => members,
        Err(e) => {
            panic!(
                "{}: {}",
                "I couldn't fetch the organization members".red(),
                e
            );
        }
    };

    println!("{} {}", "Success! I found: ".green(), members.len());

    let repositories: HashSet<Repository> = client
        .fetch_all_repositories(REPOS_PAGE_SIZE, false)
        .unwrap();

    println!("{}", "Finally the big one, I'm going to check each repository one by one to find deploy keys and their access. This is going to take a while...".yellow());

    let mut tracker = ProgressTracker::new(repositories.len());

    for repository in repositories {
        let deploy_keys: HashSet<DeployKey> = match client.get_paginated(
            &format!("/repos/{}/{}/keys", client.org(), repository.name),
            MEMBERS_PAGE_SIZE,
            None,
        ) {
            Ok(dks) => dks,
            Err(e) => {
                panic!(
                    "{} {}: {e}",
                    repository.name.white(),
                    "I couldn't fetch the repository deploy keys".red()
                );
            }
        };

        for deploy_key in deploy_keys {
            match (all, members.contains_key(&deploy_key.added_by)) {
                (true, is_member) => {
                    println!(
                        "{} has deploy key {} {}: {}",
                        repository.name.white(),
                        deploy_key.title.yellow(),
                        if is_member {
                            "added by member".yellow()
                        } else {
                            "added by non-member".red()
                        },
                        deploy_key.added_by.white()
                    );
                }
                // We don't want all, and they are not a member so we
                // log
                (false, false) => {
                    println!(
                        "{} has deploy key {} {}: {}",
                        repository.name.white(),
                        deploy_key.title.yellow(),
                        "added by non-member".red(),
                        deploy_key.added_by.white()
                    );
                }
                // We don't want all, and they are members so we can skip
                // this deploy key
                (false, true) => (),
            }
        }

        tracker.tick();
    }
}

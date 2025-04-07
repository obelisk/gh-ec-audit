use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::{
    make_paginated_github_request, make_paginated_github_request_with_index, Bootstrap, Member,
};

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

pub fn run_audit(bootstrap: Bootstrap, _previous_csv: Option<String>) {
    println!("{}", "GitHub Deploy Key Audit".white().bold());

    println!("{}", "Fetching all organization members".yellow());
    let members: HashMap<String, Member> = match make_paginated_github_request_with_index(
        &bootstrap.token,
        75,
        &format!("/orgs/{}/members", &bootstrap.org),
        3,
        None,
    ) {
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

    let repositories = bootstrap.fetch_all_repositories(75).unwrap();

    println!("{}", "Finally the big one, I'm going to check each repository one by one to find deploy keys and their access. This is going to take a while...".yellow());

    let one_percent = (repositories.len() as f64 * 0.01).ceil() as usize;
    let mut progress = 0;

    for repository in repositories {
        let deploy_keys: HashSet<DeployKey> = match make_paginated_github_request(
            &bootstrap.token,
            25,
            &format!("/repos/{}/{}/keys", &bootstrap.org, repository.1.name),
            3,
            None,
        ) {
            Ok(dks) => dks,
            Err(e) => {
                panic!(
                    "{} {}: {e}",
                    repository.1.name.white(),
                    "I couldn't fetch the repository deploy keys".red()
                );
            }
        };

        for deploy_key in deploy_keys {
            if !members.contains_key(&deploy_key.added_by) {
                println!(
                    "{} has deploy key {} {}: {}",
                    repository.1.name.white(),
                    deploy_key.title.yellow(),
                    "added by a non-member".red(),
                    deploy_key.added_by.white()
                );
            }
        }

        progress += 1;
        if progress % one_percent == 0 {
            println!("Processed {} reposistories", progress.to_string().blue());
        }
    }
}

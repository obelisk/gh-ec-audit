use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use colored::Colorize;

use crate::{
    get_repo_collaborators, make_paginated_github_request_with_index, Bootstrap, GitHubIndex,
    Repository,
};

pub type ExternalCollaboratorPermissions =
    HashMap<(String, String), ExternalCollaboratorPermission>;

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq, Clone)]
pub struct OutsideCollaborator {
    login: String,
}

impl GitHubIndex for OutsideCollaborator {
    fn index(&self) -> String {
        self.login.clone()
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Hash, Eq, PartialEq)]
pub struct ExternalCollaboratorPermission {
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

pub fn run_audit(bootstrap: Bootstrap, previous_csv: Option<String>) {
    println!("{}", "GitHub External Collaborator Audit".white().bold());

    let previous_ec_permissions = match previous_csv {
        None => {
            println!(
                "{}",
                "I don't see any previous CSV file so I'm going to assume this is the first run."
                    .yellow()
            );
            ExternalCollaboratorPermissions::new()
        }
        Some(previous_csv) => {
            println!(
                "{}",
                "I see a path so I'm going to assume it's a CSV with the output from a previous run."
                    .yellow()
            );
            parse_previous_run_csv(previous_csv)
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

    let outside_collaborators: HashMap<String, OutsideCollaborator> =
        match make_paginated_github_request_with_index(
            &bootstrap.token,
            100,
            &format!("/orgs/{}/outside_collaborators", &bootstrap.org),
            3,
            None,
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

    let repositories: HashSet<Repository> = bootstrap.fetch_all_repositories(75).unwrap();

    println!("{}", "Finally the big one, I'm going to check each repository one by one to find external collaborators and their access. This is going to take a while...".yellow());

    let one_percent = (repositories.len() as f64 * 0.01).ceil() as usize;
    let mut progress = 0;
    let mut never_seen_outside_collaborators = outside_collaborators.clone();

    let mut ec_permissions = ExternalCollaboratorPermissions::new();

    for repository in repositories {
        let collaborators = get_repo_collaborators(&bootstrap, &repository.name);

        for collaborator in collaborators {
            if outside_collaborators.contains_key(&collaborator.login) {
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
                never_seen_outside_collaborators.remove(&collaborator.login);
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

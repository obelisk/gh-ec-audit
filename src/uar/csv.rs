use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use colored::Colorize;

use crate::{email_from_gh_username, Bootstrap, Collaborator, Permissions, Team};

/// Which format we are following when exporting data to CSV
pub(crate) enum CsvFormat {
    CodeOwners,
    Traditional,
}

/// Write to a CSV file the information we collected during a repo audit
pub(crate) fn repo_audit_to_csv(
    bootstrap: &Bootstrap,
    csv_file: impl Display,
    users: &HashSet<Collaborator>,
    teams: &HashSet<Team>,
    format: CsvFormat,
) {
    // Create file and all intermediate folders if necessary
    let csv_file = csv_file.to_string();
    let path = Path::new(&csv_file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect(&"Could not create folders".red());
    }
    let file = File::create(path).expect(&"Could not create CSV file".red());
    let mut writer = BufWriter::new(file);

    match format {
        CsvFormat::CodeOwners => {
            // Write headers
            writeln!(writer, "login,user_or_team,email")
                .expect(&"Could not write to CSV file".red());

            // Write users
            for u in users {
                writeln!(
                    writer,
                    "{},User,{}",
                    u.login,
                    email_from_gh_username(&bootstrap, &u.login)
                        .unwrap_or("Not available".to_string()),
                )
                .expect(&"Could not write to CSV file".red());
            }

            // Write teams
            for t in teams {
                writeln!(writer, "{},Team,None", t.slug,)
                    .expect(&"Could not write to CSV file".red());
            }
        }
        CsvFormat::Traditional => {
            // Write headers
            writeln!(writer, "login,user_or_team,email,permissions")
                .expect(&"Could not write to CSV file".red());

            // Write users
            for u in users {
                writeln!(
                    writer,
                    "{},User,{},{}",
                    u.login,
                    email_from_gh_username(&bootstrap, &u.login)
                        .unwrap_or("Not available".to_string()),
                    u.permissions.highest_perm()
                )
                .expect(&"Could not write to CSV file".red());
            }

            // Write teams
            for t in teams {
                writeln!(
                    writer,
                    "{},Team,None,{}",
                    t.slug,
                    t.permissions.as_ref().unwrap().highest_perm()
                )
                .expect(&"Could not write to CSV file".red());
            }
        }
    }

    println!(
        "{} {}",
        "Successfully written file".green(),
        csv_file.white()
    );
}

/// Write to a CSV file all the access that teams have
pub(crate) fn team_access_to_csv(
    csv_file: impl Display,
    teams_to_repos: &HashMap<String, HashSet<(String, Permissions)>>,
) {
    // Create file and all intermediate folders if necessary
    let csv_file = csv_file.to_string();
    let path = Path::new(&csv_file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect(&"Could not create folders".red());
    }
    let file = File::create(path).expect(&"Could not create CSV file".red());
    let mut writer = BufWriter::new(file);

    // Write headers
    writeln!(writer, "team,repo,permissions").expect(&"Could not write to CSV file".red());

    for (team, access) in teams_to_repos {
        for (repo, permissions) in access {
            writeln!(writer, "{team},{repo},{}", permissions.highest_perm())
                .expect(&"Could not write to CSV file".red());
        }
    }

    println!(
        "{} {}",
        "Successfully written file".green(),
        csv_file.white()
    );
}

/// Write to a CSV file all the members of the teams we encountered
pub(crate) fn team_members_to_csv(
    bootstrap: &Bootstrap,
    csv_file: impl Display,
    teams_to_repos: &HashMap<String, HashSet<(String, Permissions)>>,
) {
    // Create file and all intermediate folders if necessary
    let csv_file = csv_file.to_string();
    let path = Path::new(&csv_file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect(&"Could not create folders".red());
    }
    let file = File::create(path).expect(&"Could not create CSV file".red());
    let mut writer = BufWriter::new(file);

    // Write headers
    writeln!(writer, "team,user,email").expect(&"Could not write to CSV file".red());

    for (team, _) in teams_to_repos {
        // A temporary team object just to be able to call the fetch_members method
        let tmp_team = Team {
            slug: team.to_string(),
            name: team.to_string(),
            permissions: None,
        };
        let members: Vec<String> = tmp_team
            .fetch_team_members(bootstrap)
            .unwrap()
            .keys()
            .map(|v| v.to_string())
            .collect();

        for user in &members {
            writeln!(
                writer,
                "{team},{user},{}",
                email_from_gh_username(bootstrap, user).unwrap_or("Not available".to_string())
            )
            .expect(&"Could not write to CSV file".red());
        }
    }

    println!(
        "{} {}",
        "Successfully written file".green(),
        csv_file.white()
    );
}

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    io::Write,
};

use colored::Colorize;

use crate::{create_csv_writer, fetch_team_members, GitHubClient, Permissions};

/// Write to a CSV file the information we collected during a repo audit
pub(crate) fn repo_audit_to_csv(csv_file: impl Display, lines: &[String]) {
    let csv_file = csv_file.to_string();
    let mut writer = create_csv_writer(&csv_file);

    let mut count_entries = 0;

    for line in lines {
        writeln!(writer, "{line}").expect("Could not write to CSV file");
        count_entries += 1;
    }

    count_entries -= 1; // Remove header from count

    println!(
        "{} {}: {} {} {}",
        "Successfully written file".green(),
        csv_file.white(),
        "There were".green(),
        count_entries.to_string().white(),
        "entries".green()
    );
}

/// Write to a CSV file all the access that teams have
pub(crate) fn team_access_to_csv(
    csv_file: impl Display,
    teams_to_repos: &HashMap<String, HashSet<(String, Option<Permissions>)>>,
) {
    let csv_file = csv_file.to_string();
    let mut writer = create_csv_writer(&csv_file);

    // Write headers
    writeln!(writer, "team,repo,permissions").expect("Could not write to CSV file");

    let mut count_entries = 0;

    for (team, access) in teams_to_repos {
        for (repo, permissions) in access {
            // If we found this team during a traditional UAR, then we will
            // have its permissions, otherwise it means we encountered it
            // during a CODEOWNERS UAR, and we set it simply to "Codeowner"
            let p = match permissions {
                Some(p) => p.highest_perm(),
                None => "Codeowner".to_string(),
            };
            writeln!(writer, "{team},{repo},{}", p).expect("Could not write to CSV file");
            count_entries += 1;
        }
    }

    println!(
        "{} {}: {} {} {}",
        "Successfully written file".green(),
        csv_file.white(),
        "There were".green(),
        count_entries.to_string().white(),
        "entries".green()
    );
}

/// Write to a CSV file all the members of the teams we encountered
pub(crate) fn team_members_to_csv(
    client: &GitHubClient,
    csv_file: impl Display,
    teams_to_repos: &HashMap<String, HashSet<(String, Option<Permissions>)>>,
) {
    let csv_file = csv_file.to_string();
    let mut writer = create_csv_writer(&csv_file);

    // Write headers
    writeln!(writer, "team,user,email").expect("Could not write to CSV file");

    let mut count_entries = 0;

    for team in teams_to_repos.keys() {
        let members: Vec<String> = fetch_team_members(client, team)
            .unwrap()
            .keys()
            .map(|v| v.to_string())
            .collect();

        for user in &members {
            writeln!(
                writer,
                "{team},{user},{}",
                client
                    .email_from_gh_username(user)
                    .unwrap_or("Not available".to_string())
            )
            .expect("Could not write to CSV file");
            count_entries += 1;
        }

        // Print out the number of members in this team
        println!("{team}: {} members", members.len());
    }

    println!(
        "{} {}: {} {} {}",
        "Successfully written file".green(),
        csv_file.white(),
        "There were".green(),
        count_entries.to_string().white(),
        "entries".green()
    );
}

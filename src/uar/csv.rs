use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::BufWriter,
    io::Write,
};

use colored::Colorize;

use crate::{Collaborator, Team};

/// Export the data collected during the audit to a CSV file.
pub(crate) fn export_to_csv(
    csv_file: &str,
    repo_access: &HashMap<String, (HashSet<Collaborator>, HashSet<Team>)>,
) {
    let mut file_lines = vec![];

    // repo, username, role, user/team
    for (repo, (users, teams)) in repo_access {
        for u in users {
            file_lines.push(format!(
                "{repo},{},{},user",
                u.login,
                u.permissions.highest_perm()
            ));
        }

        for t in teams {
            file_lines.push(format!(
                "{repo},{},{},team",
                t.slug,
                t.permissions.as_ref().unwrap().highest_perm()
            ));
        }
    }

    let file = File::create(csv_file).expect(&"Could not create CSV file".red());
    let mut writer = BufWriter::new(file);

    for line in file_lines {
        writeln!(writer, "{}", line).expect(&"Could not write to CSV file".red());
    }

    println!(
        "{} {}",
        "Data successfully exported to".green(),
        csv_file.white()
    );
}

mod audit;
/// Use an iterative approach, i.e., scan repos one by one
pub mod iterate;
/// Leverage the GH search API to find relevant information
mod search;

use std::{collections::HashSet, fmt::Display};

use crate::{members::get_indexed_org_members, teams::get_indexed_org_teams, GitHubClient};
use colored::Colorize;
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    /// Regex used to find matches in CO files that look like @something.
    static ref co_regex: Regex = Regex::new(r"@\S+").unwrap();
}

/// Represents a CODEOWNERS file
pub struct CodeownersFile {
    /// Repository this CO file belongs to
    pub repo: String,
    /// HTML URL, to give the user a quick way to reach the file
    pub url: String,
    /// List of all users mentioned in the file, for further analysis
    pub users: HashSet<String>,
    /// List of all teams mentioned in the file, for further analysis
    pub teams: HashSet<String>,
}

/// Strip an inline comment from a CODEOWNERS line.
///
/// CODEOWNERS treats `#` as the start of a comment anywhere on the line, so
/// `* @alice # @ghost` must not be read as mentioning `@ghost`.
fn strip_inline_comment(line: &str) -> &str {
    match line.find('#') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

impl CodeownersFile {
    /// Process the content of a CO file and turn it into a CodeownersFile struct.
    ///
    /// Takes the org slug (rather than a client) so the parsing logic can be
    /// unit-tested without a live GitHub client.
    fn parse_from_content(
        org: &str,
        content: String,
        html_url: &str,
        repo: &str,
    ) -> CodeownersFile {
        let mut users = HashSet::new();
        let mut teams = HashSet::new();

        // The prefix that teams have in CO files
        let team_prefix = format!("@{org}/");

        for line in content.split('\n') {
            // Drop comments: a `#` starts a comment anywhere on the line, so
            // this also removes full-line comments (leaving an empty line).
            let line = strip_inline_comment(line);
            if line.trim().is_empty() {
                continue;
            }

            for m in co_regex.find_iter(line) {
                let matched = m.as_str();
                if matched.starts_with(&team_prefix) {
                    // It's a team
                    teams.insert(matched.trim_start_matches(&team_prefix).to_string());
                } else {
                    // It's a user
                    users.insert(matched.trim_start_matches("@").to_string());
                }
            }
        }

        CodeownersFile {
            repo: repo.to_string(),
            url: html_url.to_string(),
            users,
            teams,
        }
    }
}

/// The problems we can encounter while auditing a CODEOWNERS file
enum CodeownersFileProblem {
    UserNotInOrg { user: String, co_file_url: String },
    TeamNotInOrg { team: String, co_file_url: String },
    EmptyTeam { team: String, co_file_url: String },
}

impl Display for CodeownersFileProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            CodeownersFileProblem::EmptyTeam { team, co_file_url } => format!(
                "{} {} {} {}",
                "Warning! CODEOWNERS file".yellow(),
                co_file_url.white(),
                "contains an empty team".yellow(),
                team.white(),
            ),
            CodeownersFileProblem::UserNotInOrg { user, co_file_url } => {
                format!(
                    "{} {} {} {} {}",
                    "Error in CODEOWNERS file".red(),
                    co_file_url.white(),
                    "User".red(),
                    user.white(),
                    "is not in the org".red()
                )
            }
            CodeownersFileProblem::TeamNotInOrg { team, co_file_url } => {
                format!(
                    "{} {} {} {} {}",
                    "Error in CODEOWNERS file".red(),
                    co_file_url.white(),
                    "Team".red(),
                    team.white(),
                    "is not in the org".red()
                )
            }
        };
        write!(f, "{msg}")
    }
}

/// Run the audit on CODEOWNERS files to determine
/// * If all users mentioned in the file exist and are members of the org
/// * If all teams mentioned in the file exist
///
/// We will also alert if a team is empty.
pub fn run_codeowners_audit(
    client: GitHubClient,
    repos: Option<Vec<String>>,
    search: bool,
    also_gh_api: bool,
    verbose: bool,
) {
    // Immediately stop if we received incompatible options
    if search && repos.is_some() {
        panic!("{}", "Using --search assumes an org-wide search, and it is not supported in conjunction with a list of repos (i.e., --repos).".red());
    }

    println!("{}", "I am fetching CODEOWNERS files...".yellow());

    // Build a list of CO files we will audit
    let codeowners_files = match search {
        true => search::find_codeowners_in_org(&client),
        false => iterate::find_codeowners_in_org(&client, repos.clone()),
    }
    .expect("Error while fetching CODEOWNERS file: I cannot continue");

    println!(
        "{} {} {}",
        "Done! I found".green(),
        codeowners_files.len().to_string().white(),
        "CODEOWNERS files".green()
    );

    println!(
        "{}",
        "Preparing to analyze these CODEOWNERS files...".yellow()
    );

    // Get all members and teams in the org, so that we can efficiently
    // cross-check the content of all the CODEOWNERS files we have found.
    let org_members = get_indexed_org_members(&client);
    let org_teams = get_indexed_org_teams(&client);

    audit::audit_co_files(
        &client,
        &codeowners_files,
        &org_members,
        &org_teams,
        also_gh_api,
        verbose,
    );
}

/// Look for all occurrences of that team in CODEOWNERS files across the org.
/// This is useful to estimate the impact on CODEOWNERS that removing or renaming a team would have.
pub fn run_team_in_codeowners_audit(
    client: GitHubClient,
    team: String,
    repos: Option<Vec<String>>,
    search: bool,
) {
    // Immediately stop if we received incompatible options
    if search && repos.is_some() {
        panic!("{}", "Using --search assumes an org-wide search, and it is not supported in conjunction with a list of repos (i.e., --repos).".red());
    }

    println!(
        "{} {} {}",
        "Searching for occurrences of team".yellow(),
        team.white(),
        "in CODEOWNERS files...".yellow()
    );
    if search {
        search::find_team_in_codeowners(&client, team)
    } else {
        iterate::find_team_in_codeowners(&client, team, repos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str, org: &str) -> CodeownersFile {
        CodeownersFile::parse_from_content(org, content.to_string(), "url", "repo")
    }

    #[test]
    fn parses_users_and_teams() {
        let content = "* @alice @my-org/platform\n/docs/ @bob\n";
        let co = parse(content, "my-org");
        assert!(co.users.contains("alice"));
        assert!(co.users.contains("bob"));
        assert!(co.teams.contains("platform"));
        assert!(!co.users.contains("my-org/platform"));
    }

    #[test]
    fn skips_full_line_comments() {
        let content = "# @ghost\n* @alice\n";
        let co = parse(content, "my-org");
        assert!(!co.users.contains("ghost"));
        assert!(co.users.contains("alice"));
    }

    #[test]
    fn ignores_inline_comments() {
        // `#` starts a comment anywhere on the line, so `@ghost` is not a mention
        let content = "* @alice # @ghost\n";
        let co = parse(content, "my-org");
        assert!(co.users.contains("alice"));
        assert!(!co.users.contains("ghost"));
    }

    #[test]
    fn ignores_inline_comment_after_team() {
        let content = "* @my-org/platform # @ghost\n";
        let co = parse(content, "my-org");
        assert!(co.teams.contains("platform"));
        assert!(!co.users.contains("ghost"));
    }

    #[test]
    fn treats_other_org_team_as_user() {
        // A team from another org is not recognised as a team of this org
        let content = "* @other-org/team\n";
        let co = parse(content, "my-org");
        assert!(co.teams.is_empty());
        assert!(co.users.contains("other-org/team"));
    }
}

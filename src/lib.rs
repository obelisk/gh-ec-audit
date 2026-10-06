use std::{collections::HashMap, fs::File, io::BufWriter, path::Path};

use colored::Colorize;

pub mod actors;
pub mod alerts;
pub mod bpr;
pub mod codeowners;
pub mod compliance;
pub mod constants;
pub mod deploy_key;
pub mod external_collaborator;
pub mod github;
pub mod gql_queries;
pub mod members;
pub mod repos;
pub mod teams;
pub mod uar;
pub mod utils;

pub use github::GitHubClient;

/// A type that can be indexed by a string key (e.g. a login or a slug)
pub trait GitHubIndex {
    fn index(&self) -> String;
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq, Clone)]
pub struct Permissions {
    pull: bool,
    triage: bool,
    push: bool,
    maintain: bool,
    admin: bool,
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq, Clone)]
pub struct Collaborator {
    login: String,
    permissions: Permissions,
}

impl Collaborator {
    pub fn login(&self) -> &str {
        &self.login
    }

    pub fn permissions(&self) -> &Permissions {
        &self.permissions
    }
}

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
pub struct Member {
    pub avatar_url: String,
    pub login: String,
}

impl GitHubIndex for Member {
    fn index(&self) -> String {
        self.login.clone()
    }
}

impl Permissions {
    pub fn highest_perm(&self) -> String {
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
    pub archived: bool,
    pub disabled: bool,
    pub permissions: Permissions,
    pub visibility: String,
    // We leave this as a generic Value because its contents seem to change
    // depending on some org-level settings (e.g., whether GHAS is enabled).
    pub security_and_analysis: Option<serde_json::Value>,
}

impl Repository {
    /// Returns whether a given security property is available and enabled.
    pub fn is_security_property_enabled(&self, property: &str) -> bool {
        self.security_and_analysis
            .as_ref()
            .and_then(|sa| sa.get(property))
            .and_then(|v| v.get("status"))
            .and_then(|st| st.as_str())
            .unwrap_or("")
            == "enabled"
    }
}

#[derive(serde::Deserialize, Hash, Eq, PartialEq, Clone)]
pub struct Team {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub permissions: Option<Permissions>,
}

impl GitHubIndex for Team {
    fn index(&self) -> String {
        self.slug.clone()
    }
}

impl Team {
    /// Return whether a team is empty, i.e., if the team has no members,
    /// including its sub-teams.
    pub fn is_empty(&self, client: &GitHubClient) -> Result<bool, String> {
        // NOTE - We don't make a paginated request on purpose: we only want
        // to see if a team is empty or not, and we don't need to fetch _all_ members.
        let members = client.get_json(
            &format!("/orgs/{}/teams/{}/members", client.org(), self.slug),
            None,
        )?;

        match members.as_array() {
            Some(v) => Ok(v.is_empty()),
            None => Err("The value returned by GitHub is not an array".to_string()),
        }
    }
}

/// Fetch members of a team, including members of child teams, indexed by login.
///
/// This is a free function (rather than a method on [`Team`]) so that callers
/// that only have a team slug do not need to fabricate a [`Team`] value.
pub fn fetch_team_members(
    client: &GitHubClient,
    slug: &str,
) -> Result<HashMap<String, Member>, String> {
    client.get_paginated_indexed(
        &format!("/orgs/{}/teams/{}/members", client.org(), slug),
        constants::DEFAULT_PAGE_SIZE,
        None,
    )
}

/// Progress tracker for repo-iteration audits
pub struct ProgressTracker {
    one_percent: usize,
    progress: usize,
}

impl ProgressTracker {
    pub fn new(total: usize) -> Self {
        Self {
            one_percent: (total as f64 * 0.01).ceil() as usize,
            progress: 0,
        }
    }

    pub fn tick(&mut self) {
        self.progress += 1;
        if self.one_percent > 0 && self.progress.is_multiple_of(self.one_percent) {
            println!(
                "Processed {} repositories",
                self.progress.to_string().blue()
            );
        }
    }
}

/// Escape a value for inclusion in a CSV field.
///
/// Values containing a comma, a double quote, a newline or a carriage return
/// are wrapped in double quotes, with any embedded double quote doubled, as
/// per RFC 4180. Values that need no escaping are returned unchanged.
pub fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Create a BufWriter to a CSV file, creating parent directories as needed.
pub fn create_csv_writer(csv_file: &str) -> BufWriter<File> {
    let path = Path::new(csv_file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("Could not create folders");
    }
    let file = File::create(path).expect("Could not create CSV file");
    BufWriter::new(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perms(pull: bool, triage: bool, push: bool, maintain: bool, admin: bool) -> Permissions {
        Permissions {
            pull,
            triage,
            push,
            maintain,
            admin,
        }
    }

    #[test]
    fn highest_perm_prefers_admin() {
        assert_eq!(perms(true, true, true, true, true).highest_perm(), "admin");
    }

    #[test]
    fn highest_perm_prefers_maintain_over_push() {
        assert_eq!(
            perms(true, true, true, true, false).highest_perm(),
            "maintain"
        );
    }

    #[test]
    fn highest_perm_prefers_push_over_triage() {
        assert_eq!(perms(true, true, true, false, false).highest_perm(), "push");
    }

    #[test]
    fn highest_perm_prefers_triage_over_pull() {
        assert_eq!(
            perms(true, true, false, false, false).highest_perm(),
            "triage"
        );
    }

    #[test]
    fn highest_perm_is_pull_when_only_pull() {
        assert_eq!(
            perms(true, false, false, false, false).highest_perm(),
            "pull"
        );
    }

    #[test]
    fn highest_perm_is_none_when_no_permission() {
        assert_eq!(
            perms(false, false, false, false, false).highest_perm(),
            "none"
        );
    }

    #[test]
    fn security_property_enabled_only_when_status_is_enabled() {
        let repo = Repository {
            name: "r".to_string(),
            private: false,
            archived: false,
            disabled: false,
            permissions: perms(false, false, false, false, false),
            visibility: "public".to_string(),
            security_and_analysis: Some(serde_json::json!({
                "secret_scanning": { "status": "enabled" },
                "dependabot_security_updates": { "status": "disabled" }
            })),
        };
        assert!(repo.is_security_property_enabled("secret_scanning"));
        assert!(!repo.is_security_property_enabled("dependabot_security_updates"));
        assert!(!repo.is_security_property_enabled("missing"));
    }

    #[test]
    fn security_property_is_false_when_analysis_missing() {
        let repo = Repository {
            name: "r".to_string(),
            private: false,
            archived: false,
            disabled: false,
            permissions: perms(false, false, false, false, false),
            visibility: "public".to_string(),
            security_and_analysis: None,
        };
        assert!(!repo.is_security_property_enabled("secret_scanning"));
    }

    #[test]
    fn progress_tracker_does_not_panic_on_empty_input() {
        // `one_percent` is 0 when total is 0: tick must not divide by zero.
        let mut tracker = ProgressTracker::new(0);
        tracker.tick();
    }

    #[test]
    fn csv_escape_leaves_plain_values_untouched() {
        assert_eq!(csv_escape("chainlink"), "chainlink");
        assert_eq!(csv_escape("admin"), "admin");
    }

    #[test]
    fn csv_escape_quotes_values_with_commas() {
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
    }

    #[test]
    fn csv_escape_doubles_embedded_quotes() {
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn csv_escape_quotes_values_with_newlines() {
        assert_eq!(csv_escape("a\nb"), "\"a\nb\"");
        assert_eq!(csv_escape("a\rb"), "\"a\rb\"");
    }
}

use std::cell::RefCell;
use std::collections::HashMap;

use colored::Colorize;
use serde::Deserialize;

use crate::constants::MEMBERS_PAGE_SIZE;
use crate::GitHubClient;

/// An actor that can bypass branch protection rules or rulesets, identified
/// by GitHub's actor type and ID. It can be rendered as a human-readable
/// string, resolving IDs to names where possible.
#[derive(Debug, Deserialize)]
pub struct BypassActor {
    #[serde(default)]
    pub actor_id: Option<i64>,
    pub actor_type: String,
    #[serde(default = "default_bypass_mode")]
    pub bypass_mode: String,
}

/// GitHub defaults the bypass mode to "always" when the field is missing
fn default_bypass_mode() -> String {
    "always".to_string()
}

/// Resolves actor IDs (teams, users, integrations, custom repository roles)
/// to human-readable names. The org-level sources are fetched once when the
/// resolver is created; users are looked up lazily and cached, as GitHub
/// offers no bulk endpoint for them.
pub struct ActorResolver {
    /// team id -> "name (slug)"
    teams: HashMap<i64, String>,
    /// installation id and app id -> app name (an integration can be
    /// referenced by either ID, depending on the endpoint)
    integrations: HashMap<i64, String>,
    /// custom repository role id -> role name
    custom_roles: HashMap<i64, String>,
    /// lazy cache: user id -> login (None = lookup already tried and failed)
    users: RefCell<HashMap<i64, Option<String>>>,
}

impl ActorResolver {
    /// Build a resolver for the given org. All the sources are optional:
    /// if a fetch fails (e.g., the token lacks the required permissions),
    /// the corresponding actors will fall back to their ID representation.
    pub fn new(client: &GitHubClient) -> Self {
        Self {
            teams: Self::fetch_teams(client),
            integrations: Self::fetch_integrations(client),
            custom_roles: Self::fetch_custom_roles(client),
            users: RefCell::new(HashMap::new()),
        }
    }

    /// Fetch all teams in the org, indexed by ID
    fn fetch_teams(client: &GitHubClient) -> HashMap<i64, String> {
        let teams = match client.org_teams_set() {
            Ok(teams) => teams,
            Err(e) => {
                println!(
                    "{}: {e}",
                    "I couldn't fetch the org teams: teams will be shown as IDs".yellow()
                );
                return HashMap::new();
            }
        };

        teams
            .into_iter()
            .map(|team| (team.id, format!("{} ({})", team.name, team.slug)))
            .collect()
    }

    /// Fetch all GitHub Apps installed on the org, indexed by both the
    /// installation ID and the app ID
    fn fetch_integrations(client: &GitHubClient) -> HashMap<i64, String> {
        let installations = match client.get_paginated_values(
            &format!("/orgs/{}/installations", client.org()),
            "installations",
            MEMBERS_PAGE_SIZE,
            None,
        ) {
            Ok(installations) => installations,
            Err(e) => {
                println!(
                    "{}: {e}",
                    "I couldn't fetch the org installations: integrations will be shown as IDs"
                        .yellow()
                );
                return HashMap::new();
            }
        };

        let mut integrations = HashMap::new();
        for installation in installations {
            // The org installations endpoint returns a flat `app_slug`
            // field, not a nested `app` object like other endpoints do
            let app_name = installation
                .get("app_slug")
                .or_else(|| installation.get("app").and_then(|a| a.get("name")))
                .and_then(|n| n.as_str())
                .map(|n| n.to_string());
            let app_name = match app_name {
                Some(name) => name,
                None => continue,
            };

            if let Some(id) = installation.get("id").and_then(|i| i.as_i64()) {
                integrations.insert(id, app_name.clone());
            }
            if let Some(app_id) = installation.get("app_id").and_then(|i| i.as_i64()) {
                integrations.insert(app_id, app_name);
            }
        }

        integrations
    }

    /// Fetch the custom repository roles defined in the org, indexed by ID.
    /// Built-in roles (IDs 1 to 5) are not included, as they have well-known
    /// names.
    fn fetch_custom_roles(client: &GitHubClient) -> HashMap<i64, String> {
        // The shape of the response has changed over time: it can be either
        // a bare array or an object wrapping the roles, so we accept both
        let res = match client.get_json(
            &format!("/orgs/{}/custom-repository-roles", client.org()),
            None,
        ) {
            Ok(res) => res,
            Err(e) => {
                println!(
                    "{}: {e}",
                    "I couldn't fetch the org's custom repository roles: custom roles will be shown as IDs"
                        .yellow()
                );
                return HashMap::new();
            }
        };

        // The response is an object wrapping the roles array
        let items = match res.get("custom_roles").and_then(|r| r.as_array()) {
            Some(items) => items.clone(),
            None => match res.as_array() {
                // Some API versions return a bare array
                Some(items) => items.clone(),
                None => return HashMap::new(),
            },
        };

        items
            .into_iter()
            .filter_map(|role| {
                let id = role.get("id").and_then(|i| i.as_i64())?;
                let name = role.get("name").and_then(|n| n.as_str())?.to_string();
                Some((id, name))
            })
            .collect()
    }

    /// Resolve a user ID to a login, looking it up and caching it on first use
    fn resolve_user(&self, client: &GitHubClient, user_id: i64) -> Option<String> {
        // The cache is full of resolved names and failures alike: a `None`
        // entry means we already tried and failed, so we don't retry
        if let Some(cached) = self.users.borrow().get(&user_id) {
            return cached.clone();
        }

        let login = match client.get_json(&format!("/user/{user_id}"), None) {
            Ok(res) => res
                .get("login")
                .and_then(|l| l.as_str())
                .map(|l| l.to_string()),
            Err(e) => {
                println!(
                    "{}: {e}",
                    format!("I couldn't fetch the user with id {user_id}").yellow()
                );
                None
            }
        };

        self.users.borrow_mut().insert(user_id, login.clone());
        login
    }

    /// Return a human-readable label for the given bypass actor
    pub fn actor_label(&self, client: &GitHubClient, actor: &BypassActor) -> String {
        self.actor_label_with(actor, |id| self.resolve_user(client, id))
    }

    /// Label an actor, delegating user-ID resolution to the given closure.
    /// This keeps the labelling logic testable without a live client.
    fn actor_label_with<F>(&self, actor: &BypassActor, resolve_user: F) -> String
    where
        F: Fn(i64) -> Option<String>,
    {
        match actor.actor_type.as_str() {
            "OrganizationAdmin" => "organization admins".to_string(),
            "DeployKey" => "a deploy key".to_string(),
            "RepositoryRole" => self.repository_role_label(actor.actor_id),
            "Team" => match actor.actor_id.and_then(|id| self.teams.get(&id)) {
                Some(team) => format!("team '{team}'"),
                None => format!("team #{}", actor.actor_id.unwrap_or(0)),
            },
            "User" => match actor.actor_id.and_then(&resolve_user) {
                Some(login) => format!("user '{login}'"),
                None => format!("user #{}", actor.actor_id.unwrap_or(0)),
            },
            "Integration" => match actor.actor_id.and_then(|id| self.integrations.get(&id)) {
                Some(app) => format!("integration '{app}'"),
                None => format!("integration #{}", actor.actor_id.unwrap_or(0)),
            },
            other => format!("{other} #{}", actor.actor_id.unwrap_or(0)),
        }
    }

    /// GitHub's built-in repository roles have well-known IDs; custom roles
    /// are resolved through the org's custom roles
    fn repository_role_label(&self, actor_id: Option<i64>) -> String {
        match actor_id {
            Some(1) => "repository role 'Read'".to_string(),
            Some(2) => "repository role 'Triage'".to_string(),
            Some(3) => "repository role 'Write'".to_string(),
            Some(4) => "repository role 'Maintain'".to_string(),
            Some(5) => "repository role 'Admin'".to_string(),
            Some(id) => match self.custom_roles.get(&id) {
                Some(name) => format!("repository role '{name}'"),
                None => format!("repository role #{id}"),
            },
            None => "a repository role".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> ActorResolver {
        ActorResolver {
            teams: HashMap::from([(42, "Platform (platform)".to_string())]),
            integrations: HashMap::from([(7, "my-app".to_string())]),
            custom_roles: HashMap::from([(16021, "Release Manager".to_string())]),
            users: RefCell::new(HashMap::new()),
        }
    }

    fn actor(actor_type: &str, actor_id: Option<i64>) -> BypassActor {
        BypassActor {
            actor_id,
            actor_type: actor_type.to_string(),
            bypass_mode: "always".to_string(),
        }
    }

    /// Label an actor with a stub user resolver, so no network is needed.
    fn label(r: &ActorResolver, actor: &BypassActor) -> String {
        r.actor_label_with(actor, |id| Some(format!("user{id}")))
    }

    #[test]
    fn labels_organization_admin_and_deploy_key() {
        let r = resolver();
        assert_eq!(
            label(&r, &actor("OrganizationAdmin", None)),
            "organization admins"
        );
        assert_eq!(label(&r, &actor("DeployKey", None)), "a deploy key");
    }

    #[test]
    fn labels_builtin_repository_roles() {
        let r = resolver();
        assert_eq!(
            label(&r, &actor("RepositoryRole", Some(1))),
            "repository role 'Read'"
        );
        assert_eq!(
            label(&r, &actor("RepositoryRole", Some(5))),
            "repository role 'Admin'"
        );
    }

    #[test]
    fn labels_custom_repository_role() {
        let r = resolver();
        assert_eq!(
            label(&r, &actor("RepositoryRole", Some(16021))),
            "repository role 'Release Manager'"
        );
        assert_eq!(
            label(&r, &actor("RepositoryRole", Some(999))),
            "repository role #999"
        );
        assert_eq!(
            label(&r, &actor("RepositoryRole", None)),
            "a repository role"
        );
    }

    #[test]
    fn labels_team_by_id() {
        let r = resolver();
        assert_eq!(
            label(&r, &actor("Team", Some(42))),
            "team 'Platform (platform)'"
        );
        assert_eq!(label(&r, &actor("Team", Some(1))), "team #1");
    }

    #[test]
    fn labels_integration_by_id() {
        let r = resolver();
        assert_eq!(
            label(&r, &actor("Integration", Some(7))),
            "integration 'my-app'"
        );
        assert_eq!(label(&r, &actor("Integration", Some(1))), "integration #1");
    }

    #[test]
    fn labels_user_via_resolver() {
        let r = resolver();
        assert_eq!(label(&r, &actor("User", Some(99))), "user 'user99'");
    }

    #[test]
    fn labels_unknown_actor_type() {
        let r = resolver();
        assert_eq!(
            label(&r, &actor("SomethingNew", Some(3))),
            "SomethingNew #3"
        );
    }
}

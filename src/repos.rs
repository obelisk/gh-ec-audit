use std::collections::HashSet;

use colored::Colorize;

use crate::{Bootstrap, Repository};

/// Count the number of non-archived repos with given visibility
/// that have a certain security property set to "enabled"
fn count_repos_security_property_enabled(
    repos: &HashSet<Repository>,
    visibility: &str,
    property: &str,
) -> usize {
    repos
        .iter()
        .filter(|r| {
            r.visibility == visibility && !r.archived && r.is_security_property_enabled(property)
        })
        .count()
}

/// Get a list of non-archived repos with given visibility that have
/// a certain security property not set to "enabled".
fn get_repos_security_property_not_enabled(
    repos: &HashSet<Repository>,
    visibility: &str,
    property: &str,
) -> Vec<String> {
    repos
        .iter()
        .filter_map(|r| {
            if r.visibility == visibility
                && !r.archived
                && !r.is_security_property_enabled(property)
            {
                Some(r.name.clone())
            } else {
                None
            }
        })
        .collect()
}

/// Run the repos audit
pub fn run_repos_audit(bootstrap: Bootstrap) {
    let repos = bootstrap.fetch_all_repositories(75).expect(&format!(
        "{}",
        "I could not fetch the list of repositories. I am giving up.".red()
    ));

    // Collect some numbers about the repos and the types
    let num_total = repos.len();

    let num_public = repos.iter().filter(|r| r.visibility == "public").count();
    let num_public_archived = repos
        .iter()
        .filter(|r| r.visibility == "public" && r.archived)
        .count();
    let num_public_non_archived = num_public - num_public_archived;

    let num_private = repos.iter().filter(|r| r.visibility == "private").count();
    let num_private_archived = repos
        .iter()
        .filter(|r| r.visibility == "private" && r.archived)
        .count();
    let _num_private_non_archived = num_private - num_private_archived;

    let num_internal = repos.iter().filter(|r| r.visibility == "internal").count();
    let num_internal_archived = repos
        .iter()
        .filter(|r| r.visibility == "internal" && r.archived)
        .count();
    let _num_internal_non_archived = num_internal - num_internal_archived;

    let num_archived = num_public_archived + num_private_archived + num_internal_archived;

    println!(
        "{} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
        "I have found".green(),
        num_total.to_string().white(),
        "repositories (".green(),
        num_archived.to_string().white(),
        "archived ) so divided:\n*".green(),
        num_public.to_string().white(),
        "public (".green(),
        num_public_archived.to_string().white(),
        "archived )\n*".green(),
        num_private.to_string().white(),
        "private (".green(),
        num_private_archived.to_string().white(),
        "archived )\n*".green(),
        num_internal.to_string().white(),
        "internal (".green(),
        num_internal_archived.to_string().white(),
        "archived )".green(),
    );

    // Collect some numbers about the repos' security posture
    let num_public_secret_scanning_enabled =
        count_repos_security_property_enabled(&repos, "public", "secret_scanning");
    let num_public_secret_scanning_push_prot_enabled =
        count_repos_security_property_enabled(&repos, "public", "secret_scanning_push_protection");
    let num_public_dependabot_enabled =
        count_repos_security_property_enabled(&repos, "public", "dependabot_security_updates");

    println!(
        "{} {} {} {} {}",
        "Out of the".green(),
        num_public_non_archived.to_string().white(),
        "non-archived public repos,".green(),
        num_public_secret_scanning_enabled.to_string().white(),
        "have secret scanning enabled".green()
    );

    println!(
        "{} {} {} {} {}",
        "Out of the".green(),
        num_public_non_archived.to_string().white(),
        "non-archived public repos,".green(),
        num_public_secret_scanning_push_prot_enabled
            .to_string()
            .white(),
        "have secret scanning push protection enabled".green()
    );

    println!(
        "{} {} {} {} {}",
        "Out of the".green(),
        num_public_non_archived.to_string().white(),
        "non-archived public repos,".green(),
        num_public_dependabot_enabled.to_string().white(),
        "have dependabot security updates enabled".green()
    );

    // Get a list of repos that have some security properties not set to "enabled"
    let repos_public_dependabot_disabled: Vec<String> =
        get_repos_security_property_not_enabled(&repos, "public", "dependabot_security_updates");
    let repos_public_secret_scanning_disabled: Vec<String> =
        get_repos_security_property_not_enabled(&repos, "public", "secret_scanning");

    println!(
        "{} {:?}",
        "Non-archived public repos with dependabot security updates not enabled".red(),
        repos_public_dependabot_disabled
    );

    println!(
        "{} {:?}",
        "Non-archived public repos with secret scanning not enabled".red(),
        repos_public_secret_scanning_disabled
    );
}

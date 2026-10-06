use std::fmt::Display;

use colored::Colorize;
use serde::Deserialize;

use crate::actors::{ActorResolver, BypassActor};
use crate::{make_github_request, make_paginated_github_request, Bootstrap, ProgressTracker};

/// A repository ruleset, as returned by the rulesets endpoint
#[derive(Debug, Deserialize)]
struct Ruleset {
    id: i64,
    name: String,
    #[serde(default = "default_target")]
    target: String,
    enforcement: String,
    #[serde(default)]
    source_type: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    bypass_actors: Vec<BypassActor>,
}

/// GitHub defaults the target to "branch" when the field is missing
fn default_target() -> String {
    "branch".to_string()
}

/// Deserialize a field that GitHub may set to `null` as the default value.
/// `#[serde(default)]` alone only covers *missing* fields, not null ones.
fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    let opt = Option::<T>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// Where a ruleset is configured (repo, org or enterprise), for display
fn ruleset_source(ruleset: &Ruleset) -> String {
    match (&ruleset.source_type, &ruleset.source) {
        (Some(source_type), Some(source)) => format!("{source_type} '{source}'"),
        _ => "unknown source".to_string(),
    }
}

/// The result of the bypass audit for one repository
#[derive(Debug)]
struct RepoBypassAudit {
    name: String,
    default_branch: String,
    /// Whether any traditional branch protection exists on the default branch
    has_bpr: bool,
    /// The raw branch protection, as returned by GitHub (for verbose output)
    bpr_raw: Option<serde_json::Value>,
    /// Ways to bypass the traditional branch protection
    bpr_bypasses: Vec<String>,
    /// The raw rulesets, as returned by GitHub (for verbose output)
    rulesets_raw: serde_json::Value,
    /// Ways to bypass the active rulesets
    ruleset_bypasses: Vec<String>,
    /// Rulesets that are not active, hence not enforced at all
    inactive_rulesets: Vec<String>,
    /// Names of rulesets that GitHub returned but that could not be parsed
    unparsed_rulesets: Vec<String>,
    /// Whether an active ruleset targeting branches applies to the repository
    has_active_branch_ruleset: bool,
}

impl RepoBypassAudit {
    /// Whether the default branch is protected at all
    fn is_unprotected(&self) -> bool {
        !self.has_bpr && !self.has_active_branch_ruleset
    }

    /// Whether the protections in place can be bypassed by someone
    fn is_bypassable(&self) -> bool {
        !self.bpr_bypasses.is_empty() || !self.ruleset_bypasses.is_empty()
    }

    /// Whether some rulesets could not be parsed
    fn has_unparsed_rulesets(&self) -> bool {
        !self.unparsed_rulesets.is_empty()
    }
}

fn get_default_branch(bootstrap: &Bootstrap, repo: impl Display) -> String {
    match make_github_request(
        &bootstrap.client,
        &bootstrap.token,
        &format!("/repos/{}/{repo}", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => {
            res.get("default_branch")
                .unwrap()
                .as_str()
                .unwrap()
                .to_string() // unwraps OK: required field in GH response
        }
        Err(e) => {
            panic!(
                "{} for repo {}: {}",
                "I couldn't fetch the repo's default branch".red(),
                repo,
                e
            );
        }
    }
}

/// Fetch the traditional branch protection for the given branch.
/// Returns `None` if there is no branch protection (GitHub replies with a 404).
fn get_bpr(
    bootstrap: &Bootstrap,
    repo: impl Display,
    branch: impl Display,
) -> Option<serde_json::Value> {
    match make_github_request(
        &bootstrap.client,
        &bootstrap.token,
        &format!(
            "/repos/{}/{repo}/branches/{branch}/protection",
            bootstrap.org
        ),
        3,
        None,
    ) {
        Ok(res) => {
            // GitHub replies with a 404-shaped body when no protection exists
            if res.get("status").and_then(|s| s.as_str()) == Some("404") {
                return None;
            }
            Some(res)
        }
        Err(e) => {
            panic!(
                "{} for repo {}: {}",
                "I couldn't fetch the repo's BPRs".red(),
                repo,
                e
            );
        }
    }
}

/// Fetch all rulesets applying to the given repo, including the ones
/// configured at the org level. Returns the raw response from GitHub.
fn get_rulesets_raw(bootstrap: &Bootstrap, repo: impl Display) -> serde_json::Value {
    match make_paginated_github_request::<serde_json::Value>(
        &bootstrap.client,
        &bootstrap.token,
        100,
        &format!("/repos/{}/{repo}/rulesets", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => serde_json::Value::Array(res.into_iter().collect()),
        Err(e) => {
            panic!(
                "{} for repo {}: {}",
                "I couldn't fetch the repo's rulesets".red(),
                repo,
                e
            );
        }
    }
}

/// Fetch a single ruleset with all its details, including its bypass actors.
/// The rulesets *list* endpoint does not populate `bypass_actors`: to see who
/// can bypass a ruleset, each ruleset must be fetched individually.
fn get_ruleset_details(
    bootstrap: &Bootstrap,
    repo: impl Display,
    ruleset_id: i64,
) -> Option<serde_json::Value> {
    match make_github_request(
        &bootstrap.client,
        &bootstrap.token,
        &format!("/repos/{}/{repo}/rulesets/{ruleset_id}", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => {
            // GitHub replies with a 404-shaped body when we cannot access the
            // ruleset (e.g., an org-level ruleset the token cannot see)
            if res.get("status").and_then(|s| s.as_str()) == Some("404") {
                None
            } else {
                Some(res)
            }
        }
        Err(e) => {
            println!(
                "{} for repo {}: ruleset {ruleset_id}: {e}",
                "I couldn't fetch the ruleset's details".yellow(),
                repo
            );
            None
        }
    }
}

/// Parse the raw rulesets response into typed rulesets. Rulesets that cannot
/// be parsed are collected separately, so that we can report them: dropping
/// them silently would hide rules (and their bypass actors) from the audit.
fn parse_rulesets(raw: &serde_json::Value, repo: impl Display) -> (Vec<Ruleset>, Vec<String>) {
    let items = match raw.as_array() {
        Some(items) => items,
        None => panic!(
            "{} for repo {}: the response from GitHub is not an array",
            "I couldn't fetch the repo's rulesets".red(),
            repo
        ),
    };

    let mut rulesets = vec![];
    let mut unparsed = vec![];

    for item in items {
        match serde_json::from_value::<Ruleset>(item.clone()) {
            Ok(ruleset) => rulesets.push(ruleset),
            Err(e) => {
                // Try to salvage the name for reporting
                let name = item
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("<unknown>");
                println!("{}: '{}' ({e})", "Couldn't parse a ruleset".red(), name);
                unparsed.push(name.to_string());
            }
        }
    }

    (rulesets, unparsed)
}

/// Analyze a traditional BPR and return the ways it can be bypassed.
/// We don't judge the content of the rule (that's visible in the dump):
/// we only look for actors or actions that can circumvent it.
fn analyze_bpr(bpr: &serde_json::Value) -> Vec<String> {
    let mut bypasses = vec![];

    // Admins: `enforce_admins` can be a boolean or an object with an
    // `enabled` field, depending on the API version. When admins are not
    // enforced, they can push straight to the branch, ignoring the rule.
    let admins_enforced = match bpr.get("enforce_admins") {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(enforce) => enforce
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        None => false,
    };
    if !admins_enforced {
        bypasses.push("administrators can bypass the rule".to_string());
    }

    // Classic BPRs expose an explicit list of actors allowed to bypass
    // required pull request reviews. GitHub returns it as an object with
    // `users`, `teams` and `apps` arrays, each entry carrying its own name.
    if let Some(allowances) = bpr
        .get("required_pull_request_reviews")
        .and_then(|reviews| reviews.get("bypass_pull_request_allowances"))
    {
        for kind in ["users", "teams", "apps"] {
            for actor in allowances
                .get(kind)
                .and_then(|a| a.as_array())
                .unwrap_or(&vec![])
            {
                let name = actor
                    .get("login")
                    .or_else(|| actor.get("slug"))
                    .or_else(|| actor.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown");
                bypasses.push(format!(
                    "explicit bypass allowance for required reviews: {kind} '{name}'"
                ));
            }
        }
    }

    bypasses
}

/// Analyze the rulesets of a repo. Returns the ways the active rulesets can
/// be bypassed, and the list of inactive rulesets (which are not enforced at
/// all, so they neither protect the repo nor can they be bypassed).
fn analyze_rulesets(
    resolver: &ActorResolver,
    bootstrap: &Bootstrap,
    rulesets: &[Ruleset],
) -> (Vec<String>, Vec<String>) {
    let mut bypasses = vec![];
    let mut inactive = vec![];

    for ruleset in rulesets {
        // Collect inactive rulesets separately: they are not enforced, so
        // they are reported for completeness but not analyzed
        if ruleset.enforcement != "active" {
            inactive.push(format!(
                "'{}' ({}, {})",
                ruleset.name,
                ruleset.target,
                ruleset_source(ruleset)
            ));
            continue;
        }

        // Bypass actors can circumvent the rules of the ruleset
        for actor in &ruleset.bypass_actors {
            bypasses.push(format!(
                "ruleset '{}' ({}, {}): {} can bypass ({})",
                ruleset.name,
                ruleset.target,
                ruleset_source(ruleset),
                resolver.actor_label(bootstrap, actor),
                actor.bypass_mode
            ));
        }
    }

    (bypasses, inactive)
}

/// Print a recap of the findings to the terminal
fn print_recap(results: &[RepoBypassAudit]) {
    let num_unprotected = results.iter().filter(|r| r.is_unprotected()).count();
    let num_bypassable = results.iter().filter(|r| r.is_bypassable()).count();
    let num_no_bypass = results
        .iter()
        .filter(|r| !r.is_unprotected() && !r.is_bypassable())
        .count();

    println!("{}", "BPR BYPASS AUDIT RECAP".white().bold());
    println!(
        "{} {} {}",
        "I have audited".green(),
        results.len().to_string().white(),
        "repositories:".green()
    );
    println!(
        "* {} {}",
        num_unprotected.to_string().white(),
        "have no protection at all on their default branch".green()
    );
    println!(
        "* {} {}",
        num_bypassable.to_string().white(),
        "have protections that can be bypassed".green()
    );
    println!(
        "* {} {}",
        num_no_bypass.to_string().white(),
        "have protections with no bypass path found".green()
    );

    if num_unprotected > 0 {
        println!(
            "{}",
            "Repos with no protection on their default branch:".yellow()
        );
        for result in results.iter().filter(|r| r.is_unprotected()) {
            println!("  - {}", result.name.white());
        }
    }

    if num_bypassable > 0 {
        println!(
            "{}",
            "Repos with protections that can be bypassed:".yellow()
        );
        for result in results.iter().filter(|r| r.is_bypassable()) {
            println!("  - {}", result.name.white());
        }
    }

    let num_with_unparsed = results.iter().filter(|r| r.has_unparsed_rulesets()).count();
    if num_with_unparsed > 0 {
        println!(
            "{}",
            "Repos with rulesets that could not be parsed (they are NOT included in the analysis):"
                .yellow()
        );
        for result in results.iter().filter(|r| r.has_unparsed_rulesets()) {
            println!(
                "  - {}: {}",
                result.name.white(),
                result.unparsed_rulesets.join(", ")
            );
        }
    }
}

/// Run the branch protection bypass audit on the given repos (or all org repos
/// if none are passed). When `verbose` is set, also dump the raw BPRs and
/// rulesets as returned by GitHub.
pub fn run_audit(bootstrap: Bootstrap, repos: Option<Vec<String>>, verbose: bool) {
    println!("{}", "GitHub Branch Protection Bypass Audit".white().bold());
    println!(
        "{}",
        "Note: rulesets configured at the org level are only listed for tokens with the org-level Administration permission, and bypass actors are only returned for tokens with write access to the ruleset.".yellow()
    );

    let repos = bootstrap.resolve_repos(repos);

    // Resolve actor IDs (teams, users, integrations, custom roles) to names
    let resolver = ActorResolver::new(&bootstrap);

    let mut tracker = ProgressTracker::new(repos.len());
    let mut results: Vec<RepoBypassAudit> = vec![];

    for repo in &repos {
        let default_branch = get_default_branch(&bootstrap, repo);
        let bpr = get_bpr(&bootstrap, repo, &default_branch);
        let rulesets_raw = get_rulesets_raw(&bootstrap, repo);
        let (mut rulesets, unparsed_rulesets) = parse_rulesets(&rulesets_raw, repo);

        // The list endpoint does not populate bypass_actors: fetch each
        // ruleset individually to get them. We only need the details of
        // active rulesets: the others are not enforced, so their bypass
        // actors are irrelevant.
        for ruleset in rulesets.iter_mut().filter(|r| r.enforcement == "active") {
            if let Some(details) = get_ruleset_details(&bootstrap, repo, ruleset.id) {
                match serde_json::from_value::<Ruleset>(details) {
                    Ok(detailed) => ruleset.bypass_actors = detailed.bypass_actors,
                    Err(e) => println!(
                        "{}: '{}' ({e})",
                        "Couldn't parse a ruleset's details".red(),
                        ruleset.name
                    ),
                }
            } else {
                println!(
                    "{}: '{}' (id {})",
                    "I couldn't fetch the details of ruleset".yellow(),
                    ruleset.name,
                    ruleset.id
                );
            }
        }

        let (ruleset_bypasses, inactive_rulesets) =
            analyze_rulesets(&resolver, &bootstrap, &rulesets);

        results.push(RepoBypassAudit {
            name: repo.clone(),
            default_branch: default_branch.clone(),
            has_bpr: bpr.is_some(),
            bpr_raw: bpr.clone(),
            bpr_bypasses: bpr.as_ref().map(analyze_bpr).unwrap_or_default(),
            rulesets_raw: rulesets_raw.clone(),
            ruleset_bypasses,
            inactive_rulesets,
            unparsed_rulesets,
            has_active_branch_ruleset: rulesets
                .iter()
                .any(|r| r.enforcement == "active" && (r.target == "branch" || r.target == "push")),
        });

        tracker.tick();
    }

    // Print the findings, repo by repo
    println!();
    for result in &results {
        println!(
            "{} {} ({})",
            "Repo:".yellow(),
            result.name.white().bold(),
            result.default_branch.white()
        );

        if result.is_unprotected() {
            println!("   {}", "No protection at all on the default branch".red());
        } else {
            for bypass in &result.bpr_bypasses {
                println!("   {} {}", "BPR bypass:".red(), bypass);
            }
            for bypass in &result.ruleset_bypasses {
                println!("   {} {}", "Ruleset bypass:".red(), bypass);
            }

            if !result.inactive_rulesets.is_empty() {
                println!("   {}", "Inactive rulesets (not enforced):".yellow());
                for inactive in &result.inactive_rulesets {
                    println!("      - {inactive}");
                }
            }

            if !result.is_bypassable() {
                println!("   {}", "No bypass path found".green());
            }
        }

        if result.has_unparsed_rulesets() {
            println!(
                "   {} {}",
                "Unparsed rulesets (not analyzed):".yellow(),
                result.unparsed_rulesets.join(", ")
            );
        }

        // Dump the raw BPRs and rulesets, as the original audit did
        if verbose {
            let bprs_raw = result
                .bpr_raw
                .as_ref()
                .map(|bpr| serde_json::to_string_pretty(bpr).unwrap())
                .unwrap_or_else(|| "Empty".to_string());
            println!("{} {bprs_raw}\n", "          BPRs:".yellow());

            let rulesets_raw = serde_json::to_string_pretty(&result.rulesets_raw).unwrap();
            println!("{} {rulesets_raw}\n\n", "      Rulesets:".yellow());
        }

        println!();
    }

    // Print the recap
    print_recap(&results);
}

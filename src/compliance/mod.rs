mod rules;
mod utils;

use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::Path;
use std::{thread, time::Duration};

use crate::{make_github_request, Bootstrap};

#[derive(Default, Debug, Clone, PartialEq)]
/// Result of a single compliance check.
enum Check {
    /// The check is satisfied
    Pass,
    /// The check is not satisfied
    #[default]
    Fail,
    /// The check could not be evaluated due to lack of access (HTTP 403)
    NoAccess,
}

#[derive(PartialEq)]
enum Errors {
    NoAccess403,
    MissingOrError,
}

#[derive(Default, Debug, Clone)]
struct ProtectionChecks {
    /// Pull requests require at least one approving review
    pr_one_approval: Check,
    /// Stale reviews are dismissed when new commits are pushed to the PR
    pr_dismiss_stale: Check,
    /// Pull requests require CODEOWNERS approval
    pr_require_code_owner: Check,
    /// Force-pushes are disabled (non fast-forward enforced)
    disable_force_push: Check,
    /// Deletion of the protected branch is disabled
    disable_deletion: Check,
    /// Commits to the protected branch must be signed
    require_signed_commits: Check,
    /// Required status checks are configured and must pass before merging
    require_status_checks: Check,
    /// CODEOWNERS file exists and GitHub reports zero parsing/ownership errors
    codeowners_valid: Check,
}

/// Weights for the different checks (in case some are more important than others)
#[derive(Clone)]
struct Weights {
    pr_one_approval: u32,
    pr_dismiss_stale: u32,
    pr_require_code_owner: u32,
    disable_force_push: u32,
    disable_deletion: u32,
    require_signed_commits: u32,
    require_status_checks: u32,
    codeowners_valid: u32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            pr_one_approval: 1,
            pr_dismiss_stale: 1,
            pr_require_code_owner: 1,
            disable_force_push: 1,
            disable_deletion: 1,
            require_signed_commits: 1,
            require_status_checks: 1,
            codeowners_valid: 1,
        }
    }
}

/// Response received from GH when querying BPRs
#[derive(Debug, Deserialize)]
struct BprResponse {
    allow_force_pushes: Option<EnabledFlag>,
    allow_deletions: Option<EnabledFlag>,
    required_signatures: Option<EnabledFlag>,
    required_status_checks: Option<RequiredStatusChecks>,
    required_pull_request_reviews: Option<PullRequestReviews>,
}

/// The representation of a boolean value in GH API
#[derive(Debug, Deserialize)]
struct EnabledFlag {
    enabled: bool,
}

/// Required status checks in a BPR or ruleset
#[derive(Debug, Deserialize)]
struct RequiredStatusChecks {
    checks: Vec<serde_json::Value>,
}

/// Info about PR reviews
#[derive(Debug, Deserialize)]
struct PullRequestReviews {
    required_approving_review_count: u32,
    dismiss_stale_reviews: bool,
    require_code_owner_reviews: bool,
}

/// A ruleset and its parameters.
/// Rulesets are the newer version of BPRs.
#[derive(Debug, Deserialize)]
struct RulesetRule {
    #[serde(rename = "type")]
    type_: String,
    parameters: Option<serde_json::Value>,
}

/// Information about a repository
struct RepoInfo {
    branch: String,
    visibility: Option<String>,
}

/// Status of a CODEOWNERS file for a given repo.
enum CodeownersStatus {
    Valid,
    Invalid,
    Missing,
}

pub fn run_compliance_audit(
    bootstrap: Bootstrap,
    repos: Option<Vec<String>>,
    csv_path: Option<String>,
    active_repo_only: bool,
    selected_checks: Option<Vec<String>>,
) {
    // Select checks we are interested in
    let selected_set: Option<HashSet<String>> = selected_checks.map(|v| {
        v.into_iter()
            .map(|s| s.to_lowercase())
            .collect::<HashSet<String>>()
    });
    // Simple function to determine if a check has been selected.
    // If the user passed no selection, then all checks are selected by default.
    let is_selected = |name: &str| -> bool {
        match &selected_set {
            None => true,
            Some(s) => s.contains(&name.to_lowercase()),
        }
    };
    // All the repositories we will check
    let repos = repos.unwrap_or_else(|| {
        let mut list = bootstrap
            .fetch_all_repositories(75, active_repo_only)
            .unwrap()
            .into_iter()
            .filter(|r| !active_repo_only || (!r.archived && !r.disabled))
            .map(|r| r.name)
            .collect::<Vec<String>>();
        list.sort();
        list
    });

    // Prepare CSV writer if requested; support appending and skipping already-processed repos
    let mut already_processed: HashSet<String> = HashSet::new();
    let mut csv_writer = match csv_path {
        Some(ref path) => {
            if Path::new(path).exists() {
                already_processed = utils::read_existing_csv_repos(path);
            }
            let should_write_header = match std::fs::metadata(path) {
                Ok(m) => m.len() == 0,
                Err(_) => true,
            };
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("Unable to open CSV file for append");
            let buf = BufWriter::new(file);
            let mut wtr = csv::WriterBuilder::new()
                .has_headers(false)
                .from_writer(buf);
            if should_write_header {
                wtr.write_record([
                    "repository",
                    "default_branch",
                    "visibility",
                    "pr_one_approval",
                    "pr_dismiss_stale",
                    "pr_require_code_owner",
                    "disable_force_push",
                    "disable_deletion",
                    "require_signed_commits",
                    "require_status_checks",
                    "codeowners_valid",
                    "codeowners_path",
                ])
                .expect("Unable to write CSV header");
                wtr.flush().ok();
            }
            Some(wtr)
        }
        None => None,
    };

    // If we have previously processed repos and CSV export is enabled, filter them out
    let repos = if csv_writer.is_some() && !already_processed.is_empty() {
        repos
            .into_iter()
            .filter(|r| !already_processed.contains(r))
            .collect::<Vec<String>>()
    } else {
        repos
    };

    // Progress bar
    let pb = ProgressBar::new(repos.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}",
        )
        .unwrap()
        .progress_chars("=>-"),
    );

    for repo in repos {
        pb.set_message(repo.clone());

        // Retry default branch resolution to be resilient to transient failures
        let max_attempts = 3;
        let mut attempt = 0;
        let (default_branch_opt, visibility_opt) = loop {
            attempt += 1;
            match get_default_branch(&bootstrap, &repo) {
                Ok(info) => {
                    break (Some(info.branch), info.visibility);
                }
                Err(Errors::NoAccess403) => {
                    println!(
                        "{} {}: {}",
                        "Skipping repo".yellow(),
                        repo.white(),
                        "default branch not accessible (403)".red()
                    );
                    break (None, None);
                }
                Err(Errors::MissingOrError) => {
                    if attempt < max_attempts {
                        thread::sleep(Duration::from_millis(600));
                        continue;
                    } else {
                        println!(
                            "{} {}: {}",
                            "Skipping repo".yellow(),
                            repo.white(),
                            "could not determine default branch".red()
                        );
                        break (None, None);
                    }
                }
            }
        };
        // If something went wrong above and we did not get the default branch, skip this repo
        let Some(default_branch) = default_branch_opt else {
            pb.inc(1);
            continue;
        };

        // Initialize default (all failing) checks, which we will update as we scan through BPRs and rulesets
        let mut checks = ProtectionChecks::default();

        // Determine which sources are needed based on selected checks: do we need to check BPRs?
        let need_bpr = is_selected("disable_force_push")
            || is_selected("disable_deletion")
            || is_selected("require_signed_commits")
            || is_selected("pr_one_approval")
            || is_selected("pr_dismiss_stale")
            || is_selected("pr_require_code_owner")
            || is_selected("require_status_checks");

        if need_bpr {
            // 1) Classic BPR with retries on transient errors
            let mut attempts = 0;
            let bpr_fetch = loop {
                attempts += 1;
                match rules::get_bpr(&bootstrap, &repo, &default_branch) {
                    Err(Errors::MissingOrError) => {
                        if attempts < max_attempts {
                            thread::sleep(Duration::from_millis(500));
                            continue;
                        } else {
                            break Err(Errors::MissingOrError);
                        }
                    }
                    Err(Errors::NoAccess403) => break Err(Errors::NoAccess403),
                    Ok(bpr) => {
                        if is_selected("disable_force_push")
                            && !(bpr
                                .allow_force_pushes
                                .as_ref()
                                .map(|f| f.enabled)
                                .unwrap_or(true))
                        {
                            checks.disable_force_push = Check::Pass;
                        }
                        if is_selected("disable_deletion")
                            && !(bpr
                                .allow_deletions
                                .as_ref()
                                .map(|f| f.enabled)
                                .unwrap_or(true))
                        {
                            checks.disable_deletion = Check::Pass;
                        }
                        if is_selected("require_signed_commits")
                            && bpr
                                .required_signatures
                                .as_ref()
                                .map(|f| f.enabled)
                                .unwrap_or(false)
                        {
                            checks.require_signed_commits = Check::Pass;
                        }
                        if let Some(pr) = &bpr.required_pull_request_reviews {
                            if is_selected("pr_one_approval")
                                && pr.required_approving_review_count > 0
                            {
                                checks.pr_one_approval = Check::Pass
                            }
                            if is_selected("pr_dismiss_stale") && pr.dismiss_stale_reviews {
                                checks.pr_dismiss_stale = Check::Pass;
                            }
                            if is_selected("pr_require_code_owner") && pr.require_code_owner_reviews
                            {
                                checks.pr_require_code_owner = Check::Pass;
                            }
                        }
                        if is_selected("require_status_checks") {
                            if let Some(rsc) = &bpr.required_status_checks {
                                if !(rsc.checks.is_empty()) {
                                    checks.require_status_checks = Check::Pass;
                                }
                            }
                        }
                        break Ok(());
                    }
                }
            };

            // 2) New Rulesets with retries on transient errors
            let mut attempts_r = 0;
            let rules_fetch = loop {
                attempts_r += 1;
                match rules::get_rules(&bootstrap, &repo, &default_branch) {
                    Err(Errors::MissingOrError) => {
                        if attempts_r < max_attempts {
                            thread::sleep(Duration::from_millis(500));
                            continue;
                        } else {
                            break Err(Errors::MissingOrError);
                        }
                    }
                    Err(Errors::NoAccess403) => break Err(Errors::NoAccess403),
                    Ok(rules) => {
                        for rule in rules {
                            match rule.type_.as_str() {
                                // The presence of this rule means deletion is disabled
                                "deletion" => {
                                    if is_selected("disable_deletion") {
                                        checks.disable_deletion = Check::Pass;
                                    }
                                }
                                // The presence of this rule means signed commits are required
                                "required_signatures" => {
                                    if is_selected("require_signed_commits") {
                                        checks.require_signed_commits = Check::Pass;
                                    }
                                }
                                // The presence of this rule means force push is disabled
                                "non_fast_forward" => {
                                    if is_selected("disable_force_push") {
                                        checks.disable_force_push = Check::Pass;
                                    }
                                }
                                // The presence of this rule means a PR is needed. Now we check the rule's params
                                "pull_request" => {
                                    if let Some(params) = rule.parameters.as_ref() {
                                        if is_selected("pr_one_approval")
                                            && params
                                                .get("required_approving_review_count")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0)
                                                > 0
                                        {
                                            checks.pr_one_approval = Check::Pass;
                                        }
                                        if is_selected("pr_dismiss_stale")
                                            && params
                                                .get("dismiss_stale_reviews_on_push")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false)
                                        {
                                            checks.pr_dismiss_stale = Check::Pass;
                                        }
                                        if is_selected("pr_require_code_owner")
                                            && params
                                                .get("require_code_owner_review")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false)
                                        {
                                            checks.pr_require_code_owner = Check::Pass;
                                        }
                                    }
                                }
                                "required_status_checks" => {
                                    if is_selected("require_status_checks") {
                                        if let Some(params) = rule.parameters.as_ref() {
                                            if params
                                                .get("required_status_checks")
                                                .and_then(|v| v.as_array())
                                                .map(|a| !a.is_empty())
                                                .unwrap_or(false)
                                            {
                                                checks.require_status_checks = Check::Pass;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        break Ok(());
                    }
                }
            };

            // Propagate 403 status for checks unresolved by either source
            if Err(Errors::NoAccess403) == bpr_fetch || Err(Errors::NoAccess403) == rules_fetch {
                let mark_na = |c: &mut Check| {
                    if *c != Check::Pass {
                        *c = Check::NoAccess
                    }
                };
                mark_na(&mut checks.pr_one_approval);
                mark_na(&mut checks.pr_dismiss_stale);
                mark_na(&mut checks.pr_require_code_owner);
                mark_na(&mut checks.disable_force_push);
                mark_na(&mut checks.disable_deletion);
                mark_na(&mut checks.require_signed_commits);
                mark_na(&mut checks.require_status_checks);
            }
        }

        // 3) CODEOWNERS exists and is valid
        if is_selected("codeowners_valid") {
            match utils::codeowners_exists_and_is_valid(&bootstrap, &repo) {
                Ok(CodeownersStatus::Valid) => checks.codeowners_valid = Check::Pass,
                Ok(CodeownersStatus::Invalid) | Ok(CodeownersStatus::Missing) => {}
                Err(Errors::NoAccess403) => checks.codeowners_valid = Check::NoAccess,
                Err(Errors::MissingOrError) => {}
            }
        }

        // Determine repository visibility (reuse from repo metadata if present)
        let visibility = visibility_opt.unwrap_or_else(|| "unknown".to_string());

        // If CSV export is enabled, write a row; otherwise, print report
        if let Some(wtr) = csv_writer.as_mut() {
            let co_path = if is_selected("codeowners_valid") {
                utils::find_codeowners_path(&bootstrap, &repo).unwrap_or_else(|| "".to_string())
            } else {
                "".to_string()
            };
            wtr.write_record([
                repo.as_str(),
                default_branch.as_str(),
                visibility.as_str(),
                check_csv_value_named(
                    checks.pr_one_approval,
                    "pr_one_approval",
                    selected_set.as_ref(),
                )
                .as_str(),
                check_csv_value_named(
                    checks.pr_dismiss_stale,
                    "pr_dismiss_stale",
                    selected_set.as_ref(),
                )
                .as_str(),
                check_csv_value_named(
                    checks.pr_require_code_owner,
                    "pr_require_code_owner",
                    selected_set.as_ref(),
                )
                .as_str(),
                check_csv_value_named(
                    checks.disable_force_push,
                    "disable_force_push",
                    selected_set.as_ref(),
                )
                .as_str(),
                check_csv_value_named(
                    checks.disable_deletion,
                    "disable_deletion",
                    selected_set.as_ref(),
                )
                .as_str(),
                check_csv_value_named(
                    checks.require_signed_commits,
                    "require_signed_commits",
                    selected_set.as_ref(),
                )
                .as_str(),
                check_csv_value_named(
                    checks.require_status_checks,
                    "require_status_checks",
                    selected_set.as_ref(),
                )
                .as_str(),
                check_csv_value_named(
                    checks.codeowners_valid,
                    "codeowners_valid",
                    selected_set.as_ref(),
                )
                .as_str(),
                co_path.as_str(),
            ])
            .expect("Unable to write CSV row");
            // Flush after every write to ensure durability on long runs
            wtr.flush().ok();
        } else {
            print_report(
                &repo,
                &default_branch,
                &visibility,
                checks,
                selected_set.as_ref(),
            );
        }
        pb.inc(1);
    }

    // Flush CSV if used
    if let Some(mut wtr) = csv_writer {
        wtr.flush().expect("Unable to flush CSV writer");
    }
    pb.finish_with_message("done");
}

fn print_report(
    repo: &str,
    branch: &str,
    visibility: &str,
    checks: ProtectionChecks,
    selected: Option<&HashSet<String>>,
) {
    let (score, max) = compute_selected_score(&checks, selected);
    println!(
        "{} {}  {} {}  {} {}  {} {}/{}",
        "Repo:".yellow(),
        repo.white(),
        "Default branch:".yellow(),
        branch.white(),
        "Visibility:".yellow(),
        visibility.white(),
        "Score:".yellow(),
        score.to_string().white(),
        max.to_string().white()
    );
    let show = |name: &str, sel: Option<&HashSet<String>>| -> bool {
        match sel {
            None => true,
            Some(s) => s.contains(&name.to_lowercase()),
        }
    };

    if show("pr_one_approval", selected) {
        println!(
            "  - PR requires one approval: {}",
            check_symbol(checks.pr_one_approval)
        );
    }
    if show("pr_dismiss_stale", selected) {
        println!(
            "  - PR: dismiss stale reviews: {}",
            check_symbol(checks.pr_dismiss_stale)
        );
    }
    if show("pr_require_code_owner", selected) {
        println!(
            "  - PR requires code owners approval: {}",
            check_symbol(checks.pr_require_code_owner)
        );
    }
    if show("disable_force_push", selected) {
        println!(
            "  - Force-push disabled: {}",
            check_symbol(checks.disable_force_push)
        );
    }
    if show("disable_deletion", selected) {
        println!(
            "  - Deletion disabled: {}",
            check_symbol(checks.disable_deletion)
        );
    }
    if show("require_signed_commits", selected) {
        println!(
            "  - Require signed commits: {}",
            check_symbol(checks.require_signed_commits)
        );
    }
    if show("require_status_checks", selected) {
        println!(
            "  - Require status checks: {}",
            check_symbol(checks.require_status_checks)
        );
    }
    if show("codeowners_valid", selected) {
        println!(
            "  - CODEOWNERS exists and is valid: {}",
            check_symbol(checks.codeowners_valid)
        );
    }
}

fn compute_selected_score(
    checks: &ProtectionChecks,
    selected: Option<&HashSet<String>>,
) -> (u32, u32) {
    let weights = Weights::default();
    let mut items: Vec<(&Check, u32)> = Vec::new();
    let include = |name: &str, sel: Option<&HashSet<String>>| -> bool {
        match sel {
            None => true,
            Some(s) => s.contains(&name.to_lowercase()),
        }
    };
    if include("pr_one_approval", selected) {
        items.push((&checks.pr_one_approval, weights.pr_one_approval));
    }
    if include("pr_dismiss_stale", selected) {
        items.push((&checks.pr_dismiss_stale, weights.pr_dismiss_stale));
    }
    if include("pr_require_code_owner", selected) {
        items.push((&checks.pr_require_code_owner, weights.pr_require_code_owner));
    }
    if include("disable_force_push", selected) {
        items.push((&checks.disable_force_push, weights.disable_force_push));
    }
    if include("disable_deletion", selected) {
        items.push((&checks.disable_deletion, weights.disable_deletion));
    }
    if include("require_signed_commits", selected) {
        items.push((
            &checks.require_signed_commits,
            weights.require_signed_commits,
        ));
    }
    if include("require_status_checks", selected) {
        items.push((&checks.require_status_checks, weights.require_status_checks));
    }
    if include("codeowners_valid", selected) {
        items.push((&checks.codeowners_valid, weights.codeowners_valid));
    }

    let max: u32 = items.iter().map(|(_, w)| *w).sum();
    let score: u32 = items
        .iter()
        .map(|(p, w)| if **p == Check::Pass { *w } else { 0 })
        .sum();
    (score, max)
}

fn check_symbol(v: Check) -> String {
    match v {
        Check::NoAccess => "? (403)".to_string(),
        Check::Pass => "✅".to_string(),
        _ => "❌".to_string(),
    }
}

fn check_csv_value(v: Check) -> String {
    match v {
        Check::NoAccess => "403".to_string(),
        Check::Pass => "pass".to_string(),
        _ => "fail".to_string(),
    }
}

fn check_csv_value_named(v: Check, name: &str, selected: Option<&HashSet<String>>) -> String {
    match selected {
        None => check_csv_value(v),
        Some(s) => {
            if !s.contains(&name.to_lowercase()) {
                return "n/a".to_string();
            }
            check_csv_value(v)
        }
    }
}

fn get_default_branch(bootstrap: &Bootstrap, repo: &str) -> Result<RepoInfo, Errors> {
    // Try repository metadata first
    match make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => {
            if res.get("status").and_then(|v| v.as_str()) == Some("403") {
                return Err(Errors::NoAccess403);
            }
            if let Some(branch) = res.get("default_branch").and_then(|v| v.as_str()) {
                let visibility = res
                    .get("visibility")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                return Ok(RepoInfo {
                    branch: branch.to_string(),
                    visibility,
                });
            }
        }
        Err(_) => {}
    }

    Err(Errors::MissingOrError)
}

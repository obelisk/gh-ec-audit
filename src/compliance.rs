use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::Path;
use std::{thread, time::Duration};

use crate::{make_github_request, Bootstrap};

#[derive(Default, Debug, Clone, Copy)]
/// Result of a single compliance check.
/// - pass: the check is satisfied
/// - no_access_403: unable to evaluate due to GitHub returning 403 for the resource
struct Check {
    /// Whether the check is satisfied
    pass: bool,
    /// Whether the check could not be evaluated due to lack of access (HTTP 403)
    no_access_403: bool,
}

#[derive(Default, Debug, Clone, Copy)]
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

impl ProtectionChecks {
    fn score(&self) -> u32 {
        let weights = Weights::default();
        let mut s: u32 = 0;
        if self.pr_one_approval.pass {
            s += weights.pr_one_approval
        }
        if self.pr_dismiss_stale.pass {
            s += weights.pr_dismiss_stale
        }
        if self.pr_require_code_owner.pass {
            s += weights.pr_require_code_owner
        }
        if self.disable_force_push.pass {
            s += weights.disable_force_push
        }
        if self.disable_deletion.pass {
            s += weights.disable_deletion
        }
        if self.require_signed_commits.pass {
            s += weights.require_signed_commits
        }
        if self.require_status_checks.pass {
            s += weights.require_status_checks
        }
        if self.codeowners_valid.pass {
            s += weights.codeowners_valid
        }
        s
    }

    fn max_score() -> u32 {
        let w = Weights::default();
        w.pr_one_approval
            + w.pr_dismiss_stale
            + w.pr_require_code_owner
            + w.disable_force_push
            + w.disable_deletion
            + w.require_signed_commits
            + w.require_status_checks
            + w.codeowners_valid
    }
}

#[derive(Clone, Copy)]
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

// No RepoInfo struct needed; we read default_branch directly from the JSON

#[derive(Debug, Deserialize)]
struct BprResponse {
    allow_force_pushes: Option<EnabledFlag>,
    allow_deletions: Option<EnabledFlag>,
    required_signatures: Option<EnabledFlag>,
    required_status_checks: Option<RequiredStatusChecks>,
    required_pull_request_reviews: Option<PullRequestReviews>,
}

#[derive(Debug, Deserialize)]
struct EnabledFlag {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct RequiredStatusChecks {
    checks: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct PullRequestReviews {
    required_approving_review_count: u32,
    dismiss_stale_reviews: bool,
    require_code_owner_reviews: bool,
}

#[derive(Debug, Deserialize)]
struct RulesetRule {
    #[serde(rename = "type")]
    _type: String,
    parameters: Option<serde_json::Value>,
}

pub fn run_compliance_audit(
    bootstrap: Bootstrap,
    repos: Option<Vec<String>>,
    csv_path: Option<String>,
    active_repo_only: bool,
) {
    let mut repos = repos.unwrap_or_else(|| {
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
                already_processed = read_existing_csv_repos(path);
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
    if csv_writer.is_some() && !already_processed.is_empty() {
        repos = repos
            .into_iter()
            .filter(|r| !already_processed.contains(r))
            .collect::<Vec<String>>();
    }

    // Progress bar
    let pb = ProgressBar::new(repos.len() as u64);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );

    for repo in repos {
        pb.set_message(format!("{}", repo));

        // Retry default branch resolution to be resilient to transient failures
        let max_attempts = 3;
        let mut attempt = 0;
        let mut default_branch_opt: Option<String> = None;
        let mut visibility_opt: Option<String> = None;
        loop {
            attempt += 1;
            match get_default_branch(&bootstrap, &repo) {
                DefaultBranchFetch::Ok(info) => {
                    default_branch_opt = Some(info.branch);
                    visibility_opt = info.visibility;
                    break;
                }
                DefaultBranchFetch::NoAccess403 => {
                    println!(
                        "{} {}: {}",
                        "Skipping repo".yellow(),
                        repo.white(),
                        "default branch not accessible (403)".red()
                    );
                    break;
                }
                DefaultBranchFetch::MissingOrError => {
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
                        break;
                    }
                }
            }
        }
        let Some(default_branch) = default_branch_opt else {
            pb.inc(1);
            continue;
        };
        let mut checks = ProtectionChecks::default();
        let mut saw_403_bpr = false;
        let mut saw_403_rules = false;

        // 1) Classic BPR with retries on transient errors
        let mut bpr_fetch;
        let mut attempts = 0;
        loop {
            attempts += 1;
            bpr_fetch = get_bpr(&bootstrap, &repo, &default_branch);
            match bpr_fetch {
                BprFetch::MissingOrError if attempts < max_attempts => {
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
                _ => break,
            }
        }
        match bpr_fetch {
            BprFetch::Ok(bpr) => {
                checks.disable_force_push.pass = !bpr
                    .allow_force_pushes
                    .as_ref()
                    .map(|f| f.enabled)
                    .unwrap_or(false);
                checks.disable_deletion.pass = !bpr
                    .allow_deletions
                    .as_ref()
                    .map(|f| f.enabled)
                    .unwrap_or(false);
                checks.require_signed_commits.pass = bpr
                    .required_signatures
                    .as_ref()
                    .map(|f| f.enabled)
                    .unwrap_or(false);
                if let Some(pr) = &bpr.required_pull_request_reviews {
                    checks.pr_one_approval.pass = pr.required_approving_review_count > 0;
                    checks.pr_dismiss_stale.pass = pr.dismiss_stale_reviews;
                    checks.pr_require_code_owner.pass = pr.require_code_owner_reviews;
                }
                if let Some(rsc) = &bpr.required_status_checks {
                    checks.require_status_checks.pass = !rsc.checks.is_empty();
                }
            }
            BprFetch::NoAccess403 => {
                saw_403_bpr = true;
            }
            BprFetch::MissingOrError => {}
        }

        // 2) New Rulesets with retries on transient errors
        let mut rules_fetch;
        let mut attempts_r = 0;
        loop {
            attempts_r += 1;
            rules_fetch = get_rules(&bootstrap, &repo, &default_branch);
            match rules_fetch {
                RulesFetch::MissingOrError if attempts_r < max_attempts => {
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
                _ => break,
            }
        }
        match rules_fetch {
            RulesFetch::Ok(rules) => {
                for rule in rules {
                    match rule._type.as_str() {
                        "deletion" => checks.disable_deletion.pass = true,
                        "required_signatures" => checks.require_signed_commits.pass = true,
                        "non_fast_forward" => checks.disable_force_push.pass = true,
                        "pull_request" => {
                            if let Some(params) = rule.parameters.as_ref() {
                                if params
                                    .get("required_approving_review_count")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    > 0
                                {
                                    checks.pr_one_approval.pass = true;
                                }
                                if params
                                    .get("dismiss_stale_reviews_on_push")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false)
                                {
                                    checks.pr_dismiss_stale.pass = true;
                                }
                                if params
                                    .get("require_code_owner_review")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false)
                                {
                                    checks.pr_require_code_owner.pass = true;
                                }
                            }
                        }
                        "required_status_checks" => {
                            if let Some(params) = rule.parameters.as_ref() {
                                if params
                                    .get("required_status_checks")
                                    .and_then(|v| v.as_array())
                                    .map(|a| !a.is_empty())
                                    .unwrap_or(false)
                                {
                                    checks.require_status_checks.pass = true;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            RulesFetch::NoAccess403 => {
                saw_403_rules = true;
            }
            RulesFetch::MissingOrError => {}
        }

        // 3) CODEOWNERS exists and is valid
        match codeowners_exists_and_is_valid(&bootstrap, &repo) {
            CodeownersCheck::Valid => checks.codeowners_valid.pass = true,
            CodeownersCheck::Invalid | CodeownersCheck::Missing => {}
            CodeownersCheck::NoAccess403 => checks.codeowners_valid.no_access_403 = true,
            CodeownersCheck::Error => {}
        }

        // Propagate 403 status for checks unresolved by either source
        if saw_403_bpr || saw_403_rules {
            let mark_na = |c: &mut Check| {
                if !c.pass {
                    c.no_access_403 = true;
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

        // Determine repository visibility (reuse from repo metadata if present)
        let visibility = visibility_opt.unwrap_or_else(|| "unknown".to_string());

        // If CSV export is enabled, write a row; otherwise, print report
        if let Some(wtr) = csv_writer.as_mut() {
            let co_path = find_codeowners_path(&bootstrap, &repo).unwrap_or_else(|| "".to_string());
            wtr.write_record([
                repo.as_str(),
                default_branch.as_str(),
                visibility.as_str(),
                check_csv_value(checks.pr_one_approval).as_str(),
                check_csv_value(checks.pr_dismiss_stale).as_str(),
                check_csv_value(checks.pr_require_code_owner).as_str(),
                check_csv_value(checks.disable_force_push).as_str(),
                check_csv_value(checks.disable_deletion).as_str(),
                check_csv_value(checks.require_signed_commits).as_str(),
                check_csv_value(checks.require_status_checks).as_str(),
                check_csv_value(checks.codeowners_valid).as_str(),
                co_path.as_str(),
            ])
            .expect("Unable to write CSV row");
            // Flush after every write to ensure durability on long runs
            wtr.flush().ok();
        } else {
            print_report(&repo, &default_branch, &visibility, checks);
        }
        pb.inc(1);
    }

    // Flush CSV if used
    if let Some(mut wtr) = csv_writer {
        wtr.flush().expect("Unable to flush CSV writer");
    }
    pb.finish_with_message("done");
}

fn print_report(repo: &str, branch: &str, visibility: &str, checks: ProtectionChecks) {
    let max = ProtectionChecks::max_score();
    println!(
        "{} {}  {} {}  {} {}  {} {}/{}",
        "Repo:".yellow(),
        repo.white(),
        "Default branch:".yellow(),
        branch.white(),
        "Visibility:".yellow(),
        visibility.white(),
        "Score:".yellow(),
        checks.score().to_string().white(),
        max.to_string().white()
    );
    println!(
        "  - PR requires one approval: {}\n  - PR: dismiss stale reviews: {}\n  - PR requires code owners approval: {}\n  - Force-push disabled: {}\n  - Deletion disabled: {}\n  - Require signed commits: {}\n  - Require status checks: {}\n  - CODEOWNERS exists and is valid: {}\n",
        check_symbol(checks.pr_one_approval),
        check_symbol(checks.pr_dismiss_stale),
        check_symbol(checks.pr_require_code_owner),
        check_symbol(checks.disable_force_push),
        check_symbol(checks.disable_deletion),
        check_symbol(checks.require_signed_commits),
        check_symbol(checks.require_status_checks),
        check_symbol(checks.codeowners_valid)
    );
}

fn check_symbol(v: Check) -> String {
    if v.no_access_403 {
        return "? (403)".to_string();
    }
    if v.pass {
        return "✅".to_string();
    }
    "❌".to_string()
}

fn check_csv_value(v: Check) -> String {
    if v.no_access_403 {
        return "403".to_string();
    }
    if v.pass {
        return "pass".to_string();
    }
    "fail".to_string()
}

struct RepoInfo {
    branch: String,
    visibility: Option<String>,
}

enum DefaultBranchFetch {
    Ok(RepoInfo),
    NoAccess403,
    MissingOrError,
}

fn get_default_branch(bootstrap: &Bootstrap, repo: &str) -> DefaultBranchFetch {
    // Try repository metadata first
    match make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => {
            if res.get("status").and_then(|v| v.as_str()) == Some("403") {
                return DefaultBranchFetch::NoAccess403;
            }
            if let Some(branch) = res.get("default_branch").and_then(|v| v.as_str()) {
                let visibility = res.get("visibility").and_then(|v| v.as_str()).map(|s| s.to_string());
                return DefaultBranchFetch::Ok(RepoInfo {
                    branch: branch.to_string(),
                    visibility,
                });
            }
        }
        Err(_) => {}
    }

    DefaultBranchFetch::MissingOrError
}

 

enum BprFetch {
    Ok(BprResponse),
    NoAccess403,
    MissingOrError,
}

fn get_bpr(bootstrap: &Bootstrap, repo: &str, branch: &str) -> BprFetch {
    match make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}/branches/{branch}/protection", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => {
            if res.get("status").and_then(|v| v.as_str()) == Some("404") {
                return BprFetch::MissingOrError;
            }
            if res.get("status").and_then(|v| v.as_str()) == Some("403")
                || res
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(|m| m.contains("Resource not accessible"))
                    .unwrap_or(false)
            {
                return BprFetch::NoAccess403;
            }
            match serde_json::from_value::<BprResponse>(res) {
                Ok(v) => BprFetch::Ok(v),
                Err(_) => BprFetch::MissingOrError,
            }
        }
        Err(_) => BprFetch::MissingOrError,
    }
}

enum RulesFetch {
    Ok(Vec<RulesetRule>),
    NoAccess403,
    MissingOrError,
}

fn get_rules(
    bootstrap: &Bootstrap,
    repo: &str,
    branch: &str,
) -> RulesFetch {
    match make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}/rules/branches/{branch}", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => {
            if res.get("status").and_then(|v| v.as_str()) == Some("403") {
                return RulesFetch::NoAccess403;
            }
            match serde_json::from_value::<Vec<RulesetRule>>(res) {
                Ok(v) => RulesFetch::Ok(v),
                Err(_) => RulesFetch::MissingOrError,
            }
        }
        Err(_) => RulesFetch::MissingOrError,
    }
}

enum CodeownersCheck {
    Valid,
    Invalid,
    Missing,
    NoAccess403,
    Error,
}

// Check CODEOWNERS state via GitHub API
fn codeowners_exists_and_is_valid(bootstrap: &Bootstrap, repo: &str) -> CodeownersCheck {
    let url = format!("/repos/{}/{repo}/codeowners/errors", bootstrap.org);
    match make_github_request(&bootstrap.token, &url, 3, None) {
        Ok(res) => {
            if res.get("status").and_then(|v| v.as_str()) == Some("403") {
                return CodeownersCheck::NoAccess403;
            }
            match res.get("errors") {
                None => CodeownersCheck::Missing,
                Some(errors) => match errors.as_array() {
                    Some(arr) if arr.is_empty() => CodeownersCheck::Valid,
                    Some(_) => CodeownersCheck::Invalid,
                    None => CodeownersCheck::Error,
                },
            }
        }
        Err(_) => CodeownersCheck::Error,
    }
}

// (removed) get_repo_visibility in favor of reusing repository metadata

// Try the common CODEOWNERS locations and return the repository-relative path if found
fn find_codeowners_path(bootstrap: &Bootstrap, repo: &str) -> Option<String> {
    const CO_LOCATIONS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];
    for location in CO_LOCATIONS {
        let url = format!("/repos/{}/{}/contents/{}", bootstrap.org, repo, location);
        match make_github_request(&bootstrap.token, &url, 2, None) {
            Ok(v) => {
                if v.get("status").and_then(|s| s.as_str()) == Some("404") {
                    continue;
                }
                // If GitHub returned an object for this path, it exists
                if v.get("path").and_then(|p| p.as_str()).is_some() {
                    return Some(location.to_string());
                }
            }
            Err(_) => continue,
        }
    }
    None
}

fn read_existing_csv_repos(path: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let rdr = csv::Reader::from_path(path);
    let mut rdr = match rdr {
        Ok(r) => r,
        Err(_) => return set,
    };
    let headers = match rdr.headers() {
        Ok(h) => h.clone(),
        Err(_) => return set,
    };
    let repo_idx = headers
        .iter()
        .position(|h| h == "repository")
        .unwrap_or(0);
    for result in rdr.records() {
        if let Ok(record) = result {
            if let Some(repo) = record.get(repo_idx) {
                set.insert(repo.to_string());
            }
        }
    }
    set
}



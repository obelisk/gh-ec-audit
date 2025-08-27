use colored::Colorize;
use serde::Deserialize;

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

pub fn run_compliance_audit(bootstrap: Bootstrap, repos: Option<Vec<String>>) {
    let repos = repos.unwrap_or_else(|| {
        bootstrap
            .fetch_all_repositories(75)
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<String>>()
    });

    for repo in repos {
        let Some(default_branch) = get_default_branch(&bootstrap, &repo) else {
            println!(
                "{} {}: {}",
                "Skipping repo".yellow(),
                repo.white(),
                "could not determine default branch".red()
            );
            continue;
        };
        let mut checks = ProtectionChecks::default();
        let mut saw_403_bpr = false;
        let mut saw_403_rules = false;

        // 1) Classic BPR
        match get_bpr(&bootstrap, &repo, &default_branch) {
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

        // 2) New Rulesets
        match get_rules(&bootstrap, &repo, &default_branch) {
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

        print_report(&repo, &default_branch, checks);
    }
}

fn print_report(repo: &str, branch: &str, checks: ProtectionChecks) {
    let max = ProtectionChecks::max_score();
    println!(
        "{} {}  {} {}  {} {}/{}",
        "Repo:".yellow(),
        repo.white(),
        "Default branch:".yellow(),
        branch.white(),
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
        return "? (403)".yellow().to_string();
    }
    if v.pass {
        return "✅".green().to_string();
    }
    "❌".red().to_string()
}

fn get_default_branch(bootstrap: &Bootstrap, repo: &str) -> Option<String> {
    // Try repository metadata first
    if let Ok(res) = make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}", bootstrap.org),
        3,
        None,
    ) {
        if let Some(branch) = res.get("default_branch").and_then(|v| v.as_str()) {
            return Some(branch.to_string());
        }
    }

    // Fallbacks: probe common default branch names
    if branch_exists(bootstrap, repo, "main") {
        return Some("main".to_string());
    }
    if branch_exists(bootstrap, repo, "master") {
        return Some("master".to_string());
    }
    None
}

fn branch_exists(bootstrap: &Bootstrap, repo: &str, branch: &str) -> bool {
    match make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}/branches/{branch}", bootstrap.org),
        2,
        None,
    ) {
        Ok(res) => {
            // If GitHub returns an error payload, it often has a string "status" like "404"
            if res.get("status").and_then(|v| v.as_str()) == Some("404") {
                return false;
            }
            // Presence of a branch name indicates success
            res.get("name").and_then(|v| v.as_str()).is_some()
        }
        Err(_) => false,
    }
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



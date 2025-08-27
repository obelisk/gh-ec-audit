use colored::Colorize;
use serde::Deserialize;

use crate::{make_github_request, Bootstrap};

#[derive(Default, Debug, Clone, Copy)]
struct ProtectionChecks {
    pr_one_approval: bool,
    pr_dismiss_stale: bool,
    pr_require_code_owner: bool,
    disable_force_push: bool,
    disable_deletion: bool,
    require_signed_commits: bool,
    require_status_checks: bool,
    codeowners_valid: bool,
}

impl ProtectionChecks {
    fn score(&self) -> u8 {
        let mut s = 0u8;
        if self.pr_one_approval {
            s += 1
        }
        if self.pr_dismiss_stale {
            s += 1
        }
        if self.pr_require_code_owner {
            s += 1
        }
        if self.disable_force_push {
            s += 1
        }
        if self.disable_deletion {
            s += 1
        }
        if self.require_signed_commits {
            s += 1
        }
        if self.require_status_checks {
            s += 1
        }
        if self.codeowners_valid {
            s += 1
        }
        s
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

        // 1) Classic BPR
        if let Some(bpr) = get_bpr(&bootstrap, &repo, &default_branch) {
            checks.disable_force_push = !bpr
                .allow_force_pushes
                .as_ref()
                .map(|f| f.enabled)
                .unwrap_or(false);
            checks.disable_deletion = !bpr
                .allow_deletions
                .as_ref()
                .map(|f| f.enabled)
                .unwrap_or(false);
            checks.require_signed_commits = bpr
                .required_signatures
                .as_ref()
                .map(|f| f.enabled)
                .unwrap_or(false);
            if let Some(pr) = &bpr.required_pull_request_reviews {
                checks.pr_one_approval = pr.required_approving_review_count > 0;
                checks.pr_dismiss_stale = pr.dismiss_stale_reviews;
                checks.pr_require_code_owner = pr.require_code_owner_reviews;
            }
            if let Some(rsc) = &bpr.required_status_checks {
                checks.require_status_checks = !rsc.checks.is_empty();
            }
        }

        // Early win
        if checks.score() == 7 {
            print_report(&repo, &default_branch, checks);
            continue;
        }

        // 2) New Rulesets
        if let Some(rules) = get_rules(&bootstrap, &repo, &default_branch) {
            for rule in rules {
                match rule._type.as_str() {
                    "deletion" => checks.disable_deletion = true,
                    "required_signatures" => checks.require_signed_commits = true,
                    "non_fast_forward" => checks.disable_force_push = true,
                    "pull_request" => {
                        if let Some(params) = rule.parameters.as_ref() {
                            if params
                                .get("required_approving_review_count")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                                > 0
                            {
                                checks.pr_one_approval = true;
                            }
                            if params
                                .get("dismiss_stale_reviews_on_push")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                            {
                                checks.pr_dismiss_stale = true;
                            }
                            if params
                                .get("require_code_owner_review")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                            {
                                checks.pr_require_code_owner = true;
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
                                checks.require_status_checks = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // 3) CODEOWNERS exists and is valid
        checks.codeowners_valid = codeowners_exists_and_is_valid(&bootstrap, &repo).unwrap_or(false);

        print_report(&repo, &default_branch, checks);
    }
}

fn print_report(repo: &str, branch: &str, checks: ProtectionChecks) {
    println!(
        "{} {}  {} {}  {} {}",
        "Repo:".yellow(),
        repo.white(),
        "Default branch:".yellow(),
        branch.white(),
        "Score (0-8):".yellow(),
        checks.score().to_string().white()
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

fn check_symbol(v: bool) -> String {
    if v {
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

fn get_bpr(bootstrap: &Bootstrap, repo: &str, branch: &str) -> Option<BprResponse> {
    match make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}/branches/{branch}/protection", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => {
            if res.get("status").is_some() && res.get("status").unwrap() == "404" {
                return None;
            }
            serde_json::from_value::<BprResponse>(res).ok()
        }
        Err(_) => None,
    }
}

fn get_rules(
    bootstrap: &Bootstrap,
    repo: &str,
    branch: &str,
) -> Option<Vec<RulesetRule>> {
    match make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}/rules/branches/{branch}", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => serde_json::from_value::<Vec<RulesetRule>>(res).ok(),
        Err(_) => None,
    }
}

// Return Ok(true) if CODEOWNERS exists and has zero errors; Ok(false) if present but invalid; Ok(false) if missing; Err on API error
fn codeowners_exists_and_is_valid(bootstrap: &Bootstrap, repo: &str) -> Result<bool, String> {
    let url = format!("/repos/{}/{repo}/codeowners/errors", bootstrap.org);
    let res = make_github_request(&bootstrap.token, &url, 3, None)?;
    match res.get("errors") {
        None => Ok(false), // missing CODEOWNERS
        Some(errors) => {
            let arr = errors
                .as_array()
                .ok_or_else(|| "Unexpected response format for codeowners errors".to_string())?;
            Ok(arr.is_empty())
        }
    }
}



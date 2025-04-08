use std::fmt::Display;

use colored::Colorize;

use crate::{make_github_request, Bootstrap};

fn get_default_branch(bootstrap: &Bootstrap, repo: impl Display) -> String {
    match make_github_request(
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

fn get_bprs(bootstrap: &Bootstrap, repo: impl Display, branch: impl Display) -> String {
    match make_github_request(
        &bootstrap.token,
        &format!(
            "/repos/{}/{repo}/branches/{branch}/protection",
            bootstrap.org
        ),
        3,
        None,
    ) {
        Ok(res) => {
            if res.get("status").is_some() && res.get("status").unwrap() == "404" {
                return "Empty".to_string();
            }
            serde_json::to_string_pretty(&res).unwrap()
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

fn get_rulesets(bootstrap: &Bootstrap, repo: impl Display, branch: impl Display) -> String {
    match make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}/rules/branches/{branch}", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => serde_json::to_string_pretty(&res).unwrap(),
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

pub fn run_audit(bootstrap: Bootstrap, repos: Option<Vec<String>>) {
    let repos = repos.unwrap_or_else(|| {
        bootstrap
            .fetch_all_repositories(75)
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<String>>()
    });

    for repo in repos {
        let default_branch = get_default_branch(&bootstrap, &repo);
        println!(
            "{} {}\n{} {}\n",
            "          Repo:".yellow(),
            repo.white(),
            "Default branch:".yellow(),
            default_branch.white()
        );

        let bprs = get_bprs(&bootstrap, &repo, &default_branch);
        println!("{} {bprs}\n", "          BPRs:".yellow());

        let rulesets = get_rulesets(&bootstrap, &repo, &default_branch);
        println!("{} {rulesets}\n\n", "      Rulesets:".yellow());
    }
}

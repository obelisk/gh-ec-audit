use std::{thread, time::Duration};

use crate::{
    compliance::{Errors, RepoInfo},
    GitHubClient,
};

pub fn get_default_branch(
    client: &GitHubClient,
    repo: &str,
    max_attempts: u32,
) -> Result<RepoInfo, Errors> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match client.get_json(&format!("/repos/{}/{repo}", client.org()), None) {
            Ok(res) => {
                if res.get("status").and_then(|v| v.as_str()) == Some("403") {
                    // We got a 403 so we return because retrying would not make sense
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
                // The response was a success but did not contain a default
                // branch (e.g. a 404 body for a deleted/renamed repo):
                // retrying would not help, so give up instead of looping.
                return Err(Errors::MissingOrError);
            }
            Err(_) => {
                // This could be a transient error. See if we should try again
                if attempts < max_attempts {
                    thread::sleep(Duration::from_millis(500));
                    continue;
                } else {
                    return Err(Errors::MissingOrError);
                }
            }
        }
    }
}

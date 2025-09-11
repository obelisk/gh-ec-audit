use std::{thread, time::Duration};

use crate::{
    compliance::{Errors, RepoInfo},
    make_github_request, Bootstrap,
};

pub fn get_default_branch(
    bootstrap: &Bootstrap,
    repo: &str,
    max_attempts: u32,
) -> Result<RepoInfo, Errors> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match make_github_request(
            &bootstrap.token,
            &format!("/repos/{}/{repo}", bootstrap.org),
            3,
            None,
        ) {
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

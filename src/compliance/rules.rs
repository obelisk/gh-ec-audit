use crate::{
    compliance::{BprResponse, Errors, RulesetRule},
    make_github_request, Bootstrap,
};

/// Get Branch Protection Rules for a repo's given branch
pub fn get_bpr(bootstrap: &Bootstrap, repo: &str, branch: &str) -> Result<BprResponse, Errors> {
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
            if res.get("status").and_then(|v| v.as_str()) == Some("404") {
                return Err(Errors::MissingOrError);
            }
            if res.get("status").and_then(|v| v.as_str()) == Some("403")
                || res
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(|m| m.contains("Resource not accessible"))
                    .unwrap_or(false)
            {
                return Err(Errors::NoAccess403);
            }
            serde_json::from_value::<BprResponse>(res).map_err(|_| Errors::MissingOrError)
        }
        Err(_) => Err(Errors::MissingOrError),
    }
}

/// Get Rulesets for a repo's given branch
pub fn get_rules(
    bootstrap: &Bootstrap,
    repo: &str,
    branch: &str,
) -> Result<Vec<RulesetRule>, Errors> {
    match make_github_request(
        &bootstrap.token,
        &format!("/repos/{}/{repo}/rules/branches/{branch}", bootstrap.org),
        3,
        None,
    ) {
        Ok(res) => {
            if res.get("status").and_then(|v| v.as_str()) == Some("403") {
                return Err(Errors::NoAccess403);
            }
            serde_json::from_value::<Vec<RulesetRule>>(res).map_err(|_| Errors::MissingOrError)
        }
        Err(_) => Err(Errors::MissingOrError),
    }
}

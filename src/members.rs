use std::collections::HashSet;

use colored::Colorize;

use crate::{make_paginated_github_request, Bootstrap};

#[derive(Debug, serde::Deserialize, Hash, Eq, PartialEq)]
struct Member {
    avatar_url: String,
}

pub fn run_audit(bootstrap: Bootstrap) {
    let members: HashSet<Member> = match make_paginated_github_request(
        &bootstrap.token,
        100,
        &format!("/orgs/{}/members", &bootstrap.org),
        3,
    ) {
        Ok(outside_collaborators) => outside_collaborators,
        Err(e) => {
            panic!("{}: {e}", "I couldn't fetch the organization members".red());
        }
    };

    for member in members {
        println!("{}", member.avatar_url);
    }
}

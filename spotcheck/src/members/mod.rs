use serde::{Deserialize, Serialize};

use crate::GitHubIndex;

pub mod audits;

#[derive(Debug, Serialize, Deserialize, Hash, Eq, PartialEq, PartialOrd, Ord)]
pub struct Member {
    pub login: String,
    pub avatar_url: String,
}

impl GitHubIndex for Member {
    fn index(&self) -> String {
        self.login.clone()
    }
}

use serde::{Deserialize, Serialize};

pub mod audits;

#[derive(Serialize, Deserialize, Hash, Eq, PartialEq, PartialOrd, Ord)]
pub struct Team {
    pub slug: String,
    pub name: String,
}

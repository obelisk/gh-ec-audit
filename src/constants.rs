//! Shared constants used across the audits.
//!
//! Centralizing these avoids the magic numbers that used to be scattered
//! across the code base (page sizes, retry counts, API version, ...).

/// Base URL for the GitHub REST API
pub const API_BASE: &str = "https://api.github.com";

/// URL for the GitHub GraphQL API
pub const GRAPHQL_URL: &str = "https://api.github.com/graphql";

/// `User-Agent` header sent with every request
pub const USER_AGENT: &str = "GitHub EC Audit";

/// GitHub REST API version pinned via the `X-GitHub-Api-Version` header
pub const API_VERSION: &str = "2022-11-28";

/// Default number of retries for a single request
pub const DEFAULT_RETRIES: u8 = 3;

/// Default page size for paginated requests
pub const DEFAULT_PAGE_SIZE: u8 = 25;

/// Page size used when fetching repositories
pub const REPOS_PAGE_SIZE: u8 = 75;

/// Page size used when fetching organization members
pub const MEMBERS_PAGE_SIZE: u8 = 100;

/// Page size used when fetching security alerts
pub const ALERTS_PAGE_SIZE: u8 = 30;

/// How long to wait when GitHub reports that the rate limit was exceeded
pub const RATE_LIMIT_SLEEP_SECS: u64 = 60;

/// Locations where a CODEOWNERS file can be placed, sorted by priority.
/// For more info, see
/// <https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners#codeowners-file-location>
pub const CO_LOCATIONS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];

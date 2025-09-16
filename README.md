### gh-ec-audit

Because GitHub just doesn't provide the APIs you need to audit External Collaborators.

This CLI audits a GitHub organization across several areas (external collaborators, deploy keys, admins, CODEOWNERS, teams, and more). It uses the GitHub REST API and prints actionable findings to stdout; some audits also emit CSV.

### Features

- **External Collaborators audit (`--ec`)**: Enumerates outside collaborators and their repo-level access; optionally compares against a previous CSV to preserve approvals and highlight access changes. Prints an updated CSV to stdout.
- **Deploy Keys audit (`--dk`)**: Lists deploy keys per repository; flags keys added by non-members. With `--all`, prints keys regardless of who added them.
- **Members audit (`--mem`)**: Lists organization members (currently prints the member avatar URLs).
- **Admin audit (`--admin`)**: Finds repo admins who are not organization admins and not members of a repo admin team. Supports limiting to `--repos`.
- **BPR & Rulesets audit (`--bpr`)**: For each repo, prints the default branch, Branch Protection Rules JSON, and Rulesets JSON.
- **Team permissions audit (`--teamperm --team <slug>`)**: Lists repositories a team can access with highest permission per repo.
- **Empty teams audit (`--emptyteams`)**: Lists teams with no members and how many repos each can access.
- **CODEOWNERS audit (`--codeowners`)**: Fetches CODEOWNERS files across the org (via repo enumeration or GitHub Search) and checks:
  - Users referenced are organization members
  - Teams referenced exist in the org
  - Teams referenced are not empty (warns if empty)
  Optionally also asks the GitHub API for CODEOWNERS parsing errors with `--also-gh-api`. Use `--verbose` to print successes.
- **Team occurrences in CODEOWNERS (`--team-in-codeowners --team <slug>`)**: Finds where a team is referenced in CODEOWNERS across the org (useful before renames/removals).

### Requirements

- Rust toolchain (to build from source): `cargo`.
- Environment variables:
  - `GH_TOKEN`: Fine-grained or classic PAT with read access to the org and repositories you want to audit. For private repos, ensure the token has access. Some endpoints (e.g., deploy keys) may require elevated permissions on the repository.
  - `GH_ORG`: The GitHub organization slug (e.g., `my-org`).

Example:

```bash
export GH_TOKEN="ghp_xxx..."  # fine-grained PAT recommended
export GH_ORG="your-org"
```

### Install

- Install Rust/cargo:

- macOS (Homebrew):

```bash
brew install rustup-init
rustup-init -y
source "$HOME/.cargo/env"
cargo --version
```

- macOS/Linux (rustup):

```bash
curl https://sh.rustup.rs -sSf | sh -s -- -y
source "$HOME/.cargo/env"
cargo --version
```

- Note for macOS: If prompted for developer tools, run `xcode-select --install`.

- From source (local checkout):

```bash
cargo build --release
# binary at target/release/gh-ec-audit

# or install into cargo bin
cargo install --path .
```

### Usage

Exactly one audit should be run at a time; pick the appropriate flag below. Common options:

- `--repos repo1,repo2` limit to a comma-separated list of repositories (supported by several audits)
- `--previous <file.csv>` path to a previous run CSV (used by `--ec`)
- `--team <slug>` team slug (used by `--teamperm` and `--team-in-codeowners`)
- `--search` use GitHub Search API instead of enumerating repos (CODEOWNERS-related audits)
- `--also-gh-api` additionally call the GH API that reports CODEOWNERS parsing errors
- `--verbose` increase output verbosity (some audits)
- `--all` disable default filtering where applicable (used by deploy keys)

Show version/help:

```bash
gh-ec-audit -V
gh-ec-audit -h
```

Run from source:

```bash
cargo run -- --ec
```

#### External Collaborators

```bash
# First run (no previous approvals/metadata)
gh-ec-audit --ec > ec-audit.csv

# Subsequent run to preserve prior metadata and detect access changes
gh-ec-audit --ec --previous ec-audit.csv > ec-audit.updated.csv
```

Output: CSV to stdout with columns: GitHub User, Repo, Access, Status, JIRA Ticket, Quorum Proposal. Changes in access are highlighted in logs and corresponding rows reset approvals in the new CSV.

#### Deploy Keys

```bash
# Only keys added by non-members (default filter)
gh-ec-audit --dk

# Show all deploy keys regardless of adder
gh-ec-audit --dk --all
```

#### Organization Members

```bash
gh-ec-audit --mem
```

#### Repository Admins (non-org-admin, non-admin-team)

```bash
# Org-wide
gh-ec-audit --admin

# Limited to repos
gh-ec-audit --admin --repos repo-one,repo-two
```

#### Branch Protection Rules & Rulesets

```bash
gh-ec-audit --bpr
gh-ec-audit --bpr --repos repo-one,repo-two
```

#### Team Permissions

```bash
gh-ec-audit --teamperm --team my-team-slug
```

#### Empty Teams

```bash
gh-ec-audit --emptyteams
```

#### CODEOWNERS Audit

```bash
# Enumerate repos and check CODEOWNERS content
gh-ec-audit --codeowners

# Use GitHub Search to discover CODEOWNERS (faster but rate-limited)
gh-ec-audit --codeowners --search

# Ask GitHub for CODEOWNERS parsing errors as well
gh-ec-audit --codeowners --also-gh-api

# Verbose mode prints confirmations for clean files
gh-ec-audit --codeowners --verbose

# Limit to specific repos (cannot be combined with --search)
gh-ec-audit --codeowners --repos repo-one,repo-two
```

Checks performed:

- Users mentioned with `@user` are members of the org
- Teams mentioned with `@org/team` exist
- Warns if referenced teams are empty (no members, including sub-teams)

#### Find Team in CODEOWNERS

```bash
# Enumerate repos
gh-ec-audit --team-in-codeowners --team platform-eng

# Use GitHub Search
gh-ec-audit --team-in-codeowners --team platform-eng --search
```

### Notes and Limits

- **Permissions**: Your token must have read access to the organization and to private repositories you want to inspect. Some endpoints (e.g., deploy keys) may require admin-level access on the repository to be fully visible; repositories without sufficient access will be skipped with a warning.
- **Rate limiting**: The GitHub Search API used by `--search` has a distinct, stricter rate limit. The tool retries on transient errors and will log when it needs to wait.
- **Pagination & retries**: All list endpoints are paginated; the tool handles pagination and performs limited retries on failures.
- **Colorized logs**: Output uses ANSI colors; redirecting to files retains escape codes unless you strip them.

### Development

```bash
# Run any audit
cargo run -- --codeowners --verbose

# Format / lint as per your local setup
```



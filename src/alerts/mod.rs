use std::{
    collections::HashSet,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use colored::Colorize;
use serde::Deserialize;

use crate::{get_repo_visibility, make_paginated_github_request, Bootstrap};

/// The severity of a security alert
#[derive(Debug, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "lowercase")]
enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

// -------------------------------------- Dependabot --------------------------------------

/// A security advisory returned by Dependabot
#[derive(Debug, Deserialize, Eq, PartialEq, Hash)]
struct DependabotSecurityAdvisory {
    severity: Severity,
}

/// A security alert returned by Dependabot
#[derive(Debug, Deserialize, Eq, PartialEq, Hash)]
struct DependabotAlert {
    number: u32,
    security_advisory: DependabotSecurityAdvisory,
}

// -------------------------------------- End of Dependabot --------------------------------------

// -------------------------------------- Code Scanning --------------------------------------

/// A rule used by CodeQL when analyzing code
#[derive(Debug, Deserialize, Eq, PartialEq, Hash)]
struct CodeScanningRule {
    security_severity_level: Severity,
}

/// A security alert returned by CodeQL
#[derive(Debug, Deserialize, Eq, PartialEq, Hash)]
struct CodeScanningAlert {
    number: u32,
    rule: CodeScanningRule,
}

// -------------------------------------- End of Code Scanning --------------------------------------

// -------------------------------------- Alert Counts --------------------------------------

/// Counters for alerts, grouped by severity
#[derive(Debug)]
struct AlertCounts {
    low: i32,
    medium: i32,
    high: i32,
    critical: i32,
}

impl AlertCounts {
    /// Return the total number of alerts
    fn total(self: &Self) -> i32 {
        self.low + self.medium + self.high + self.critical
    }

    /// Create a special object to signal that the security analysis is Not Available
    fn na() -> Self {
        Self {
            low: -1,
            medium: -1,
            high: -1,
            critical: -1,
        }
    }
}

impl From<Result<HashSet<DependabotAlert>, String>> for AlertCounts {
    fn from(alerts: Result<HashSet<DependabotAlert>, String>) -> Self {
        match alerts {
            Err(_) => Self::na(),
            Ok(alerts) => {
                let mut low = 0;
                let mut medium = 0;
                let mut high = 0;
                let mut critical = 0;

                for alert in alerts {
                    match alert.security_advisory.severity {
                        Severity::Low => low += 1,
                        Severity::Medium => medium += 1,
                        Severity::High => high += 1,
                        Severity::Critical => critical += 1,
                    }
                }

                Self {
                    low,
                    medium,
                    high,
                    critical,
                }
            }
        }
    }
}

impl From<Result<HashSet<CodeScanningAlert>, String>> for AlertCounts {
    fn from(alerts: Result<HashSet<CodeScanningAlert>, String>) -> Self {
        match alerts {
            Err(_) => Self::na(),
            Ok(alerts) => {
                let mut low = 0;
                let mut medium = 0;
                let mut high = 0;
                let mut critical = 0;

                for alert in alerts {
                    match alert.rule.security_severity_level {
                        Severity::Low => low += 1,
                        Severity::Medium => medium += 1,
                        Severity::High => high += 1,
                        Severity::Critical => critical += 1,
                    }
                }

                Self {
                    low,
                    medium,
                    high,
                    critical,
                }
            }
        }
    }
}

// -------------------------------------- End of Alert Counts --------------------------------------

/// The situation of a repository, with details about its security alerts
#[derive(Debug)]
struct RepoAlerts {
    name: String,
    dependabot: AlertCounts,
    codescanning: AlertCounts,
}

/// Return the string representation of the given number if it is >= 0.
/// Otherwise, return the string "NA".
fn number_or_na(num: i32) -> String {
    if num < 0 {
        "NA".to_string()
    } else {
        num.to_string()
    }
}

/// Run the audit on the security alerts for the given repos (or all org repos if none are passed).
/// Optionally, produce a CSV file with the results.
pub fn run_alerts_audit(bootstrap: Bootstrap, repos: Option<Vec<String>>, csv: bool) {
    let repos = repos.unwrap_or_else(|| {
        bootstrap
            .fetch_all_repositories(75)
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<String>>()
    });

    let repo_alerts = repos
        .iter()
        .map(|repo| RepoAlerts {
            name: repo.to_string(),
            dependabot: AlertCounts::from(fetch_dependabot_alerts(&bootstrap, &repo)),
            codescanning: AlertCounts::from(fetch_codescanning_alerts(&bootstrap, &repo)),
        })
        .collect::<Vec<_>>();

    // Print or write to file all the results

    if csv {
        // Create file and all intermediate folders if necessary
        let csv_file = "output/alerts.csv".to_string();
        let path = Path::new(&csv_file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect(&"Could not create folders".red());
        }
        let file = File::create(path).expect(&"Could not create CSV file".red());
        let mut writer = BufWriter::new(file);

        // Write headers
        writeln!(writer, "repository, visibility, dependabot alerts, low, medium, high, critical, code scanning alerts, low, medium, high, critical").expect(&"Could not write to CSV file".red());

        for repo_alert in repo_alerts {
            let line = format!(
                "{}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}",
                repo_alert.name,
                get_repo_visibility(&bootstrap, &repo_alert.name).unwrap_or("NA".to_string()),
                number_or_na(repo_alert.dependabot.total()),
                number_or_na(repo_alert.dependabot.low),
                number_or_na(repo_alert.dependabot.medium),
                number_or_na(repo_alert.dependabot.high),
                number_or_na(repo_alert.dependabot.critical),
                number_or_na(repo_alert.codescanning.total()),
                number_or_na(repo_alert.codescanning.low),
                number_or_na(repo_alert.codescanning.medium),
                number_or_na(repo_alert.codescanning.high),
                number_or_na(repo_alert.codescanning.critical)
            );
            writeln!(writer, "{}", line).expect(&"Could not write to CSV file".red());
        }

        println!(
            "{} {}",
            "Successfully written file".green(),
            csv_file.white()
        );
    } else {
        for repo_alert in repo_alerts {
            println!(
                "{}", format!(
                    "{} ({})\n\tDependabot: Total {} - Low {} - Medium {} - High {} - Critical {}\n\tCode Scanning: Total {} - Low {} - Medium {} - High {} - Critical {}",
                    repo_alert.name,
                    get_repo_visibility(&bootstrap, &repo_alert.name).unwrap_or("NA".to_string()),
                    number_or_na(repo_alert.dependabot.total()),
                    number_or_na(repo_alert.dependabot.low),
                    number_or_na(repo_alert.dependabot.medium),
                    number_or_na(repo_alert.dependabot.high),
                    number_or_na(repo_alert.dependabot.critical),
                    number_or_na(repo_alert.codescanning.total()),
                    number_or_na(repo_alert.codescanning.low),
                    number_or_na(repo_alert.codescanning.medium),
                    number_or_na(repo_alert.codescanning.high),
                    number_or_na(repo_alert.codescanning.critical)
                )
            )
        }
    }
}

/// Fetch all Dependabot alerts for the given repo.
fn fetch_dependabot_alerts(
    bootstrap: &Bootstrap,
    repo: &str,
) -> Result<HashSet<DependabotAlert>, String> {
    make_paginated_github_request(
        &bootstrap.token,
        30,
        &format!("/repos/{}/{repo}/dependabot/alerts", bootstrap.org),
        3,
        Some("&state=open"),
    )
}

/// Fetch all CodeQL alerts for the given repo.
fn fetch_codescanning_alerts(
    bootstrap: &Bootstrap,
    repo: &str,
) -> Result<HashSet<CodeScanningAlert>, String> {
    make_paginated_github_request(
        &bootstrap.token,
        30,
        &format!("/repos/{}/{repo}/code-scanning/alerts", bootstrap.org),
        3,
        Some("&state=open"),
    )
}

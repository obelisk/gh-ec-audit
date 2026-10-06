use std::{collections::HashSet, io::Write};

use colored::Colorize;
use serde::Deserialize;

use crate::constants::ALERTS_PAGE_SIZE;
use crate::{create_csv_writer, csv_escape, GitHubClient};

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
    fn total(&self) -> i32 {
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
pub fn run_alerts_audit(client: GitHubClient, repos: Option<Vec<String>>, csv: bool) {
    let repos = client.resolve_repos(repos);

    let repo_alerts = repos
        .iter()
        .map(|repo| RepoAlerts {
            name: repo.to_string(),
            dependabot: AlertCounts::from(fetch_dependabot_alerts(&client, repo)),
            codescanning: AlertCounts::from(fetch_codescanning_alerts(&client, repo)),
        })
        .collect::<Vec<_>>();

    // Print or write to file all the results

    if csv {
        let csv_file = "output/alerts.csv";
        let mut writer = create_csv_writer(csv_file);

        // Write headers
        writeln!(writer, "repository,visibility,dependabot_alerts,low,medium,high,critical,code_scanning_alerts,low,medium,high,critical").expect("Could not write to CSV file");

        for repo_alert in repo_alerts {
            let line = format!(
                "{},{},{},{},{},{},{},{},{},{},{},{}",
                csv_escape(&repo_alert.name),
                csv_escape(
                    &client
                        .repo_visibility(&repo_alert.name)
                        .unwrap_or("NA".to_string())
                ),
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
            writeln!(writer, "{}", line).expect("Could not write to CSV file");
        }

        println!(
            "{} {}",
            "Successfully written file".green(),
            csv_file.white()
        );
    } else {
        for repo_alert in repo_alerts {
            println!(
                "{} ({})\n\tDependabot: Total {} - Low {} - Medium {} - High {} - Critical {}\n\tCode Scanning: Total {} - Low {} - Medium {} - High {} - Critical {}",
                repo_alert.name,
                client
                    .repo_visibility(&repo_alert.name)
                    .unwrap_or("NA".to_string()),
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
        }
    }
}

/// Fetch all Dependabot alerts for the given repo.
fn fetch_dependabot_alerts(
    client: &GitHubClient,
    repo: &str,
) -> Result<HashSet<DependabotAlert>, String> {
    client.get_paginated(
        &format!("/repos/{}/{repo}/dependabot/alerts", client.org()),
        ALERTS_PAGE_SIZE,
        Some("state=open"),
    )
}

/// Fetch all CodeQL alerts for the given repo.
fn fetch_codescanning_alerts(
    client: &GitHubClient,
    repo: &str,
) -> Result<HashSet<CodeScanningAlert>, String> {
    client.get_paginated(
        &format!("/repos/{}/{repo}/code-scanning/alerts", client.org()),
        ALERTS_PAGE_SIZE,
        Some("state=open"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_or_na_handles_negative_sentinel() {
        assert_eq!(number_or_na(-1), "NA");
        assert_eq!(number_or_na(0), "0");
        assert_eq!(number_or_na(7), "7");
    }

    #[test]
    fn na_counts_total_is_negative() {
        // The NA sentinel is -1 per bucket; callers must use number_or_na.
        assert_eq!(AlertCounts::na().total(), -4);
    }

    #[test]
    fn counts_from_dependabot_alerts() {
        let alerts: HashSet<DependabotAlert> = [
            DependabotAlert {
                number: 1,
                security_advisory: DependabotSecurityAdvisory {
                    severity: Severity::High,
                },
            },
            DependabotAlert {
                number: 2,
                security_advisory: DependabotSecurityAdvisory {
                    severity: Severity::Critical,
                },
            },
        ]
        .into_iter()
        .collect();
        let counts = AlertCounts::from(Ok(alerts));
        assert_eq!(counts.high, 1);
        assert_eq!(counts.critical, 1);
        assert_eq!(counts.total(), 2);
    }

    #[test]
    fn counts_from_error_are_na() {
        let counts = AlertCounts::from(Err::<HashSet<DependabotAlert>, String>("boom".into()));
        assert_eq!(counts.total(), -4);
    }
}

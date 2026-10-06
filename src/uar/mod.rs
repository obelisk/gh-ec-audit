mod csv;
mod zip;

use std::collections::{HashMap, HashSet};

use colored::Colorize;

use crate::{
    codeowners::{iterate::get_co_file, CodeownersFile},
    email_from_gh_username, get_repo_collaborators, get_repo_teams,
    members::get_org_admins,
    teams::get_indexed_org_teams,
    Bootstrap, Collaborator, Permissions, Team,
};

/// The folder where we will write output files
const OUTPUT_FOLDER: &str = "output";

/// Permissions for all users and teams involved in the UAR
struct UarPermissions {
    /// { user --> (repo, permissions) }
    user_repo_permissions: HashMap<String, HashSet<(String, Permissions)>>,
    /// { team --> (repo, permissions) }
    team_repo_permissions: HashMap<String, HashSet<(String, Option<Permissions>)>>,
}

/// All the actors (users and teams) mentioned in a CODEOWNERS file or that have access to a repo
struct UarUsersAndTeams {
    collaborators: HashSet<Collaborator>,
    teams: HashSet<Team>,
}

/// Run the User Access Review on the given repos (mandatory argument).
///
/// For each repo, fetch the CODEOWNERS file, if one is present, and collect
/// a team-to-repos mapping. If no CODEOWNERS file is found, perform a
/// traditional access review by looking at which users/team have access
/// to the repo and with which permissions.
///
/// Finally, dump all members of all teams we have encountered.
pub fn run_uar_audit(bootstrap: Bootstrap, repos: Vec<String>, csv: bool, zip: Option<String>) {
    println!("{}", "Performing the UAR on the given repos...".yellow());
    let UarPermissions {
        user_repo_permissions,
        team_repo_permissions,
    } = repos_uar(&bootstrap, &repos, csv).expect(&format!(
        "{}",
        "I could not complete the UAR. Giving up.".red()
    ));

    teams_uar(&bootstrap, &team_repo_permissions, csv);

    if !csv {
        // Print the mappings for a complete picture
        // Users
        println!("{}", "USERS' ACCESS".green());
        for (user, repos_with_perms) in &user_repo_permissions {
            println!("{} {}", "Username:".green(), user.white());
            for (repo, perms) in repos_with_perms {
                println!(
                    "\t{} {}, {} {}",
                    "Repository".green(),
                    repo.white(),
                    "Permissions".green(),
                    perms.highest_perm().white()
                );
            }
        }
    }

    if let Some(output_zip) = zip {
        // zip assumes csv, otherwise we would have panicked immediately, so we know CSV files have been produced
        match zip::zip_dir_recursive(OUTPUT_FOLDER, &output_zip) {
            Ok(hash) => println!("SHA256 hash of '{}': {}", output_zip, hash),
            Err(e) => eprintln!("Error: {}", e),
        }
    }
}

/// Gather information about who has access to the repos in scope for the UAR
fn repos_uar(bootstrap: &Bootstrap, repos: &[String], csv: bool) -> Result<UarPermissions, String> {
    // Mappings to store which repos users and teams have access to
    // { user/team --> (repo, permissions) }
    let mut users_to_repos = HashMap::new();
    let mut teams_to_repos = HashMap::new();

    // Get all the members and teams in the org: it will be used for the CO audit
    let org_teams = get_indexed_org_teams(bootstrap);

    let org_admins: HashSet<String> = get_org_admins(bootstrap)
        .iter()
        .map(|(login, _member)| login.clone())
        .collect();

    // Prepare vectors where we will collect all the lines to be written to CSV files, if we need to do so
    let mut csv_lines_codeowners = vec![];
    let mut csv_lines_traditional = vec![];

    // Write CSV headers
    csv_lines_codeowners.push("repository,login,user_or_team,email".to_string());
    csv_lines_traditional.push("repository,login,user_or_team,email,permissions".to_string());

    for repo in repos {
        // Find all the users and teams that have access to this repo.
        // If we have a CODEOWNERS file, we focus on that one, otherwise
        // we fall back to looking at the repo's permissions.
        let (uar_users_and_teams, using_codeowners) =
            if let Ok(Some(co_file)) = get_co_file(&bootstrap, &repo) {
                // We have a CODEOWNERS file: parse it and collect all users and teams
                // mentioned in the file, then perform a UAR on those users / teams.
                println!(
                    "{} {}{}",
                    "CODEOWNERS file found for repo".green(),
                    repo.white(),
                    ": using that for the UAR".green()
                );
                (co_uar(&bootstrap, &repo, co_file, &org_teams), true)
            } else {
                // No CODEOWNERS file found: proceed with traditional UAR
                println!(
                    "{} {}{}",
                    "No CODEOWNERS file found for repo".yellow(),
                    repo.white(),
                    ": proceeding with traditional UAR".yellow()
                );
                (traditional_uar(&bootstrap, &repo, &org_admins), false)
            };

        if let Ok(UarUsersAndTeams {
            collaborators,
            teams,
        }) = uar_users_and_teams
        {
            // Add everything we found to the mappings
            for u in &collaborators {
                users_to_repos
                    .entry(u.login.to_string())
                    .or_insert_with(HashSet::new)
                    .insert((repo.clone(), u.permissions.clone()));
            }
            for t in &teams {
                teams_to_repos
                    .entry(t.slug.clone())
                    .or_insert_with(HashSet::new)
                    .insert((repo.clone(), t.permissions.clone()));
            }

            if csv {
                if using_codeowners {
                    // Write codeowners lines
                    // Users
                    for u in &collaborators {
                        csv_lines_codeowners.push(format!(
                            "{repo},{},User,{}",
                            u.login,
                            email_from_gh_username(&bootstrap, &u.login)
                                .unwrap_or("Not available".to_string())
                        ));
                    }
                    // Teams
                    for t in &teams {
                        csv_lines_codeowners.push(format!("{repo},{},Team,None", t.slug));
                    }
                } else {
                    // Write traditional lines
                    // Users
                    for u in &collaborators {
                        csv_lines_traditional.push(format!(
                            "{repo},{},User,{},{}",
                            u.login,
                            email_from_gh_username(&bootstrap, &u.login)
                                .unwrap_or("Not available".to_string()),
                            u.permissions.highest_perm()
                        ));
                    }
                    // Teams
                    for t in &teams {
                        csv_lines_traditional.push(format!(
                            "{repo},{},Team,None,{}",
                            t.slug,
                            t.permissions.as_ref().unwrap().highest_perm()
                        ));
                    }
                }
            } else {
                // No CSV: print out the access to this repo
                let repo_users: Vec<String> =
                    collaborators.iter().map(|c| c.login.clone()).collect();
                let repo_teams: Vec<String> = teams.iter().map(|t| t.slug.clone()).collect();
                println!(
                    "{} {:?} {} {:?}",
                    "Users:".green(),
                    repo_users,
                    "\nTeams:".green(),
                    repo_teams
                );
            }
        } else {
            return Err(format!("Could not fetch users and teams for repo {repo}"));
        }
    }

    // We are done looping through the repos. If we need to produce CSV files, we do it now
    if csv {
        csv::repo_audit_to_csv(
            format!("{OUTPUT_FOLDER}/codeowners_uar.csv"),
            &csv_lines_codeowners,
        );
        csv::repo_audit_to_csv(
            format!("{OUTPUT_FOLDER}/traditional_uar.csv"),
            &csv_lines_traditional,
        );
    }

    Ok(UarPermissions {
        user_repo_permissions: users_to_repos,
        team_repo_permissions: teams_to_repos,
    })
}

/// Gather information about teams and team membership
fn teams_uar(
    bootstrap: &Bootstrap,
    teams_to_repos: &HashMap<String, HashSet<(String, Option<Permissions>)>>,
    csv: bool,
) {
    if csv {
        // Write to CSV the access that teams have
        csv::team_access_to_csv(format!("{OUTPUT_FOLDER}/teams_access.csv"), &teams_to_repos);
        // Write to CSV all the members of the teams
        csv::team_members_to_csv(
            &bootstrap,
            format!("{OUTPUT_FOLDER}/teams_membership.csv"),
            &teams_to_repos,
        );
    } else {
        // Just print to stdout
        println!("\n{}", "TEAMS' ACCESS".green());
        for (team, repos_with_perms) in teams_to_repos {
            println!("{} {}", "Team name:".green(), team.white());
            for (repo, perms) in repos_with_perms {
                // If we found this team during a traditional UAR, then we will
                // have its permissions, otherwise it means we encountered it
                // during a CODEOWNERS UAR, and we set it simply to "Codeowner"
                let p = match perms {
                    Some(p) => p.highest_perm(),
                    None => "Codeowner".to_string(),
                };
                println!(
                    "\t{} {}, {} {}",
                    "Repository".green(),
                    repo.white(),
                    "Permissions".green(),
                    p.white()
                );
            }
        }

        println!("\n{}", "TEAMS' MEMBERS".green());
        for (team, _) in teams_to_repos {
            // A temporary team object just to be able to call the fetch_members method
            let tmp_team = Team {
                slug: team.to_string(),
                name: team.to_string(),
                permissions: None,
            };
            match tmp_team.fetch_team_members(&bootstrap) {
                Ok(m) => {
                    let members: Vec<String> = m.keys().map(|v| v.to_string()).collect();
                    println!("{}:{:?}", team.white(), members);
                }
                Err(e) => {
                    println!(
                        "{} {}{} {e}",
                        "Warning! Couldn't fetch members of team".yellow(),
                        team.white(),
                        ". I am ignoring this and proceeding. Error:".yellow()
                    )
                }
            }
        }
    }
}

/// Extract in-scope collaborators and teams from the given CODEOWNERS file.
fn co_uar(
    bootstrap: &Bootstrap,
    repo: &str,
    co_file: CodeownersFile,
    org_teams: &HashMap<String, Team>,
) -> Result<UarUsersAndTeams, String> {
    // Get all users that have access to this repo.
    // Then we will filter and keep only those that appear in the CO file.
    let users = get_repo_collaborators(bootstrap, repo);

    match users {
        Ok(users) => {
            let filtered_users = users
                .into_iter()
                .filter(|u| co_file.users.contains(&u.login))
                .collect();
            let filtered_teams = org_teams
                .iter()
                .filter_map(|(slug, t)| {
                    if co_file.teams.contains(slug) {
                        Some(t.clone())
                    } else {
                        None
                    }
                })
                .collect();
            Ok(UarUsersAndTeams {
                collaborators: filtered_users,
                teams: filtered_teams,
            })
        }
        _ => Err(
            "Something went wrong while retrieving users and teams from CODEOWNERS file"
                .to_string(),
        ),
    }
}

/// Extract in-scope collaborators and teams by looking at who has access to the repo.
fn traditional_uar(
    bootstrap: &Bootstrap,
    repo: &str,
    org_admins: &HashSet<String>,
) -> Result<UarUsersAndTeams, String> {
    let users = get_repo_collaborators(bootstrap, repo);
    let teams = get_repo_teams(bootstrap, repo);

    match (users, teams) {
        (Ok(users), Ok(teams)) => {
            // Filter out the members of org_admins
            let users = users
                .into_iter()
                .filter(|c| !org_admins.contains(&c.login))
                .collect();
            Ok(UarUsersAndTeams {
                collaborators: users,
                teams,
            })
        }
        _ => Err(
            "Something went wrong while retrieving users and teams for traditional UAR".to_string(),
        ),
    }
}

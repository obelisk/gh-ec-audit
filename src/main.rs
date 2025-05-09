use colored::Colorize;
use gh_ec_audit::bpr;
use gh_ec_audit::deploy_key;
use gh_ec_audit::external_collaborator;

use clap::{command, Parser};
use gh_ec_audit::codeowners;
use gh_ec_audit::members;
use gh_ec_audit::teams;
use gh_ec_audit::Bootstrap;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Run the external collaborator audit
    #[arg(short, long)]
    ec: bool,

    /// Run the deploy key audit
    #[arg(short, long)]
    dk: bool,

    /// Run the members audit
    #[arg(short, long)]
    mem: bool,

    /// Run the admin audit
    #[arg(short, long)]
    admin: bool,

    /// Run the BPR and rulesets audit
    #[arg(short, long)]
    bpr: bool,

    /// Run the team permissions audit
    #[arg(short, long)]
    teamperm: bool,

    /// Run the empty teams audit
    #[arg(long)]
    emptyteams: bool,

    /// Run the CODEOWNERS audit
    #[arg(short, long)]
    codeowners: bool,

    /// Find occurrences of a team in CODEOWNERS files
    #[arg(long)]
    team_in_codeowners: bool,

    /// Also invoke the GH API to get extra info when auditing CODEOWNERS
    #[arg(long)]
    also_gh_api: bool,

    /// Focus the audit on a given GH team
    #[arg(long)]
    team: Option<String>,

    /// Disable filtering on specific audits
    #[clap(long, default_value_t = false)]
    all: bool,

    /// Use GH search API instead of enumerating repos (only for specific audits)
    #[clap(long, default_value_t = false)]
    search: bool,

    /// Limit the scanning to the given repos
    #[clap(short, long, value_delimiter = ',', num_args = 1..)]
    repos: Option<Vec<String>>,

    /// The previous run CSV file
    #[arg(short, long)]
    previous: Option<String>,

    /// Increase verbosity
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    let args = Args::parse();

    let bootstrap = match Bootstrap::new() {
        Ok(b) => b,
        Err(e) => {
            println!("{}", e.bold().red());
            std::process::exit(1);
        }
    };

    if args.ec {
        external_collaborator::run_audit(bootstrap, args.previous);
    } else if args.dk {
        deploy_key::run_audit(bootstrap, args.previous, args.all);
    } else if args.mem {
        members::run_audit(bootstrap);
    } else if args.admin {
        members::run_admin_audit(bootstrap, args.repos);
    } else if args.bpr {
        bpr::run_audit(bootstrap, args.repos);
    } else if args.teamperm {
        if let Some(team) = args.team {
            teams::run_team_repo_audit(bootstrap, team);
        } else {
            println!("Please specify a team with --team");
        }
    } else if args.emptyteams {
        teams::run_empty_teams_audit(bootstrap);
    } else if args.codeowners {
        codeowners::run_codeowners_audit(
            bootstrap,
            args.repos,
            args.search,
            args.also_gh_api,
            args.verbose,
        );
    } else if args.team_in_codeowners {
        if let Some(team) = args.team {
            codeowners::run_team_in_codeowners_audit(bootstrap, team, args.repos, args.search);
        } else {
            println!("Please specify a team with --team");
        }
    } else {
        println!("No command specified");
    }
}

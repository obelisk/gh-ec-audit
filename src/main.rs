use colored::Colorize;
use gh_ec_audit::bpr;
use gh_ec_audit::deploy_key;
use gh_ec_audit::external_collaborator;

use clap::{command, Parser};
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

    /// Run the BPR and rulesets audit
    #[arg(short, long)]
    teamperm: bool,

    #[arg(long)]
    team: Option<String>,

    /// Limit the scanning to the given repos
    #[clap(short, long, value_delimiter = ',', num_args = 1..)]
    repos: Option<Vec<String>>,

    /// The previous run CSV file
    #[arg(short, long)]
    previous: Option<String>,
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
        deploy_key::run_audit(bootstrap, args.previous);
    } else if args.mem {
        members::run_audit(bootstrap);
    } else if args.admin {
        members::run_admin_audit(bootstrap, args.repos);
    } else if args.bpr {
        bpr::run_audit(bootstrap, args.repos);
    } else if args.teamaccess {
        if let Some(team) = args.team {
            teams::run_team_repo_audit(bootstrap, team);
        } else {
            println!("Please specify a team with --team");
        }
    } else {
        println!("No command specified");
    }
}

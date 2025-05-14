use colored::Colorize;
use spotcheck::bpr;
use spotcheck::deploy_key;
use spotcheck::external_collaborator;

use clap::{command, Parser};
use spotcheck::members;
use spotcheck::teams;
use spotcheck::Bootstrap;

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

    #[arg(long)]
    team: Option<String>,

    // Disable filtering on specific audits
    #[clap(long, default_value_t = false)]
    all: bool,

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
        deploy_key::run_audit(bootstrap, args.previous, args.all);
    } else if args.mem {
        match members::audits::run_total_member_audit(&bootstrap) {
            Ok(members) => {
                for member in members {
                    println!("{}", member.avatar_url);
                }
            }
            Err(e) => {
                println!("{}: {}", "I couldn't fetch the organization members".red(), e);
            }
        }
    } else if args.admin {
        members::audits::run_admin_audit(&bootstrap, args.repos);
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
    } else {
        println!("No command specified");
    }
}

use colored::Colorize;
use gh_ec_audit::deploy_key;
use gh_ec_audit::external_collaborator;

use clap::{command, Parser};
use gh_ec_audit::Bootstrap;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Run the external collaborator audit
    #[arg(short, long)]
    ec: bool,

    /// Run the deploy key audit
    #[arg(short, long)]
    dk: bool,

    /// The previous run CSV file
    #[arg(short, long)]
    previous: Option<String>,
}

fn main() {
    let bootstrap = match Bootstrap::new() {
        Ok(b) => b,
        Err(e) => {
            println!("{}", e.bold().red());
            std::process::exit(1);
        }
    };

    let args = Args::parse();

    if args.ec {
        external_collaborator::run_audit(bootstrap, args.previous);
    } else if args.dk {
        deploy_key::run_audit(bootstrap, args.previous);
    } else {
        println!("No command specified");
    }
}

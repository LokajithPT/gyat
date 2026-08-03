use clap::{Parser, Subcommand};
use std::fs;

mod utils;
use utils::init::init;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Status,
    Ignore,
    Push,
    Pull,
    Travel {
        commit: String,
    },
    #[command(subcommand)]
    Snip(SnipSub),
}

#[derive(Subcommand)]
enum SnipSub {
    Top,
    Bottom,
    Commit {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
    },
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();
    if let Some(command) = &cli.command {
        match command {
            Commands::Init => {
                init();
            }
            Commands::Status => {
                println!("status")
            }
            Commands::Travel { commit } => {
                println!("traveling to commit={commit}")
            }
            Commands::Ignore => {
                println!("ignore")
            }
            Commands::Push => {
                println!("push")
            }
            Commands::Pull => {
                println!("pull")
            }
            Commands::Snip(snip_sub) => match snip_sub {
                SnipSub::Top => {
                    println!("top")
                }
                SnipSub::Bottom => {
                    println!("bottom")
                }
                SnipSub::Commit { start, end } => {
                    println!("commit: start={:?} end={:?}", start, end)
                }
            },
        }
    }
    Ok(())
}

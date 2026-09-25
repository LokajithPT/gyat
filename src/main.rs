use clap::{Parser, Subcommand};

mod utils;
use utils::init::init;
use utils::status::status;

#[derive(Parser)]
#[command(author, version, about = "gyat - T420-ready, add/commit local, push remote (SSH)", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Status,
    Log,
    Add {
        files: Vec<String>,
    },
    Ignore,
    IgnoreAdd {
        pattern: String,
    },
    Commit {
        #[arg(short, long)]
        message: String,
    },
    Push,
    Pull {
        commit: Option<String>,
    },
    #[command(alias = "goto", alias = "go")]
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
    /// range syntax: gyat snip abc..def
    Range {
        range: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let res = match cli.command {
        Some(Commands::Init) => init(),
        Some(Commands::Status) => status(),
        Some(Commands::Log) => utils::log::log(),
        Some(Commands::Add { files }) => utils::add::add(&files),
        Some(Commands::Ignore) => utils::ignore::ignore_show(),
        Some(Commands::IgnoreAdd { pattern }) => utils::ignore::ignore_add(&pattern),
        Some(Commands::Commit { message }) => utils::commit::create_commit(message).map(|h| { println!("committed {h}"); }),
        Some(Commands::Push) => utils::push::push_remote(),
        Some(Commands::Pull { commit }) => utils::pull::pull(commit),
        Some(Commands::Travel { commit }) => utils::travel::travel(&commit),
        Some(Commands::Snip(snip_sub)) => match snip_sub {
            SnipSub::Top => utils::snip::snip_top(),
            SnipSub::Bottom => utils::snip::snip_bottom(),
            SnipSub::Commit { start, end } => utils::snip::snip_commit(&start, &end),
            SnipSub::Range { range } => {
                let parts: Vec<&str> = range.split("..").collect();
                if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                    Err(format!("invalid range '{range}', expected START..END"))
                } else {
                    utils::snip::snip_commit(parts[0], parts[1])
                }
            }
        },
        None => {
            println!("gyat - try `gyat --help`");
            println!("  init, add, commit -m \"msg\", push (remote), status, log, travel/goto <hash>, snip");
            Ok(())
        }
    };

    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

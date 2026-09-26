use clap::{Parser, Subcommand};

mod utils;
use utils::init::init;
use utils::status::status;

#[derive(Parser)]
#[command(author, version, about = "gyat", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Status,
    Log {
        #[arg(long)]
        oneline: bool,
    },
    Add {
        files: Vec<String>,
    },
    Ignore,
    IgnoreAdd {
        pattern: String,
    },
    Commit {
        /// commit message: gyat commit "msg" or gyat commit -m "msg"
        #[arg(short, long)]
        message: Option<String>,
        /// positional message (bare words joined with space)
        #[arg(num_args = 0..)]
        message_parts: Vec<String>,
    },
    Push {
        /// allow non-fast-forward updates on the server
        #[arg(long)]
        force: bool,
    },
    Pull {
        commit: Option<String>,
    },
    #[command(alias = "goto", alias = "go")]
    Travel {
        commit: String,
    },
    Merge {
        branch: String,
        #[arg(short, long)]
        message: Option<String>,
    },
    Clone {
        source: String,
        dest: Option<String>,
    },
    #[command(subcommand)]
    Snip(SnipSub),
    Branch {
        name: Option<String>,
        #[arg(short, long)]
        delete: bool,
        #[arg(short, long)]
        rename: Option<String>,
    },
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
    Range {
        range: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let res = match cli.command {
        Some(Commands::Init) => init(),
        Some(Commands::Status) => status(),
        Some(Commands::Log { oneline }) => utils::log::log(oneline),
        Some(Commands::Add { files }) => utils::add::add(&files),
        Some(Commands::Ignore) => utils::ignore::ignore_show(),
        Some(Commands::IgnoreAdd { pattern }) => utils::ignore::ignore_add(&pattern),
        Some(Commands::Commit { message, message_parts }) => {
            let msg_opt = message.or_else(|| {
                if message_parts.is_empty() { None } else { Some(message_parts.join(" ")) }
            });
            match msg_opt {
                None => Err("nothing to commit: need a message: `gyat commit \"msg\"` or `gyat commit -m \"msg\"`".to_string()),
                Some(m) if m.trim().is_empty() => Err("nothing to commit: empty message: `gyat commit \"msg\"`".to_string()),
                Some(m) => utils::commit::commit_with_message(m.trim().to_string()),
            }
        }
        Some(Commands::Push { force }) => utils::push::push_remote(force),
        Some(Commands::Pull { commit }) => utils::pull::pull(commit),
        Some(Commands::Travel { commit }) => utils::travel::travel(&commit),
        Some(Commands::Merge { branch, message }) => utils::merge::merge_branch(&branch, message),
        Some(Commands::Clone { source, dest }) => utils::clone::clone_repo(&source, dest),
        Some(Commands::Branch { name, delete, rename }) => {
            if let Some(new) = rename {
                match name {
                    Some(old) => utils::branch::rename(&old, &new),
                    None => Err("branch --rename <new> needs old name".to_string()),
                }
            } else if delete {
                match name {
                    Some(n) => utils::branch::delete(&n),
                    None => Err("branch -d needs name".to_string()),
                }
            } else if let Some(n) = name {
                utils::branch::create(&n)
            } else {
                utils::branch::list()
            }
        }
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
            println!("  init, add, commit -m, push, status, log, travel/goto, merge <branch>, branch, snip");
            Ok(())
        }
    };

    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

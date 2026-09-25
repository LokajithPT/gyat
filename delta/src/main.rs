use clap::Parser;
use delta::{create_delta_file, myers_diff, apply_deltas, read_lines};

#[derive(Parser)]
#[command(author, version, about = "Myers line-based diff")]
struct Args {
    /// file before the change
    old_file: String,
    /// file after the change
    new_file: String,
}

fn main() {
    let args = Args::parse();
    let old = read_lines(&args.old_file);
    let new = read_lines(&args.new_file);
    let deltas = myers_diff(&old, &new);
    for d in &deltas {
        match d {
            delta::Delta::Insert { line, text } => println!("+ [{}] {}", line, text),
            delta::Delta::Delete { line } => println!("- [{}] {}", line, old[*line]),
        }
    }
    let reconstructed = apply_deltas(&old, &deltas);
    if reconstructed != new {
        eprintln!("warning: reconstruction mismatch (bug)");
    }
    create_delta_file(&args.old_file, &args.new_file, &deltas);
}

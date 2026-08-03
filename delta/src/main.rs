use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Write};

use regex::Regex;

use clap::Parser;

#[derive(Debug)]
enum Delta {
    Insert { line: usize, text: String },
    Delete { line: usize },
}

fn hash(content: &str) -> u64 {
    let mut hasher = ahash::AHasher::default();
    Hash::hash(&content, &mut hasher);
    hasher.finish()
}

fn read_lines(path: &str) -> Vec<String> {
    let file = File::open(path).unwrap_or_else(|e| panic!("could not open {}: {}", path, e));
    let reader = BufReader::new(file);
    reader
        .lines()
        .map(|l| l.unwrap_or_else(|e| panic!("could not read {}: {}", path, e)))
        .collect()
}

fn create_delta_file(old: String, new: String, deltavec: &Vec<Delta>) {
    //regex for the .txt alone right now
    let re = Regex::new(r"\.txt$").unwrap();
    let old = re.replace(&old, "");
    let new = re.replace(&new, "");

    let filename = format!("{}_{}_delta", old, new);
    let file =
        File::create(&filename).unwrap_or_else(|e| panic!("could not create {}: {}", filename, e));
    let mut writer = BufWriter::new(file);

    for delta in deltavec {
        match delta {
            Delta::Insert { line, text } => {
                writeln!(writer, "insert::{}::{}", line, text).unwrap();
            }
            Delta::Delete { line } => {
                writeln!(writer, "delete::{}", line).unwrap();
            }
        }
    }
}

//main shit for the cli commands and stuff
#[derive(Parser)]
#[command(author, version, about = "line-based diff")]
struct Args {
    /// file before the change
    old_file: String,
    /// file after the change
    new_file: String,
}

fn main() {
    let args = Args::parse();

    let mut deltas = Vec::new();

    let old = read_lines(&args.old_file);
    let new = read_lines(&args.new_file);

    let mut old_by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, line) in old.iter().enumerate() {
        old_by_hash.entry(hash(line)).or_default().push(i);
    }

    let mut matched_old = vec![false; old.len()];

    for (i, line) in new.iter().enumerate() {
        let h = hash(line);
        if let Some(indices) = old_by_hash.get_mut(&h) {
            if let Some(idx) = indices.pop() {
                matched_old[idx] = true;
                continue;
            }
        }
        println!("+ {}", line);
        deltas.push(Delta::Insert {
            line: i,
            text: line.clone(),
        });
    }

    for (i, line) in old.iter().enumerate() {
        if !matched_old[i] {
            println!("- {}", line);

            deltas.push(Delta::Delete { line: i });
        }
    }

    create_delta_file(args.old_file, args.new_file, &deltas);

    //now lemme see what are all the deltas in here ...

    // println!("{:?} \n ", deltas);
    // for delta in &deltas {
    //     match delta {
    //         Delta::Insert { line, text } => {
    //             println!("insert line :: {} :: in index ::: {} ", text, line);
    //         }

    //         Delta::Delete { line } => {
    //             println!("delete line :: {}", line);
    //         }
    //     }
    // }
}

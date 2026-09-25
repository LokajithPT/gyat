use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use clap::Parser;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Delta {
    Insert { line: usize, text: String },
    Delete { line: usize },
}

fn read_lines(path: &str) -> Vec<String> {
    let file = File::open(path).unwrap_or_else(|e| panic!("could not open {}: {}", path, e));
    let reader = BufReader::new(file);
    reader
        .lines()
        .map(|l| l.unwrap_or_else(|e| panic!("could not read {}: {}", path, e)))
        .collect()
}

fn create_delta_file(old: &str, new: &str, deltavec: &[Delta]) {
    let old_stem = Path::new(old)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(old);
    let new_stem = Path::new(new)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(new);

    // sanitize path separators inside stems (in case old/new were dirs)
    let old_clean = old_stem.replace(['/', '\\'], "_");
    let new_clean = new_stem.replace(['/', '\\'], "_");

    let filename = format!("{}_{}_delta", old_clean, new_clean);
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
    println!("delta written to {}", filename);
}

/// Myers O(ND) shortest edit script. Lightweight, no extra deps.
/// Returns deltas in forward order (apply sequentially from start, or reverse for in-place patch).
fn myers_diff(old: &[String], new: &[String]) -> Vec<Delta> {
    let n = old.len() as i32;
    let m = new.len() as i32;

    if n == 0 && m == 0 {
        return Vec::new();
    }
    if n == 0 {
        return (0..m)
            .map(|i| Delta::Insert {
                line: i as usize,
                text: new[i as usize].clone(),
            })
            .collect();
    }
    if m == 0 {
        return (0..n)
            .map(|i| Delta::Delete { line: i as usize })
            .collect();
    }

    let max = (n + m) as usize;
    let offset = max as i32;
    let size = 2 * max + 1;
    let mut v = vec![0i32; size];
    let mut trace: Vec<Vec<i32>> = Vec::new();
    let mut d_found = 0i32;

    'outer: for d in 0..=max as i32 {
        for k in (-d..=d).step_by(2) {
            let k_offset = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize])
            {
                v[(k + 1 + offset) as usize]
            } else {
                v[(k - 1 + offset) as usize] + 1
            };
            let mut y = x - k;
            while x < n && y < m && old[x as usize] == new[y as usize] {
                x += 1;
                y += 1;
            }
            v[k_offset] = x;
            if x >= n && y >= m {
                trace.push(v.clone());
                d_found = d;
                break 'outer;
            }
        }
        trace.push(v.clone());
    }

    // backtrack
    let mut x = n;
    let mut y = m;
    let mut deltas_rev: Vec<Delta> = Vec::new();

    for d in (0..=d_found).rev() {
        let k = x - y;
        if d == 0 {
            while x > 0 && y > 0 && old[(x - 1) as usize] == new[(y - 1) as usize] {
                x -= 1;
                y -= 1;
            }
            break;
        }
        let v_prev = &trace[(d - 1) as usize];
        let prev_k = if k == -d || (k != d && v_prev[(k - 1 + offset) as usize] < v_prev[(k + 1 + offset) as usize])
        {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v_prev[(prev_k + offset) as usize];
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            // equal - no delta
        }

        if x == prev_x {
            // insert new[prev_y] at old position prev_x
            deltas_rev.push(Delta::Insert {
                line: prev_x as usize,
                text: new[prev_y as usize].clone(),
            });
        } else {
            // y == prev_y -> delete old[prev_x]
            deltas_rev.push(Delta::Delete {
                line: prev_x as usize,
            });
        }
        x = prev_x;
        y = prev_y;
    }

    deltas_rev.reverse();
    deltas_rev
}

/// Apply deltas (reverse order) to old to get new - used for tests/verification.
fn apply_deltas(old: &[String], deltas: &[Delta]) -> Vec<String> {
    let mut res = old.to_vec();
    // apply from largest line to smallest to avoid shift issues
    // for stable order when same line has multiple ops, deletions before inserts at same line
    let mut sorted = deltas.to_vec();
    sorted.sort_by(|a, b| {
        let la = match a {
            Delta::Insert { line, .. } => *line,
            Delta::Delete { line } => *line,
        };
        let lb = match b {
            Delta::Insert { line, .. } => *line,
            Delta::Delete { line } => *line,
        };
        lb.cmp(&la).then_with(|| match (a, b) {
            (Delta::Delete { .. }, Delta::Insert { .. }) => std::cmp::Ordering::Less,
            (Delta::Insert { .. }, Delta::Delete { .. }) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
    });
    for d in sorted {
        match d {
            Delta::Insert { line, text } => {
                if line <= res.len() {
                    res.insert(line, text);
                } else {
                    res.push(text);
                }
            }
            Delta::Delete { line } => {
                if line < res.len() {
                    res.remove(line);
                }
            }
        }
    }
    res
}

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
            Delta::Insert { line, text } => println!("+ [{}] {}", line, text),
            Delta::Delete { line } => println!("- [{}] {}", line, old[*line]),
        }
    }

    // verify round-trip
    let reconstructed = apply_deltas(&old, &deltas);
    if reconstructed != new {
        eprintln!("warning: reconstruction mismatch (bug)");
    }

    create_delta_file(&args.old_file, &args.new_file, &deltas);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn identical() {
        let a = s(&["a", "b", "c"]);
        let b = s(&["a", "b", "c"]);
        assert_eq!(myers_diff(&a, &b), vec![]);
        assert_eq!(apply_deltas(&a, &myers_diff(&a, &b)), b);
    }

    #[test]
    fn single_insert() {
        let a = s(&["a", "b"]);
        let b = s(&["a", "c", "b"]);
        let d = myers_diff(&a, &b);
        assert_eq!(d.len(), 1);
        assert!(matches!(d[0], Delta::Insert { line: 1, .. }));
        assert_eq!(apply_deltas(&a, &d), b);
    }

    #[test]
    fn single_delete() {
        let a = s(&["a", "b"]);
        let b = s(&["a"]);
        let d = myers_diff(&a, &b);
        assert_eq!(d, vec![Delta::Delete { line: 1 }]);
        assert_eq!(apply_deltas(&a, &d), b);
    }

    #[test]
    fn empty_old() {
        let a: Vec<String> = vec![];
        let b = s(&["x", "y"]);
        let d = myers_diff(&a, &b);
        assert_eq!(d.len(), 2);
        assert_eq!(apply_deltas(&a, &d), b);
    }

    #[test]
    fn empty_new() {
        let a = s(&["x", "y"]);
        let b: Vec<String> = vec![];
        let d = myers_diff(&a, &b);
        assert_eq!(d.len(), 2);
        assert_eq!(apply_deltas(&a, &d), b);
    }

    #[test]
    fn duplicate_lines() {
        let a = s(&["a", "a", "b"]);
        let b = s(&["a", "b", "a"]);
        let d = myers_diff(&a, &b);
        assert_eq!(apply_deltas(&a, &d), b);
    }

    #[test]
    fn with_colons() {
        let a = s(&["a::b", "c"]);
        let b = s(&["a::b", "x::y::z", "c"]);
        let d = myers_diff(&a, &b);
        assert_eq!(apply_deltas(&a, &d), b);
    }

    #[test]
    fn large_identical_prefix() {
        let mut a = s(&["x"; 100]);
        a.extend(s(&["a", "b"]));
        let mut b = s(&["x"; 100]);
        b.extend(s(&["a", "c", "b"]));
        let d = myers_diff(&a, &b);
        assert_eq!(apply_deltas(&a, &d), b);
    }
}

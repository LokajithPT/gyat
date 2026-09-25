use std::fs;
use super::config;
use super::repo;

pub fn status() -> Result<(), String> {
    repo::ensure_repo()?;
    let cfg = config::load().map_err(|e| format!("load config: {e}"))?;
    println!("repo: {}  user: {}  server: {}  ssh: {}",
        cfg.repo.name,
        cfg.repo.username,
        cfg.repo.server,
        cfg.ssh.as_ref().map(|s| s.key_path.as_str()).unwrap_or("~/.ssh/id_ed25519")
    );
    if let Some(ign) = &cfg.ignore {
        if !ign.files.is_empty() {
            println!("ignore: {:?}", ign.files);
        }
    }

    let head = repo::read_head();
    match &head {
        Some(h) if !h.is_empty() => println!("HEAD: {}", h),
        _ => println!("HEAD: (no commits yet)"),
    }

    let staged = repo::staged_files();
    if staged.is_empty() {
        println!("staged: 0 files");
    } else {
        println!("staged: {} file(s)", staged.len());
        for f in &staged {
            println!("  + {}", f.display());
        }
    }

    let commits = repo::list_commits();
    println!("commits: {} ", commits.len());
    if !commits.is_empty() {
        // show last 5
        let metas = super::commit::list_metas();
        for m in metas.iter().rev().take(5).rev() {
            println!("  {} \"{}\" by {} at {}", &m.hash[..8.min(m.hash.len())], m.message, m.author, m.timestamp);
        }
        if commits.len() > 5 {
            println!("  ... and {} more (gyat log to see all)", commits.len() - 5);
        }
    }

    let current = repo::current_root();
    if current.exists() {
        let count = walk_count(&current);
        println!("current snapshot: {count} file(s)");
    } else {
        println!("current: missing");
    }

    if staged.is_empty() && commits.is_empty() {
        println!("hint: `gyat add .` then `gyat commit -m \"init\"`");
    } else if staged.is_empty() {
        println!("hint: working tree clean (or unstaged changes not shown)");
    } else {
        println!("hint: `gyat commit -m \"msg\"` to commit, `gyat push` to remote");
    }
    Ok(())
}

fn walk_count(root: &std::path::Path) -> usize {
    let mut n = 0;
    if let Ok(entries) = fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() { n += walk_count(&p); } else { n += 1; }
        }
    }
    n
}

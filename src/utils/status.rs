use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;
use super::config;
use super::repo;

fn normalize_rel(p: &Path) -> String {
    p.strip_prefix(".")
        .unwrap_or(p)
        .to_string_lossy()
        .to_string()
        .trim_start_matches('/')
        .trim_start_matches("./")
        .to_string()
}

fn head_bytes(rel: &str) -> Option<Vec<u8>> {
    let head = repo::read_head()?;
    if head.is_empty() {
        return None;
    }
    let snap_file = repo::commit_snapshot_root(&head).join(rel);
    super::compression::read_bytes_maybe_compressed(&snap_file).ok()
}

fn head_tracked_set() -> BTreeSet<String> {
    let Some(head) = repo::read_head() else { return BTreeSet::new(); };
    if head.is_empty() {
        return BTreeSet::new();
    }
    let snap_root = repo::commit_snapshot_root(&head);
    if !snap_root.exists() {
        return BTreeSet::new();
    }
    let mut out = BTreeSet::new();
    for entry in WalkDir::new(&snap_root).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            if let Ok(rel) = p.strip_prefix(&snap_root) {
                let mut s = rel.to_string_lossy().to_string();
                if s.ends_with(".gz") {
                    s.truncate(s.len() - 3);
                }
                out.insert(s);
            }
        }
    }
    out
}

pub fn status() -> Result<(), String> {
    repo::ensure_repo()?;
    let cfg = config::load().map_err(|e| format!("load config: {e}"))?;
    let branch = repo::current_branch().unwrap_or_else(|| "(detached)".to_string());
    let head = repo::read_head();
    let head_short = head.as_ref().map(|h| h[..8.min(h.len())].to_string()).unwrap_or_else(|| "(no commits yet)".to_string());

    println!("On branch {branch}");
    if repo::read_merge_head().is_some() {
        println!("You have unmerged paths.");
        println!("  (fix conflicts and run `gyat commit \"msg\"`)");
    }
    match &head {
        Some(h) if !h.is_empty() => println!("HEAD: {head_short}"),
        _ => println!("No commits yet"),
    }

    let gyatignore_pats = super::gyatignore::load_patterns();
    let is_ignored = |rel: &str| -> bool {
        config::is_ignored(rel, &cfg)
            || super::gyatignore::is_ignored_by_file(rel, &gyatignore_pats)
    };

    // staged state
    let staged: Vec<String> = repo::staged_files().iter().map(|p| p.to_string_lossy().to_string()).collect();
    let staged_set: HashSet<String> = staged.iter().cloned().collect();
    let deletions = repo::staged_deletions();
    let deletion_set: HashSet<String> = deletions.iter().cloned().collect();

    if staged.is_empty() && deletions.is_empty() {
        println!("\nnothing to commit, working tree clean (staged: empty)");
    } else {
        println!("\nChanges to be committed:");
        println!("  (use `gyat commit \"msg\"` to commit)");
        let mut sorted = staged.clone();
        sorted.sort();
        for f in sorted.iter().take(20) {
            let is_new = head_bytes(f).is_none();
            if is_new {
                println!("  new file:   {f}");
            } else {
                println!("  modified:   {f}");
            }
        }
        let mut ds = deletions.clone();
        ds.sort();
        for f in ds.iter().take(20) {
            println!("  deleted:    {f}");
        }
        if sorted.len() + ds.len() > 20 {
            println!("  ... and {} more", sorted.len() + ds.len() - 20);
        }
    }

    // worktree walk: unstaged + untracked
    let tracked = head_tracked_set();
    let mut unstaged_mod: Vec<String> = vec![];
    let mut untracked: Vec<String> = vec![];
    let mut unstaged_del: Vec<String> = vec![];
    let mut work_set: HashSet<String> = HashSet::new();

    let walker = WalkDir::new(".").into_iter().filter_entry(|e| {
        let rel = normalize_rel(e.path());
        if rel.is_empty() || rel == "." {
            return true;
        }
        if rel == ".gyt" || rel.starts_with(".gyt/") {
            return false;
        }
        if e.file_type().is_dir() && is_ignored(&rel) {
            return false;
        }
        true
    });
    for entry in walker.filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let rel = normalize_rel(p);
        if rel.is_empty() || rel == "." || rel.starts_with(".gyt/") || rel == ".gyt" {
            continue;
        }
        if is_ignored(&rel) {
            continue;
        }
        work_set.insert(rel.clone());
        let work_bytes = fs::read(p).ok();
        match head_bytes(&rel) {
            None => {
                // untracked unless staged
                if !staged_set.contains(&rel) {
                    untracked.push(rel);
                }
            }
            Some(hb) => {
                let wb = work_bytes.unwrap_or_default();
                if wb != hb {
                    // differs from HEAD: staged or unstaged?
                    let staged_bytes = fs::read(repo::stages_root().join(&rel)).ok();
                    match staged_bytes {
                        Some(sb) if sb == wb => {
                            // worktree matches stage -> already shown as staged
                        }
                        _ => unstaged_mod.push(rel),
                    }
                }
            }
        }
    }
    for t in tracked.iter() {
        if is_ignored(t) {
            continue;
        }
        if !work_set.contains(t) && !deletion_set.contains(t) {
            unstaged_del.push(t.clone());
        }
    }

    if !unstaged_mod.is_empty() || !unstaged_del.is_empty() {
        println!("\nChanges not staged for commit:");
        println!("  (use `gyat add <files>` to stage)");
        let mut all: Vec<(String, &str)> = vec![];
        for f in &unstaged_mod {
            all.push((f.clone(), "modified"));
        }
        for f in &unstaged_del {
            all.push((f.clone(), "deleted"));
        }
        all.sort();
        for (f, kind) in all.iter().take(20) {
            println!("  {kind}:   {f}");
        }
        if all.len() > 20 {
            println!("  ... and {} more", all.len() - 20);
        }
    }
    if !untracked.is_empty() {
        untracked.sort();
        println!("\nUntracked files:");
        println!("  (use `gyat add <files>` to include)");
        for f in untracked.iter().take(20) {
            println!("  {f}");
        }
        if untracked.len() > 20 {
            println!("  ... and {} more", untracked.len() - 20);
        }
    }

    let commits = repo::list_commits();
    let branches = repo::list_branches();
    println!("\n--");
    println!("repo: {}  user: {}", cfg.repo.name, cfg.repo.username);
    println!("server: {}  branch: {branch} ({}/{})", cfg.repo.server, branches.len(), commits.len());
    if commits.is_empty() {
        println!("hint: `gyat add .` then `gyat commit \"init\"`");
    } else if !staged.is_empty() || !deletions.is_empty() {
        println!("hint: `gyat commit \"msg\"` to commit {} staged change(s), `gyat push` to publish", staged.len() + deletions.len());
    } else {
        println!("hint: `gyat log` for history, `gyat branch` for branches");
    }
    Ok(())
}

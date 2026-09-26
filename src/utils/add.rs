use std::collections::HashSet;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

use super::config;

/// Read the committed (HEAD) version of a file as bytes, handling .gz snapshots.
/// Returns None if the file is not tracked in HEAD.
fn head_bytes(rel: &str) -> Option<Vec<u8>> {
    let head = super::repo::read_head()?;
    if head.is_empty() {
        return None;
    }
    let snap_file = super::repo::commit_snapshot_root(&head).join(rel);
    super::compression::read_bytes_maybe_compressed(&snap_file).ok()
}

fn work_bytes(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

/// True if the worktree file differs from HEAD (or is untracked).
fn is_changed_vs_head(work_path: &Path, rel: &str) -> bool {
    match (work_bytes(work_path), head_bytes(rel)) {
        (Some(w), Some(h)) => w != h,
        (Some(_), None) => true, // untracked -> changed
        _ => false,
    }
}

fn normalize_rel(p: &Path) -> String {
    p.strip_prefix(".")
        .unwrap_or(p)
        .to_string_lossy()
        .to_string()
        .trim_start_matches('/')
        .trim_start_matches("./")
        .to_string()
}

pub fn add(files: &[String]) -> Result<(), String> {
    if files.is_empty() {
        return Err("add: no files specified".to_string());
    }
    super::repo::ensure_repo()?;
    let cfg = config::load().unwrap_or_default();
    let gyatignore_pats = super::gyatignore::load_patterns();
    let is_ignored_combined = |rel: &str| -> bool {
        config::is_ignored(rel, &cfg)
            || super::gyatignore::is_ignored_by_file(rel, &gyatignore_pats)
    };
    let stages_root = super::repo::stages_root();
    if !stages_root.exists() {
        return Err("not initialized: .gyt/stages missing".to_string());
    }

    let mut staged = 0usize;
    let mut unchanged = 0usize;
    let mut ignored = 0usize;
    let mut missing = 0usize;
    let mut deleted = 0usize;

    // Collect candidate worktree files first (so `add .` can also detect deletions after).
    let mut candidates: Vec<(std::path::PathBuf, String)> = vec![];
    let mut explicit_deleted: Vec<String> = vec![];

    for pattern in files {
        let src_path = Path::new(pattern);
        if pattern == "." {
            let walker = WalkDir::new(".").into_iter().filter_entry(|e| {
                let rel = normalize_rel(e.path());
                if rel.is_empty() || rel == "." {
                    return true;
                }
                if rel == ".gyt" || rel.starts_with(".gyt/") {
                    return false;
                }
                if e.file_type().is_dir() && is_ignored_combined(&rel) {
                    return false;
                }
                true
            });
            for entry in walker.filter_map(|e| e.ok()) {
                let p = entry.path().to_path_buf();
                let rel = normalize_rel(&p);
                if rel.is_empty() || rel == "." {
                    continue;
                }
                if rel.starts_with(".gyt/") || rel == ".gyt" {
                    continue;
                }
                if is_ignored_combined(&rel) {
                    ignored += 1;
                    continue;
                }
                if p.is_file() {
                    candidates.push((p, rel));
                }
            }
            continue;
        }

        if !src_path.exists() {
            // Missing path: if it was tracked in HEAD, stage it as a deletion (git add . semantics).
            let rel = pattern.trim_start_matches("./").trim_start_matches('/').to_string();
            if head_bytes(&rel).is_some() {
                super::repo::record_deletion(&rel)?;
                deleted += 1;
                println!("deleted: {rel}");
            } else {
                eprintln!("add: file not found {pattern}");
                missing += 1;
            }
            continue;
        }

        if src_path.is_dir() {
            let walker = WalkDir::new(src_path).into_iter().filter_entry(|e| {
                let rel = normalize_rel(e.path());
                if rel.is_empty() {
                    return true;
                }
                if e.file_type().is_dir() && is_ignored_combined(&rel) {
                    return false;
                }
                true
            });
            for entry in walker.filter_map(|e| e.ok()) {
                let p = entry.path().to_path_buf();
                if p.is_file() {
                    let rel = normalize_rel(&p);
                    if is_ignored_combined(&rel) {
                        ignored += 1;
                        continue;
                    }
                    candidates.push((p, rel));
                }
            }
        } else {
            let rel = pattern.trim_start_matches("./").trim_start_matches('/').to_string();
            if is_ignored_combined(&rel) {
                println!("ignored {rel}");
                ignored += 1;
                continue;
            }
            candidates.push((src_path.to_path_buf(), rel));
        }
    }

    // De-duplicate candidates (e.g. `add . a.txt`).
    let mut seen: HashSet<String> = HashSet::new();
    for (work_path, rel) in candidates {
        if !seen.insert(rel.clone()) {
            continue;
        }
        if !work_path.exists() {
            explicit_deleted.push(rel);
            continue;
        }
        if !is_changed_vs_head(&work_path, &rel) {
            unchanged += 1;
            // if it was previously marked deleted but now exists+same as HEAD, unmark
            let _ = super::repo::unrecord_deletion(&rel);
            continue;
        }
        // changed or new: (re)stage and clear any deletion mark
        let _ = super::repo::unrecord_deletion(&rel);
        stage_file(&work_path, &stages_root, &rel)?;
        staged += 1;
        println!("staged {rel}");
    }
    for rel in explicit_deleted {
        if head_bytes(&rel).is_some() {
            super::repo::record_deletion(&rel)?;
            deleted += 1;
            println!("deleted: {rel}");
        }
    }

    // `add .` also stages deletions: tracked files missing from worktree.
    let wants_dot = files.iter().any(|f| f == ".");
    if wants_dot {
        if let Some(head) = super::repo::read_head() {
            let snap_root = super::repo::commit_snapshot_root(&head);
            if snap_root.exists() {
                let mut tracked: Vec<String> = vec![];
                for entry in WalkDir::new(&snap_root).into_iter().filter_map(|e| e.ok()) {
                    let p = entry.path();
                    if p.is_file() {
                        if let Ok(rel) = p.strip_prefix(&snap_root) {
                            let mut s = rel.to_string_lossy().to_string();
                            if s.ends_with(".gz") {
                                s.truncate(s.len() - 3);
                            }
                            tracked.push(s);
                        }
                    }
                }
                // worktree rel set for existence check
                let work_set: HashSet<String> = {
                    let mut s = HashSet::new();
                    let walker = WalkDir::new(".").into_iter().filter_entry(|e| {
                        let rel = normalize_rel(e.path());
                        if rel == ".gyt" || rel.starts_with(".gyt/") {
                            return false;
                        }
                        if e.file_type().is_dir() && is_ignored_combined(&rel) {
                            return false;
                        }
                        true
                    });
                    for entry in walker.filter_map(|e| e.ok()) {
                        if entry.path().is_file() {
                            s.insert(normalize_rel(entry.path()));
                        }
                    }
                    s
                };
                for t in tracked {
                    if is_ignored_combined(&t) {
                        continue;
                    }
                    if !work_set.contains(&t) && !super::repo::staged_deletions().contains(&t) {
                        super::repo::record_deletion(&t)?;
                        deleted += 1;
                        println!("deleted: {t}");
                    }
                }
            }
        }
    }

    if staged == 0 && deleted == 0 {
        if missing > 0 && unchanged == 0 {
            return Err("nothing staged".to_string());
        }
        println!("everything up to date ({unchanged} unchanged, {ignored} ignored)");
        println!("hint: `gyat status` to review");
        return Ok(());
    }
    if missing > 0 {
        println!("warning: {missing} path(s) not found, skipped");
    }
    if ignored > 0 {
        println!("({ignored} ignored via .gyatignore)");
    }
    if unchanged > 0 {
        println!("({unchanged} unchanged, skipped)");
    }
    let mut parts = vec![];
    if staged > 0 {
        parts.push(format!("{staged} staged"));
    }
    if deleted > 0 {
        parts.push(format!("{deleted} deleted"));
    }
    println!("{} for commit", parts.join(", "));
    println!("hint: `gyat status` to review, `gyat commit \"msg\"` to commit");
    Ok(())
}

fn stage_file(src: &Path, stages_root: &Path, rel: &str) -> Result<(), String> {
    let dest = stages_root.join(rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }
    fs::copy(src, &dest).map_err(|e| format!("stage {rel}: {e}"))?;
    Ok(())
}

use std::fs;
use std::path::Path;
use super::repo;

pub fn travel(commit_or_branch: &str) -> Result<(), String> {
    repo::ensure_repo()?;
    if repo::branch_exists(commit_or_branch) {
        let hash = repo::read_branch(commit_or_branch).ok_or(format!("branch {commit_or_branch} has no commits yet"))?;
        if hash.is_empty() {
            repo::set_head_branch(commit_or_branch)?;
            println!("switched to branch {commit_or_branch} (no commits yet)");
            return Ok(());
        }
        let prev = repo::read_head();
        repo::set_head_branch(commit_or_branch)?;
        let target = hash;
        let snap = repo::commit_snapshot_root(&target);
        if !snap.exists() { return Err(format!("branch {commit_or_branch} commit {target} snapshot missing")); }
        return restore_snapshot_with_prev(prev.as_deref(), &target, &snap, &format!("switched to branch {commit_or_branch}"));
    }
    let target = resolve_commit(commit_or_branch)?;
    let snap = repo::commit_snapshot_root(&target);
    if !snap.exists() { return Err(format!("commit {target} snapshot missing")); }
    let prev = repo::read_head();
    fs::write(repo::head_path(), &target).map_err(|e| format!("detach HEAD: {e}"))?;
    let files = collect_snapshot_files(&snap);
    sync_delete_extra(prev.as_deref(), &snap);
    for rel in &files {
        let src_gz = snap.join(format!("{}.gz", rel.display()));
        let src_plain = snap.join(rel);
        let src = if src_gz.exists() { src_gz } else { src_plain };
        let dst = Path::new(".").join(rel);
        if let Some(p) = dst.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir {p:?}: {e}"))?; }
        let res = if src.extension().and_then(|s| s.to_str()) == Some("gz") {
            super::compression::decompress_file(&src, &dst)
        } else {
            fs::copy(&src, &dst).map(|_| ()).map_err(|e| format!("restore {rel:?}: {e}"))
        };
        match res {
            Ok(_) => println!("restored {}", rel.display()),
            Err(e) if e.contains("Text file busy") => eprintln!("warning: skip busy {}", rel.display()),
            Err(e) => return Err(e),
        }
    }
    let current = repo::current_root();
    let _ = fs::remove_dir_all(&current);
    fs::create_dir_all(&current).map_err(|e| format!("create current: {e}"))?;
    for rel in &files {
        let src_gz = snap.join(format!("{}.gz", rel.display()));
        let src_plain = snap.join(rel);
        let src = if src_gz.exists() { src_gz } else { src_plain };
        let dst = current.join(rel);
        if let Some(p) = dst.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir current {p:?}: {e}"))?; }
        if src.extension().and_then(|s| s.to_str()) == Some("gz") {
            let _ = super::compression::decompress_file(&src, &dst);
        } else {
            let _ = fs::copy(src, dst);
        }
    }
    let _ = fs::remove_dir_all(repo::stages_root());
    fs::create_dir_all(repo::stages_root()).map_err(|e| format!("recreate stages: {e}"))?;
    println!("traveled to {target} (detached HEAD)");
    Ok(())
}

fn restore_snapshot_with_prev(prev: Option<&str>, target: &str, snap: &Path, msg: &str) -> Result<(), String> {
    sync_delete_extra(prev, snap);
    let files = collect_snapshot_files(snap);
    for rel in &files {
        let src_gz = snap.join(format!("{}.gz", rel.display()));
        let src_plain = snap.join(rel);
        let src = if src_gz.exists() { src_gz } else { src_plain };
        let dst = Path::new(".").join(rel);
        if let Some(p) = dst.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir {p:?}: {e}"))?; }
        let res = if src.extension().and_then(|s| s.to_str()) == Some("gz") {
            super::compression::decompress_file(&src, &dst)
        } else {
            fs::copy(&src, &dst).map(|_| ()).map_err(|e| format!("restore {rel:?}: {e}"))
        };
        match res {
            Ok(_) => println!("restored {}", rel.display()),
            Err(e) if e.contains("Text file busy") => eprintln!("warning: skip busy {}", rel.display()),
            Err(e) => return Err(e),
        }
    }
    let current = repo::current_root();
    let _ = fs::remove_dir_all(&current);
    fs::create_dir_all(&current).map_err(|e| format!("create current: {e}"))?;
    for rel in &files {
        let src_gz = snap.join(format!("{}.gz", rel.display()));
        let src_plain = snap.join(rel);
        let src = if src_gz.exists() { src_gz } else { src_plain };
        let dst = current.join(rel);
        if let Some(p) = dst.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir current {p:?}: {e}"))?; }
        if src.extension().and_then(|s| s.to_str()) == Some("gz") {
            let _ = super::compression::decompress_file(&src, &dst);
        } else {
            let _ = fs::copy(src, dst);
        }
    }
    let _ = fs::remove_dir_all(repo::stages_root());
    fs::create_dir_all(repo::stages_root()).map_err(|e| format!("recreate stages: {e}"))?;
    println!("{msg} -> {target}");
    Ok(())
}

fn sync_delete_extra(prev: Option<&str>, target_snap: &Path) {
    let Some(prev_hash) = prev else { return; };
    if prev_hash.is_empty() { return; }
    let prev_snap = repo::commit_snapshot_root(prev_hash);
    if !prev_snap.exists() { return; }
    let prev_files = collect_snapshot_files(&prev_snap);
    let target_files = collect_snapshot_files(target_snap);
    let target_set: std::collections::HashSet<_> = target_files.iter().collect();
    for rel in prev_files {
        if !target_set.contains(&rel) {
            let ws = Path::new(".").join(&rel);
            if ws.exists() {
                let _ = fs::remove_file(&ws);
                println!("deleted {}", rel.display());
                if let Some(p) = ws.parent() { let _ = fs::remove_dir(p); }
            }
        }
    }
}

fn collect_snapshot_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    collect(root, root, &mut out);
    out.into_iter().map(|p| {
        let s = p.to_string_lossy().to_string();
        if s.ends_with(".gz") {
            let trimmed = s.trim_end_matches(".gz");
            std::path::PathBuf::from(trimmed)
        } else { p }
    }).collect()
}
fn collect(base: &Path, dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() { collect(base, &p, out); } else if let Ok(rel) = p.strip_prefix(base) { out.push(rel.to_path_buf()); }
        }
    }
}

fn resolve_commit(prefix: &str) -> Result<String, String> {
    if prefix.len() >= 16 {
        if repo::commit_path(prefix).exists() { return Ok(prefix.to_string()); }
    }
    let commits = repo::list_commits();
    let mut matches: Vec<String> = commits.into_iter().filter(|h| h.starts_with(prefix)).collect();
    if matches.is_empty() { return Err(format!("commit {prefix} not found")); }
    if matches.len() > 1 { return Err(format!("ambiguous commit prefix {prefix}: {:?}", matches)); }
    Ok(matches.remove(0))
}

use std::fs;
use std::path::Path;
use super::repo;

pub fn travel(commit: &str) -> Result<(), String> {
    repo::ensure_repo()?;
    // support short hash prefix
    let target = resolve_commit(commit)?;
    let snap = repo::commit_snapshot_root(&target);
    if !snap.exists() {
        return Err(format!("commit {target} snapshot missing"));
    }
    // restore working tree: copy snapshot files to workspace root (excluding .gyt, target, .git)
    // For safety, we only restore files that were in snapshot; we don't delete untracked files
    let files = collect_snapshot_files(&snap);
    for rel in &files {
        let src = snap.join(rel);
        let dst = Path::new(".").join(rel);
        if let Some(p) = dst.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir {p:?}: {e}"))?; }
        match fs::copy(&src, &dst) {
            Ok(_) => println!("restored {}", rel.display()),
            Err(e) if e.raw_os_error() == Some(26) => eprintln!("warning: skip busy {}", rel.display()),
            Err(e) => return Err(format!("restore {rel:?}: {e}")),
        }
    }
    repo::write_head(&target)?;
    // also update current mirror
    let current = repo::current_root();
    let _ = fs::remove_dir_all(&current);
    fs::create_dir_all(&current).map_err(|e| format!("create current: {e}"))?;
    for rel in &files {
        let src = snap.join(rel);
        let dst = current.join(rel);
        if let Some(p) = dst.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir current {p:?}: {e}"))?; }
        let _ = fs::copy(src, dst);
    }
    // clear stages
    let _ = fs::remove_dir_all(repo::stages_root());
    fs::create_dir_all(repo::stages_root()).map_err(|e| format!("recreate stages: {e}"))?;

    println!("traveled to {target}");
    Ok(())
}

fn collect_snapshot_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    collect(root, root, &mut out);
    out
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
        // assume full hash, check exists
        if repo::commit_path(prefix).exists() { return Ok(prefix.to_string()); }
    }
    // prefix search
    let commits = repo::list_commits();
    let mut matches: Vec<String> = commits.into_iter().filter(|h| h.starts_with(prefix)).collect();
    if matches.is_empty() {
        return Err(format!("commit {prefix} not found"));
    }
    if matches.len() > 1 {
        return Err(format!("ambiguous commit prefix {prefix}: {:?}", matches));
    }
    Ok(matches.remove(0))
}

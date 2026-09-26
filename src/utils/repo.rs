use std::fs;
use std::path::{Path, PathBuf};

pub fn gyt_root() -> &'static Path { Path::new(".gyt") }
pub fn stages_root() -> PathBuf { gyt_root().join("stages") }
pub fn commits_root() -> PathBuf { gyt_root().join("commits") }
pub fn head_path() -> PathBuf { gyt_root().join("HEAD") }
pub fn current_root() -> PathBuf { gyt_root().join("current") }
pub fn refs_heads_root() -> PathBuf { gyt_root().join("refs/heads") }
pub fn branch_path(name: &str) -> PathBuf { refs_heads_root().join(name) }

pub fn ensure_repo() -> Result<(), String> {
    if !gyt_root().exists() {
        return Err("not a gyat repo: .gyt missing. run `gyat init`".to_string());
    }
    Ok(())
}

pub fn read_head() -> Option<String> {
    let s = fs::read_to_string(head_path()).ok()?.trim().to_string();
    if s.is_empty() { return None; }
    if s.starts_with("ref: ") {
        let ref_path = s.trim_start_matches("ref: ").trim();
        let branch_file = gyt_root().join(ref_path);
        if let Ok(h) = fs::read_to_string(branch_file) {
            let h = h.trim().to_string();
            if !h.is_empty() { return Some(h); }
        }
        return None;
    }
    Some(s)
}

pub fn write_head(hash: &str) -> Result<(), String> {
    let head_content = fs::read_to_string(head_path()).unwrap_or_default();
    if head_content.starts_with("ref: ") {
        let ref_path = head_content.trim_start_matches("ref: ").trim().to_string();
        let branch_file = gyt_root().join(&ref_path);
        if let Some(p) = branch_file.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir refs: {e}"))?; }
        fs::write(branch_file, hash).map_err(|e| format!("write branch {ref_path}: {e}"))?;
        Ok(())
    } else {
        fs::write(head_path(), hash).map_err(|e| format!("write HEAD: {e}"))
    }
}

pub fn current_branch() -> Option<String> {
    let s = fs::read_to_string(head_path()).ok()?.trim().to_string();
    if s.starts_with("ref: ") {
        let ref_path = s.trim_start_matches("ref: ").trim();
        if let Some(name) = ref_path.strip_prefix("refs/heads/") {
            return Some(name.to_string());
        }
    }
    None
}

pub fn set_head_branch(branch: &str) -> Result<(), String> {
    let ref_str = format!("ref: refs/heads/{branch}");
    fs::write(head_path(), ref_str).map_err(|e| format!("write HEAD: {e}"))
}

#[allow(dead_code)]
pub fn is_detached() -> bool {
    if let Ok(s) = fs::read_to_string(head_path()) {
        !s.trim().starts_with("ref: ")
    } else { true }
}

pub fn list_branches() -> Vec<String> {
    let root = refs_heads_root();
    if !root.exists() { return vec![]; }
    let mut v = vec![];
    if let Ok(entries) = fs::read_dir(&root) {
        for e in entries.flatten() {
            if e.path().is_file() {
                if let Some(name) = e.file_name().to_str() { v.push(name.to_string()); }
            }
        }
    }
    v.sort();
    v
}

pub fn branch_exists(name: &str) -> bool { branch_path(name).exists() }

pub fn read_branch(name: &str) -> Option<String> {
    fs::read_to_string(branch_path(name)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn write_branch(name: &str, hash: &str) -> Result<(), String> {
    let p = branch_path(name);
    if let Some(parent) = p.parent() { fs::create_dir_all(parent).map_err(|e| format!("mkdir refs: {e}"))?; }
    fs::write(p, hash).map_err(|e| format!("write branch {name}: {e}"))
}

pub fn merge_head_path() -> PathBuf { gyt_root().join("MERGE_HEAD") }
pub fn read_merge_head() -> Option<String> {
    fs::read_to_string(merge_head_path()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
pub fn write_merge_head(hash: &str) -> Result<(), String> {
    fs::write(merge_head_path(), hash).map_err(|e| format!("write MERGE_HEAD: {e}"))
}
pub fn clear_merge_head() -> Result<(), String> {
    let p = merge_head_path();
    if p.exists() { fs::remove_file(p).map_err(|e| format!("clear MERGE_HEAD: {e}"))?; }
    Ok(())
}

pub fn list_commits() -> Vec<String> {
    let root = commits_root();
    if !root.exists() { return vec![]; }
    let mut v = vec![];
    if let Ok(entries) = fs::read_dir(&root) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    if name.len() >= 6 { v.push(name.to_string()); }
                }
            }
        }
    }
    v.sort();
    v
}

pub fn commit_path(hash: &str) -> PathBuf { commits_root().join(hash) }
pub fn commit_meta_path(hash: &str) -> PathBuf { commit_path(hash).join("meta.toml") }
pub fn commit_snapshot_root(hash: &str) -> PathBuf { commit_path(hash).join("snapshot") }
pub fn commit_deltas_root(hash: &str) -> PathBuf { commit_path(hash).join("deltas") }

pub fn deletions_file() -> PathBuf { stages_root().join(".gyat-deleted") }

pub fn staged_deletions() -> Vec<String> {
    let p = deletions_file();
    if !p.exists() { return vec![]; }
    fs::read_to_string(&p)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

pub fn record_deletion(rel: &str) -> Result<(), String> {
    let mut cur = staged_deletions();
    if !cur.iter().any(|x| x == rel) {
        cur.push(rel.to_string());
        cur.sort();
        fs::write(deletions_file(), cur.join("\n") + "\n")
            .map_err(|e| format!("record deletion {rel}: {e}"))?;
    }
    // make sure a stale staged copy doesn't linger
    let staged_copy = stages_root().join(rel);
    if staged_copy.exists() {
        let _ = fs::remove_file(&staged_copy);
    }
    Ok(())
}

pub fn unrecord_deletion(rel: &str) -> Result<(), String> {
    let cur: Vec<String> = staged_deletions().into_iter().filter(|x| x != rel).collect();
    if cur.is_empty() {
        let _ = fs::remove_file(deletions_file());
    } else {
        fs::write(deletions_file(), cur.join("\n") + "\n")
            .map_err(|e| format!("unrecord deletion {rel}: {e}"))?;
    }
    Ok(())
}

pub fn staged_files() -> Vec<PathBuf> {
    let root = stages_root();
    let mut out = vec![];
    if !root.exists() { return out; }
    collect_files(&root, &root, &mut out);
    out.into_iter()
        .filter(|p| {
            let s = p.to_string_lossy();
            s != ".gyat-deleted" && !s.starts_with(".gyat-deleted/")
        })
        .collect()
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() { collect_files(base, &p, out); } else if p.is_file() { if let Ok(rel) = p.strip_prefix(base) { out.push(rel.to_path_buf()); } }
        }
    }
}

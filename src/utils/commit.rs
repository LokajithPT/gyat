use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::repo;
use delta::{myers_diff, Delta};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommitMeta {
    pub hash: String,
    pub parent: Option<String>,
    pub message: String,
    pub author: String,
    pub timestamp: u64,
    pub files: Vec<String>,
}

fn now_ts() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn hash_commit(parent: &Option<String>, message: &str, author: &str, ts: u64, files: &[String], content_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(parent.as_deref().unwrap_or("").as_bytes());
    hasher.update(message.as_bytes());
    hasher.update(author.as_bytes());
    hasher.update(ts.to_be_bytes());
    for f in files { hasher.update(f.as_bytes()); }
    hasher.update(content_hash.as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

fn file_content_hash(path: &Path) -> String {
    if let Ok(bytes) = fs::read(path) {
        let mut h = Sha256::new();
        h.update(&bytes);
        hex::encode(&h.finalize()[..8])
    } else {
        String::new()
    }
}

fn read_lines_for_delta(path: &Path) -> Vec<String> {
    if !path.exists() { return vec![]; }
    if let Ok(s) = fs::read_to_string(path) {
        s.lines().map(|l| l.to_string()).collect()
    } else {
        vec![]
    }
}

fn collect_files_set(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_set(root, root, &mut out);
    out
}
fn collect_set(base: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_set(base, &p, out);
            } else if let Ok(rel) = p.strip_prefix(base) {
                out.insert(rel.to_string_lossy().to_string());
            }
        }
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {dst:?}: {e}"))?;
    for entry in walkdir::WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            let rel = p.strip_prefix(src).unwrap();
            let target = dst.join(rel);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
            }
            fs::copy(p, &target).map_err(|e| format!("copy {rel:?}: {e}"))?;
        }
    }
    Ok(())
}

pub fn create_commit(message: String) -> Result<String, String> {
    super::repo::ensure_repo()?;
    let staged = repo::staged_files();
    if staged.is_empty() {
        return Err("nothing to commit: stages empty. `gyat add <files>` first".to_string());
    }

    let author = super::config::load().map(|c| c.repo.username).unwrap_or_else(|_| "unknown".to_string());
    let parent = repo::read_head();
    let ts = now_ts();

    // all_files = parent files ∪ staged files
    let mut all_files: BTreeSet<String> = staged.iter().map(|p| p.to_string_lossy().to_string()).collect();
    if let Some(parent_hash) = &parent {
        let parent_snap = repo::commit_snapshot_root(parent_hash);
        if parent_snap.exists() {
            for f in collect_files_set(&parent_snap) {
                all_files.insert(f);
            }
        }
    }
    let files: Vec<String> = all_files.iter().cloned().collect();

    // content hash over staged contents (parent already via its hash)
    let mut content_combined = String::new();
    for rel in &staged {
        let p = repo::stages_root().join(rel);
        content_combined.push_str(&file_content_hash(&p));
    }
    // also include parent hash to avoid collision on same staged content with different parent
    let hash = hash_commit(&parent, &message, &author, ts, &files, &content_combined);

    let commit_dir = repo::commit_path(&hash);
    if commit_dir.exists() {
        return Err(format!("commit {hash} already exists (retry with different message/time)"));
    }
    let snap_root = repo::commit_snapshot_root(&hash);
    let deltas_root = repo::commit_deltas_root(&hash);
    fs::create_dir_all(&snap_root).map_err(|e| format!("create snapshot dir: {e}"))?;
    fs::create_dir_all(&deltas_root).map_err(|e| format!("create deltas dir: {e}"))?;

    // 1) copy parent snapshot as base
    if let Some(parent_hash) = &parent {
        let parent_snap = repo::commit_snapshot_root(parent_hash);
        if parent_snap.exists() {
            copy_dir(&parent_snap, &snap_root)?;
        }
    }

    // 2) overlay staged files and compute deltas
    for rel in &staged {
        let src = repo::stages_root().join(rel);
        let dst = snap_root.join(rel);
        if let Some(parent_dir) = dst.parent() { fs::create_dir_all(parent_dir).map_err(|e| format!("mkdir {parent_dir:?}: {e}"))?; }
        fs::copy(&src, &dst).map_err(|e| format!("copy {rel:?}: {e}"))?;

        if let Some(parent_hash) = &parent {
            let parent_file = repo::commit_snapshot_root(parent_hash).join(rel);
            let old_lines = read_lines_for_delta(&parent_file);
            let new_lines = read_lines_for_delta(&src);
            if old_lines != new_lines {
                let deltas = myers_diff(&old_lines, &new_lines);
                if !deltas.is_empty() {
                    let delta_path = deltas_root.join(format!("{}.delta", rel.to_string_lossy().replace('/', "__")));
                    if let Some(p) = delta_path.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir delta: {e}"))?; }
                    let mut out = String::new();
                    for d in deltas {
                        match d {
                            Delta::Insert { line, text } => out.push_str(&format!("insert::{}::{}\n", line, text)),
                            Delta::Delete { line } => out.push_str(&format!("delete::{}\n", line)),
                        }
                    }
                    fs::write(&delta_path, out).map_err(|e| format!("write delta {delta_path:?}: {e}"))?;
                }
            }
        }
    }

    let meta = CommitMeta {
        hash: hash.clone(),
        parent: parent.clone(),
        message: message.clone(),
        author,
        timestamp: ts,
        files: files.clone(),
    };
    let meta_str = toml::to_string_pretty(&meta).map_err(|e| format!("serialize meta: {e}"))?;
    fs::write(repo::commit_meta_path(&hash), meta_str).map_err(|e| format!("write meta: {e}"))?;

    repo::write_head(&hash)?;

    // update current mirror to full snapshot
    let current = repo::current_root();
    let _ = fs::remove_dir_all(&current);
    copy_dir(&snap_root, &current).map_err(|e| format!("update current: {e}"))?;

    // clear stages
    let _ = fs::remove_dir_all(repo::stages_root());
    fs::create_dir_all(repo::stages_root()).map_err(|e| format!("recreate stages: {e}"))?;

    Ok(hash)
}

pub fn load_meta(hash: &str) -> Result<CommitMeta, String> {
    let p = repo::commit_meta_path(hash);
    let s = fs::read_to_string(&p).map_err(|e| format!("read meta {hash}: {e}"))?;
    toml::from_str(&s).map_err(|e| format!("parse meta {hash}: {e}"))
}

pub fn list_metas() -> Vec<CommitMeta> {
    let mut out = vec![];
    for h in repo::list_commits() {
        if let Ok(m) = load_meta(&h) { out.push(m); }
    }
    out.sort_by_key(|m| m.timestamp);
    out
}

use std::fs;
use std::path::{Path, PathBuf};

pub fn gyt_root() -> &'static Path { Path::new(".gyt") }
pub fn stages_root() -> PathBuf { gyt_root().join("stages") }
pub fn commits_root() -> PathBuf { gyt_root().join("commits") }
pub fn head_path() -> PathBuf { gyt_root().join("HEAD") }
pub fn current_root() -> PathBuf { gyt_root().join("current") }

pub fn ensure_repo() -> Result<(), String> {
    if !gyt_root().exists() {
        return Err("not a gyat repo: .gyt missing. run `gyat init`".to_string());
    }
    Ok(())
}

pub fn read_head() -> Option<String> {
    fs::read_to_string(head_path()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn write_head(hash: &str) -> Result<(), String> {
    fs::write(head_path(), hash).map_err(|e| format!("write HEAD: {e}"))
}

pub fn list_commits() -> Vec<String> {
    let root = commits_root();
    if !root.exists() { return vec![]; }
    let mut v = vec![];
    if let Ok(entries) = fs::read_dir(&root) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    // commit dirs are hex hashes (maybe 16 chars), skip non-hex? but just collect
                    if name.len() >= 6 {
                        v.push(name.to_string());
                    }
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

pub fn staged_files() -> Vec<PathBuf> {
    let root = stages_root();
    let mut out = vec![];
    if !root.exists() { return out; }
    collect_files(&root, &root, &mut out);
    out
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_files(base, &p, out);
            } else if p.is_file() {
                if let Ok(rel) = p.strip_prefix(base) {
                    out.push(rel.to_path_buf());
                }
            }
        }
    }
}

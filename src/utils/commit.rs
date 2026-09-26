use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::repo;
use super::delta::{myers_diff, Delta};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommitMeta {
    pub hash: String,
    pub parent: Option<String>,
    #[serde(default)]
    pub second_parent: Option<String>,
    pub message: String,
    pub author: String,
    pub timestamp: u64,
    pub files: Vec<String>,
}

fn now_ts() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn hash_commit(parent: &Option<String>, second: &Option<String>, message: &str, author: &str, ts: u64, files: &[String], content_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(parent.as_deref().unwrap_or("").as_bytes());
    hasher.update(second.as_deref().unwrap_or("").as_bytes());
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
    let gz_path = std::path::PathBuf::from(format!("{}.gz", path.display()));
    let actual = if path.exists() { path.to_path_buf() } else if gz_path.exists() { gz_path } else { return vec![]; };
    if actual.extension().and_then(|s| s.to_str()) == Some("gz") {
        if let Ok(s) = super::compression::read_text_maybe_compressed(&actual) {
            return s.lines().map(|l| l.to_string()).collect();
        }
        return vec![];
    }
    if let Ok(s) = fs::read_to_string(&actual) {
        s.lines().map(|l| l.to_string()).collect()
    } else {
        vec![]
    }
}

fn snapshot_path_for(snap_root: &Path, rel: &Path, settings: &super::settings::GyatSettings) -> std::path::PathBuf {
    let base = snap_root.join(rel);
    if super::compression::should_compress(settings) {
        std::path::PathBuf::from(format!("{}.gz", base.display()))
    } else {
        base
    }
}

fn delta_path_for(deltas_root: &Path, rel: &Path, settings: &super::settings::GyatSettings) -> std::path::PathBuf {
    let base_name = format!("{}.delta", rel.to_string_lossy().replace('/', "__"));
    let base = deltas_root.join(base_name);
    if super::compression::should_compress(settings) {
        std::path::PathBuf::from(format!("{}.gz", base.display()))
    } else {
        base
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
                let mut s = rel.to_string_lossy().to_string();
                if s.ends_with(".gz") { s.truncate(s.len() - 3); }
                out.insert(s);
            }
        }
    }
}

fn copy_dir_compressed(src: &Path, dst: &Path, _settings: &super::settings::GyatSettings) -> Result<(), String> {
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

#[allow(dead_code)]
pub fn create_commit(message: String) -> Result<String, String> {
    create_commit_with_parents(message, None)
}

pub fn commit_with_message(message: String) -> Result<(), String> {
    super::repo::ensure_repo()?;
    let staged = repo::staged_files();
    let staged_deletions = repo::staged_deletions();
    if staged.is_empty() && staged_deletions.is_empty() {
        let branch = repo::current_branch().unwrap_or_else(|| "(detached)".to_string());
        return Err(format!("On branch {branch}\nnothing to commit, working tree clean\n(hint: `gyat add <files>` to stage)"));
    }
    let branch = repo::current_branch().unwrap_or_else(|| "(detached)".to_string());
    let parent = repo::read_head();
    let merge_parent = repo::read_merge_head().or(None);
    let is_merge = merge_parent.is_some();

    // per-file stats vs parent
    let mut per_file: Vec<(String, usize, usize, bool)> = vec![];
    let mut total_ins = 0usize;
    let mut total_del = 0usize;
    for rel in &staged {
        let rel_str = rel.to_string_lossy().to_string();
        let parent_file = parent.as_ref().map(|h| repo::commit_snapshot_root(h).join(rel)).unwrap_or_else(|| Path::new("__none__").to_path_buf());
        let old_lines = if parent.as_ref().is_some() { read_lines_for_delta(&parent_file) } else { vec![] };
        let new_lines = read_lines_for_delta(&repo::stages_root().join(rel));
        let existed_before = parent.as_ref().map(|_| parent_file.exists() || std::path::PathBuf::from(format!("{}.gz", parent_file.display())).exists()).unwrap_or(false);
        if old_lines == new_lines {
            continue;
        }
        let deltas = myers_diff(&old_lines, &new_lines);
        let mut ins = 0usize;
        let mut del = 0usize;
        for d in &deltas {
            match d {
                Delta::Insert { .. } => ins += 1,
                Delta::Delete { .. } => del += 1,
            }
        }
        total_ins += ins;
        total_del += del;
        per_file.push((rel_str, ins, del, !existed_before));
    }
    for d in &staged_deletions {
        let old_lines = parent.as_ref().map(|h| read_lines_for_delta(&repo::commit_snapshot_root(h).join(d))).unwrap_or_default();
        total_del += old_lines.len().max(1);
        per_file.push((d.clone(), 0, old_lines.len(), false));
    }
    per_file.sort_by(|a, b| a.0.cmp(&b.0));

    let hash = create_commit_with_parents(message.clone(), None)?;
    let short = &hash[..8.min(hash.len())];

    // git-like robust output
    if is_merge {
        println!("[{branch} {short}] Merge: {message}");
    } else if parent.is_none() {
        println!("[{branch} (root-commit) {short}] {message}");
    } else {
        println!("[{branch} {short}] {message}");
    }
    let n_files = per_file.len();
    let file_word = if n_files == 1 { "file" } else { "files" };
    if n_files == 0 {
        println!(" 0 files changed");
    } else {
        let mut summary = format!(" {n_files} {file_word} changed");
        if total_ins > 0 { summary.push_str(&format!(", {total_ins} insertion{} (+)", if total_ins == 1 { "" } else { "s" })); }
        if total_del > 0 { summary.push_str(&format!(", {total_del} deletion{} (-)", if total_del == 1 { "" } else { "s" })); }
        println!("{summary}");
        for (path, ins, del, is_new) in &per_file {
            if staged_deletions.iter().any(|d| d == path) {
                println!(" delete mode: {path} (-{del})");
            } else if *is_new {
                println!(" create mode: {path} (+{ins})");
            } else if *ins == 0 && *del == 0 {
                println!(" modify: {path}");
            } else {
                let mut parts = vec![];
                if *ins > 0 { parts.push(format!("+{ins}")); }
                if *del > 0 { parts.push(format!("-{del}")); }
                println!(" modify: {path} ({})", parts.join(" "));
            }
        }
    }
    // contextual hints
    if repo::read_merge_head().is_some() {
        // still in merge (should have been cleared, but just in case)
        println!("hint: merge in progress (.gyt/MERGE_HEAD present)");
    } else {
        println!("hint: `gyat log --oneline` to see history, `gyat push` to publish `{branch}`");
    }
    Ok(())
}

pub fn create_commit_with_parents(message: String, second_parent: Option<String>) -> Result<String, String> {
    super::repo::ensure_repo()?;
    // if MERGE_HEAD exists and no explicit second_parent, use it (merge resolution)
    let mut second_parent = second_parent;
    if second_parent.is_none() {
        if let Some(mh) = repo::read_merge_head() { second_parent = Some(mh); }
    }
    let staged = repo::staged_files();
    let deletions = repo::staged_deletions();
    if staged.is_empty() && deletions.is_empty() {
        return Err("nothing to commit, working tree clean\n(hint: `gyat add <files>` to stage changes)".to_string());
    }
    // for merge commits, staged may be empty if auto-merged without conflicts and already staged via merge
    let author = super::config::load().map(|c| c.repo.username).unwrap_or_else(|_| "unknown".to_string());
    let parent = repo::read_head();
    let ts = now_ts();

    let deleted_set: std::collections::HashSet<String> = deletions.iter().cloned().collect();
    let mut all_files: BTreeSet<String> = staged.iter().map(|p| p.to_string_lossy().to_string()).collect();
    if let Some(parent_hash) = &parent {
        let parent_snap = repo::commit_snapshot_root(parent_hash);
        if parent_snap.exists() {
            for f in collect_files_set(&parent_snap) {
                if !deleted_set.contains(&f) {
                    all_files.insert(f);
                }
            }
        }
    }
    // also include second parent files for merge
    if let Some(sp) = &second_parent {
        let sp_snap = repo::commit_snapshot_root(sp);
        if sp_snap.exists() {
            for f in collect_files_set(&sp_snap) { all_files.insert(f); }
        }
    }
    let files: Vec<String> = all_files.iter().cloned().collect();
    let settings = super::settings::load();
    let mut content_combined = String::new();
    for rel in &staged {
        let p = repo::stages_root().join(rel);
        content_combined.push_str(&file_content_hash(&p));
    }
    for d in &deletions {
        content_combined.push_str("deleted:");
        content_combined.push_str(d);
    }
    content_combined.push_str(if settings.compression.enabled { "compressed" } else { "raw" });
    content_combined.push_str(&settings.chunks.size.to_string());
    let hash = hash_commit(&parent, &second_parent, &message, &author, ts, &files, &content_combined);

    let commit_dir = repo::commit_path(&hash);
    if commit_dir.exists() { return Err(format!("commit {hash} already exists")); }
    let snap_root = repo::commit_snapshot_root(&hash);
    let deltas_root = repo::commit_deltas_root(&hash);
    fs::create_dir_all(&snap_root).map_err(|e| format!("create snapshot dir: {e}"))?;
    fs::create_dir_all(&deltas_root).map_err(|e| format!("create deltas dir: {e}"))?;

    // for merge commits, snapshot is exactly staged files (already contains merged result for all files)
    // for normal commits, copy parent snapshot as base (minus staged deletions)
    let is_merge = second_parent.is_some();
    if !is_merge {
        if let Some(parent_hash) = &parent {
            let parent_snap = repo::commit_snapshot_root(parent_hash);
            if parent_snap.exists() {
                copy_dir_compressed(&parent_snap, &snap_root, &settings)?;
                for d in &deletions {
                    let plain = snap_root.join(d);
                    let gz = std::path::PathBuf::from(format!("{}.gz", plain.display()));
                    let _ = fs::remove_file(&plain);
                    let _ = fs::remove_file(&gz);
                }
            }
        }
    } else {
        for d in &deletions {
            let plain = snap_root.join(d);
            let gz = std::path::PathBuf::from(format!("{}.gz", plain.display()));
            let _ = fs::remove_file(&plain);
            let _ = fs::remove_file(&gz);
        }
    }

    for rel in &staged {
        let src = repo::stages_root().join(rel);
        let dst = snapshot_path_for(&snap_root, rel, &settings);
        if let Some(parent_dir) = dst.parent() { fs::create_dir_all(parent_dir).map_err(|e| format!("mkdir {parent_dir:?}: {e}"))?; }
        if super::compression::should_compress(&settings) {
            super::compression::compress_file(&src, &dst, super::compression::level(&settings))?;
        } else {
            fs::copy(&src, &dst).map_err(|e| format!("copy {rel:?}: {e}"))?;
        }
        if let Some(parent_hash) = &parent {
            let parent_file = repo::commit_snapshot_root(parent_hash).join(rel);
            let old_lines = read_lines_for_delta(&parent_file);
            let new_lines = read_lines_for_delta(&src);
            if old_lines != new_lines {
                let deltas = myers_diff(&old_lines, &new_lines);
                if !deltas.is_empty() {
                    let delta_path = delta_path_for(&deltas_root, rel, &settings);
                    if let Some(p) = delta_path.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir delta: {e}"))?; }
                    let mut out = String::new();
                    for d in deltas { match d { Delta::Insert{line,text} => out.push_str(&format!("insert::{}::{}\n", line, text)), Delta::Delete{line} => out.push_str(&format!("delete::{}\n", line)), } }
                    if super::compression::should_compress(&settings) {
                        let tmp = deltas_root.join(format!("tmp_{}.delta", rel.to_string_lossy().replace('/', "__")));
                        fs::write(&tmp, out).map_err(|e| format!("write tmp delta: {e}"))?;
                        super::compression::compress_file(&tmp, &delta_path, super::compression::level(&settings))?;
                        let _ = fs::remove_file(tmp);
                    } else {
                        fs::write(&delta_path, out).map_err(|e| format!("write delta {delta_path:?}: {e}"))?;
                    }
                }
            }
        }
    }

    let is_merge = second_parent.is_some();
    let meta = CommitMeta { hash: hash.clone(), parent: parent.clone(), second_parent, message: message.clone(), author, timestamp: ts, files: files.clone() };
    let meta_str = toml::to_string_pretty(&meta).map_err(|e| format!("serialize meta: {e}"))?;
    fs::write(repo::commit_meta_path(&hash), meta_str).map_err(|e| format!("write meta: {e}"))?;
    repo::write_head(&hash)?;
    if is_merge { let _ = repo::clear_merge_head(); }
    let current = repo::current_root();
    let _ = fs::remove_dir_all(&current);
    fs::create_dir_all(&current).map_err(|e| format!("create current: {e}"))?;
    for entry in walkdir::WalkDir::new(&snap_root).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            let rel = p.strip_prefix(&snap_root).unwrap();
            let mut rel_str = rel.to_string_lossy().to_string();
            let is_gz = rel_str.ends_with(".gz");
            if is_gz { rel_str.truncate(rel_str.len() - 3); }
            let dst = current.join(rel_str);
            if let Some(parent) = dst.parent() { fs::create_dir_all(parent).map_err(|e| format!("mkdir current {parent:?}: {e}"))?; }
            if is_gz { super::compression::decompress_file(p, &dst)?; } else { fs::copy(p, &dst).map_err(|e| format!("copy current: {e}"))?; }
        }
    }
    let _ = fs::remove_dir_all(repo::stages_root());
    fs::create_dir_all(repo::stages_root()).map_err(|e| format!("recreate stages: {e}"))?;
    // deletions file lives inside stages and is gone with it; be explicit for clarity
    let _ = fs::remove_file(repo::deletions_file());
    Ok(hash)
}

pub fn load_meta(hash: &str) -> Result<CommitMeta, String> {
    let p = repo::commit_meta_path(hash);
    let s = fs::read_to_string(&p).map_err(|e| format!("read meta {hash}: {e}"))?;
    toml::from_str(&s).map_err(|e| format!("parse meta {hash}: {e}"))
}

pub fn list_metas() -> Vec<CommitMeta> {
    let mut out = vec![];
    for h in repo::list_commits() { if let Ok(m) = load_meta(&h) { out.push(m); } }
    out.sort_by_key(|m| m.timestamp);
    out
}

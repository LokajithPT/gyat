use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::repo;

fn read_snapshot_lines(hash: &str, rel: &Path) -> Vec<String> {
    let snap = repo::commit_snapshot_root(hash);
    let plain = snap.join(rel);
    let gz = PathBuf::from(format!("{}.gz", plain.display()));
    let actual = if plain.exists() { plain } else if gz.exists() { gz } else { return vec![]; };
    if actual.extension().and_then(|s| s.to_str()) == Some("gz") {
        if let Ok(s) = super::compression::read_text_maybe_compressed(&actual) {
            return s.lines().map(|l| l.to_string()).collect();
        }
        vec![]
    } else {
        fs::read_to_string(&actual).map(|s| s.lines().map(|l| l.to_string()).collect()).unwrap_or_default()
    }
}

fn snapshot_has_file(hash: &str, rel: &Path) -> bool {
    let snap = repo::commit_snapshot_root(hash);
    let gz = PathBuf::from(format!("{}.gz", snap.join(rel).display()));
    snap.join(rel).exists() || gz.exists()
}

fn collect_files(hash: &str) -> BTreeSet<String> {
    let snap = repo::commit_snapshot_root(hash);
    if !snap.exists() { return BTreeSet::new(); }
    let mut out = BTreeSet::new();
    for entry in walkdir::WalkDir::new(&snap).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            if let Ok(rel) = p.strip_prefix(&snap) {
                let mut s = rel.to_string_lossy().to_string();
                if s.ends_with(".gz") { s.truncate(s.len() - 3); }
                // strip .delta suffix? snapshot shouldn't have delta, but keep
                out.insert(s);
            }
        }
    }
    out
}

fn ancestors(hash: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut stack = vec![hash.to_string()];
    while let Some(h) = stack.pop() {
        if !set.insert(h.clone()) { continue; }
        if let Ok(meta) = super::commit::load_meta(&h) {
            if let Some(p) = meta.parent.clone() { stack.push(p); }
            if let Some(p) = meta.second_parent.clone() { stack.push(p); }
        }
    }
    set
}

fn find_base(current: &str, target: &str) -> Option<String> {
    let cur_anc = ancestors(current);
    let mut stack = vec![target.to_string()];
    let mut visited = HashSet::new();
    while let Some(h) = stack.pop() {
        if !visited.insert(h.clone()) { continue; }
        if cur_anc.contains(&h) { return Some(h); }
        if let Ok(meta) = super::commit::load_meta(&h) {
            if let Some(p) = meta.parent.clone() { stack.push(p); }
            if let Some(p) = meta.second_parent.clone() { stack.push(p); }
        }
    }
    None
}

fn three_way_merge(base: &[String], cur: &[String], tgt: &[String], cur_branch: &str, tgt_branch: &str) -> (Vec<String>, bool) {
    if cur == tgt { return (cur.to_vec(), false); }
    if base == cur { return (tgt.to_vec(), false); }
    if base == tgt { return (cur.to_vec(), false); }

    // try Myers-based auto-merge: check if edits are non-overlapping
    // simplified: if cur and tgt both changed, we attempt line-level merge with markers
    // For top-notch we do proper conflict markers
    let mut merged = Vec::new();
    let conflict = true;
    // Very simplified: produce conflict markers
    // In future we could use diff ranges to auto-merge non-overlapping hunks
    // For now, always conflict when both changed differently
    merged.push(format!("<<<<<<< {cur_branch}"));
    merged.extend(cur.iter().cloned());
    merged.push("=======".to_string());
    merged.extend(tgt.iter().cloned());
    merged.push(format!(">>>>>>> {tgt_branch}"));
    (merged, conflict)
}

pub fn merge_branch(target_branch: &str, message: Option<String>) -> Result<(), String> {
    super::repo::ensure_repo()?;
    let cur_branch = super::repo::current_branch().ok_or("detached HEAD - checkout a branch first")?;
    if cur_branch == target_branch { return Err("cannot merge branch into itself".to_string()); }
    let cur_hash = super::repo::read_branch(&cur_branch).ok_or("current branch has no commits")?;
    let tgt_hash = super::repo::read_branch(target_branch).ok_or(format!("branch {target_branch} not found"))?;
    if cur_hash.is_empty() || tgt_hash.is_empty() { return Err("one of branches has no commits".to_string()); }
    if cur_hash == tgt_hash { println!("already up to date"); return Ok(()); }

    let base = find_base(&cur_hash, &tgt_hash);
    // fast-forward check: if base == cur, we can fast-forward current to target
    if base.as_deref() == Some(&cur_hash) {
        // fast-forward
        super::repo::write_branch(&cur_branch, &tgt_hash)?;
        // also update HEAD and restore snapshot
        super::travel::travel(target_branch)?;
        println!("fast-forward {cur_branch} -> {target_branch} ({tgt_hash})");
        return Ok(());
    }
    if base.as_deref() == Some(&tgt_hash) {
        println!("already up to date (target is ancestor)");
        return Ok(());
    }

    let base_hash = base.clone();
    let cur_files = collect_files(&cur_hash);
    let tgt_files = collect_files(&tgt_hash);
    let base_files = base.as_ref().map(|h| collect_files(h)).unwrap_or_default();
    let mut all: BTreeSet<String> = BTreeSet::new();
    for s in &cur_files { all.insert(s.clone()); }
    for s in &tgt_files { all.insert(s.clone()); }
    for s in &base_files { all.insert(s.clone()); }

    // prepare stages clean
    let stages = super::repo::stages_root();
    let _ = fs::remove_dir_all(&stages);
    fs::create_dir_all(&stages).map_err(|e| format!("mkdir stages: {e}"))?;

    let mut conflicts = vec![];
    for rel_str in &all {
        let rel = Path::new(rel_str);
        let base_lines = base_hash.as_ref().map(|h| read_snapshot_lines(h, rel)).unwrap_or_default();
        let cur_lines = read_snapshot_lines(&cur_hash, rel);
        let tgt_lines = read_snapshot_lines(&tgt_hash, rel);
        let cur_exists = snapshot_has_file(&cur_hash, rel);
        let tgt_exists = snapshot_has_file(&tgt_hash, rel);
        let base_exists = base_hash.as_ref().map(|h| snapshot_has_file(h, rel)).unwrap_or(false);

        if !base_exists && !cur_exists && tgt_exists {
            write_staged(rel, &tgt_lines)?;
            continue;
        }
        if !base_exists && cur_exists && !tgt_exists {
            write_staged(rel, &cur_lines)?;
            continue;
        }
        if !base_exists && cur_exists && tgt_exists {
            if cur_lines == tgt_lines {
                write_staged(rel, &cur_lines)?;
            } else {
                let (merged, _) = three_way_merge(&[], &cur_lines, &tgt_lines, &cur_branch, target_branch);
                write_staged(rel, &merged)?;
                conflicts.push(rel_str.clone());
            }
            continue;
        }
        if base_exists && !cur_exists && !tgt_exists {
            continue;
        }
        if base_exists && !cur_exists && tgt_exists {
            if tgt_lines == base_lines {
                continue;
            } else {
                let (merged, _) = three_way_merge(&base_lines, &[], &tgt_lines, &cur_branch, target_branch);
                write_staged(rel, &merged)?;
                conflicts.push(rel_str.clone());
            }
            continue;
        }
        if base_exists && cur_exists && !tgt_exists {
            if cur_lines == base_lines {
                continue;
            } else {
                let (merged, _) = three_way_merge(&base_lines, &cur_lines, &[], &cur_branch, target_branch);
                write_staged(rel, &merged)?;
                conflicts.push(rel_str.clone());
            }
            continue;
        }
        let (merged, is_conflict) = three_way_merge(&base_lines, &cur_lines, &tgt_lines, &cur_branch, target_branch);
        if is_conflict { conflicts.push(rel_str.clone()); }
        write_staged(rel, &merged)?;
    }
    // sync working tree to staged (for both conflict and auto-merge)
    for rel_str in &all {
        let rel = Path::new(rel_str);
        let staged = super::repo::stages_root().join(rel);
        let ws = Path::new(".").join(rel);
        if staged.exists() {
            if let Some(p) = ws.parent() { let _ = fs::create_dir_all(p); }
            let _ = fs::copy(&staged, &ws);
        } else {
            // deleted on both or correctly deleted -> remove from workspace if exists
            if ws.exists() {
                let _ = fs::remove_file(&ws);
                println!("deleted {} (merged deletion)", rel_str);
            }
        }
    }

    if !conflicts.is_empty() {
        // write MERGE_HEAD for next commit to be merge
        let _ = super::repo::write_merge_head(&tgt_hash);
        // also copy conflicted files to working tree for editing
        for f in &conflicts {
            let rel = Path::new(f);
            let staged = super::repo::stages_root().join(rel);
            let ws = Path::new(".").join(rel);
            if staged.exists() {
                if let Some(p) = ws.parent() { let _ = fs::create_dir_all(p); }
                let _ = fs::copy(&staged, &ws);
            }
        }
        println!("conflicts in {} file(s):", conflicts.len());
        for f in &conflicts { println!("  {f}"); }
        println!("fix conflicts in working tree then `gyat add <files>` and `gyat commit -m \"Merge {target_branch} into {cur_branch}\"`");
        println!("staged merged files with conflict markers in .gyt/stages + working tree");
        println!("MERGE_HEAD set - next commit will be merge commit");
        return Err(format!("merge conflicts - resolve and commit"));
    }

    // no conflicts: auto commit merge
    let msg = message.unwrap_or_else(|| format!("Merge branch '{target_branch}' into {cur_branch}"));
    // stages already contain merged files, create merge commit with second parent
    let hash = super::commit::create_commit_with_parents(msg.clone(), Some(tgt_hash.clone()))?;
    println!("merged {target_branch} -> {cur_branch} as {hash} \"{msg}\"");
    Ok(())
}

fn write_staged(rel: &Path, lines: &[String]) -> Result<(), String> {
    let stages = super::repo::stages_root();
    let dst = stages.join(rel);
    if let Some(p) = dst.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir {p:?}: {e}"))?; }
    let content = lines.join("\n");
    // add trailing newline if content non-empty
    let out = if content.is_empty() { String::new() } else { format!("{content}\n") };
    fs::write(dst, out).map_err(|e| format!("write staged {rel:?}: {e}"))?;
    Ok(())
}

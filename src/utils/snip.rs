use std::fs;
use super::repo;

/// Record each doomed commit's parent *before* the commit dirs are removed, so
/// HEAD can be relocated afterwards.
fn parent_map(doomed: &[String]) -> std::collections::HashMap<String, Option<String>> {
    let mut m = std::collections::HashMap::new();
    for h in doomed {
        m.insert(h.clone(), super::commit::load_meta(h).ok().and_then(|meta| meta.parent));
    }
    m
}

/// First ancestor of `from` that survived the snip, or None if the whole
/// chain was removed.
fn first_surviving(from: &str, parents: &std::collections::HashMap<String, Option<String>>) -> Option<String> {
    let mut cur = from.to_string();
    let mut guard = 0usize;
    while parents.contains_key(&cur) {
        guard += 1;
        if guard > 100_000 { return None; }
        match parents.get(&cur).cloned().flatten() {
            Some(next) => cur = next,
            None => return None,
        }
    }
    Some(cur)
}

/// Point the current branch (or a detached HEAD) at the first ancestor of
/// `from` that survived the snip, keeping the branch/detached distinction.
fn relocate_head(from: &str, parents: &std::collections::HashMap<String, Option<String>>) -> Result<(), String> {
    if !parents.contains_key(from) { return Ok(()); }
    match first_surviving(from, parents) {
        Some(cur) => {
            // write_head updates the branch ref when HEAD is `ref: refs/heads/x`
            // and rewrites the raw hash when detached, which is exactly the
            // distinction we want to preserve.
            repo::write_head(&cur)?;
            if repo::current_branch().is_none() {
                println!("HEAD now at {cur} (detached)");
            } else {
                println!("new HEAD {cur}");
            }
        }
        None => {
            fs::write(repo::head_path(), "").map_err(|e| format!("clear HEAD: {e}"))?;
            println!("no commits left");
        }
    }
    Ok(())
}

/// Repair every ref that pointed into the snipped range, not just HEAD.
///
/// Repairing only HEAD left other branches dangling at a deleted commit, so
/// `travel` on them failed with "snapshot missing" and their commits were
/// unreachable. Branches whose entire history was removed are deleted.
fn repair_refs(parents: &std::collections::HashMap<String, Option<String>>) -> Result<(), String> {
    let head_before = repo::read_head();
    for b in repo::list_branches() {
        let Some(h) = repo::read_branch(&b) else { continue; };
        if !parents.contains_key(&h) { continue; }
        match first_surviving(&h, parents) {
            Some(target) => {
                repo::write_branch(&b, &target)?;
                println!("branch {b} moved to {}", &target[..8.min(target.len())]);
            }
            None => {
                let _ = fs::remove_file(repo::branch_path(&b));
                println!("branch {b} deleted (all of its commits were snipped)");
            }
        }
    }
    if let Some(h) = head_before {
        relocate_head(&h, parents)?;
    }
    Ok(())
}

/// Refuse to rewrite history while a merge is unresolved. Snipping commits
/// leaves MERGE_HEAD pointing at a hash that may no longer exist, and the
/// next `commit` then writes a merge commit whose second parent is missing.
fn ensure_no_merge_in_progress() -> Result<(), String> {
    if let Some(h) = repo::read_merge_head() {
        return Err(format!(
            "cannot snip: a merge is in progress (MERGE_HEAD {})\n\
             hint: resolve the conflict markers and `gyat commit \"msg\"` to finish it first",
            &h[..8.min(h.len())]
        ));
    }
    Ok(())
}

pub fn snip_top() -> Result<(), String> {
    repo::ensure_repo()?;
    ensure_no_merge_in_progress()?;
    let head = repo::read_head().ok_or("no commits to snip")?;
    // Read the parent chain first: after the delete the metadata is gone.
    let parents = parent_map(std::slice::from_ref(&head));
    println!("snip top: removing HEAD {head}");
    let commit_dir = repo::commit_path(&head);
    if commit_dir.exists() {
        fs::remove_dir_all(&commit_dir).map_err(|e| format!("remove {head}: {e}"))?;
    }
    repair_refs(&parents)
}

pub fn snip_bottom() -> Result<(), String> {
    repo::ensure_repo()?;
    ensure_no_merge_in_progress()?;
    let mut metas = super::commit::list_metas();
    if metas.is_empty() { return Err("no commits to snip".to_string()); }
    metas.sort_by_key(|m| m.timestamp);
    let oldest = metas.first().unwrap().hash.clone();
    let parents = parent_map(std::slice::from_ref(&oldest));
    println!("snip bottom: removing oldest {oldest}");
    let dir = repo::commit_path(&oldest);
    fs::remove_dir_all(&dir).map_err(|e| format!("remove {oldest}: {e}"))?;
    // other branches may have pointed at this commit
    repair_refs(&parents)
}

pub fn snip_commit(start: &str, end: &str) -> Result<(), String> {
    repo::ensure_repo()?;
    ensure_no_merge_in_progress()?;
    // Range is inclusive, resolved by full/abbreviated hash, branch name, or
    // any revision expression (`HEAD~2`, `main^2`, `abc123~1`, ...).
    //
    // Order by (timestamp, parent depth) — the same order `log` shows — not by
    // hash, so that a range selects the commits a human would expect.
    let hashes: Vec<String> = super::commit::list_metas().into_iter().map(|m| m.hash).collect();
    let start_hash = repo::resolve_rev(start)?;
    let end_hash = repo::resolve_rev(end)?;
    let start_idx = hashes.iter().position(|h| *h == start_hash)
        .ok_or(format!("start {start} not found in history"))?;
    let end_idx = hashes.iter().position(|h| *h == end_hash)
        .ok_or(format!("end {end} not found in history"))?;
    let (s, e) = if start_idx <= end_idx { (start_idx, end_idx) } else { (end_idx, start_idx) };
    let doomed: Vec<String> = hashes[s..=e].to_vec();
    let parents = parent_map(&doomed);
    for h in &doomed {
        println!("snip commit {h}");
        let dir = repo::commit_path(h);
        let _ = fs::remove_dir_all(dir);
    }
    // If HEAD pointed into the snipped range, move it to the first ancestor
    // that survived rather than to some unrelated remaining commit.
    repair_refs(&parents)
}

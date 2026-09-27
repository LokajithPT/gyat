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

/// Point the current branch (or a detached HEAD) at the first ancestor of
/// `from` that survived the snip, keeping the branch/detached distinction.
///
/// Picking "the newest remaining commit" instead would move a branch onto an
/// unrelated commit whenever another branch had a newer tip.
fn relocate_head(from: &str, parents: &std::collections::HashMap<String, Option<String>>) -> Result<(), String> {
    if !parents.contains_key(from) { return Ok(()); }
    let mut cur = from.to_string();
    let mut guard = 0usize;
    while parents.contains_key(&cur) {
        guard += 1;
        if guard > 100_000 { break; }
        match parents.get(&cur).cloned().flatten() {
            Some(next) => cur = next,
            None => { cur.clear(); break; }
        }
    }
    if cur.is_empty() || parents.contains_key(&cur) {
        fs::write(repo::head_path(), "").map_err(|e| format!("clear HEAD: {e}"))?;
        println!("no commits left");
    } else {
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
    Ok(())
}

pub fn snip_top() -> Result<(), String> {
    repo::ensure_repo()?;
    let head = repo::read_head().ok_or("no commits to snip")?;
    // Read the parent chain first: after the delete the metadata is gone.
    let parents = parent_map(std::slice::from_ref(&head));
    println!("snip top: removing HEAD {head}");
    let commit_dir = repo::commit_path(&head);
    if commit_dir.exists() {
        fs::remove_dir_all(&commit_dir).map_err(|e| format!("remove {head}: {e}"))?;
    }
    relocate_head(&head, &parents)
}

pub fn snip_bottom() -> Result<(), String> {
    repo::ensure_repo()?;
    let mut metas = super::commit::list_metas();
    if metas.is_empty() { return Err("no commits to snip".to_string()); }
    metas.sort_by_key(|m| m.timestamp);
    let oldest = metas.first().unwrap().hash.clone();
    println!("snip bottom: removing oldest {oldest}");
    let dir = repo::commit_path(&oldest);
    fs::remove_dir_all(&dir).map_err(|e| format!("remove {oldest}: {e}"))?;
    Ok(())
}

pub fn snip_commit(start: &str, end: &str) -> Result<(), String> {
    repo::ensure_repo()?;
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
    if let Some(head) = repo::read_head() {
        relocate_head(&head, &parents)?;
    }
    Ok(())
}

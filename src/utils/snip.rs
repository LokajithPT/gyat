use std::fs;
use super::repo;

pub fn snip_top() -> Result<(), String> {
    repo::ensure_repo()?;
    let head = repo::read_head().ok_or("no commits to snip")?;
    println!("snip top: removing HEAD {head}");
    let commit_dir = repo::commit_path(&head);
    if commit_dir.exists() {
        fs::remove_dir_all(&commit_dir).map_err(|e| format!("remove {head}: {e}"))?;
    }
    // update HEAD to parent if exists, else clear
    // try load meta first (but we just deleted), so we need to have read parent before delete
    // Instead list remaining commits and pick last by timestamp
    let metas = super::commit::list_metas();
    if let Some(last) = metas.last() {
        repo::write_head(&last.hash)?;
        println!("new HEAD {}", last.hash);
    } else {
        fs::write(repo::head_path(), "").map_err(|e| format!("clear HEAD: {e}"))?;
        println!("no commits left");
    }
    Ok(())
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
    // range inclusive by hash prefix or full
    let all = repo::list_commits();
    let start_idx = all.iter().position(|h| h.starts_with(start)).ok_or(format!("start {start} not found"))?;
    let end_idx = all.iter().position(|h| h.starts_with(end)).ok_or(format!("end {end} not found"))?;
    let (s, e) = if start_idx <= end_idx { (start_idx, end_idx) } else { (end_idx, start_idx) };
    for h in all[s..=e].to_vec() {
        println!("snip commit {h}");
        let dir = repo::commit_path(&h);
        let _ = fs::remove_dir_all(dir);
    }
    // fix HEAD if it was deleted
    if let Some(head) = repo::read_head() {
        if !repo::commit_path(&head).exists() {
            let metas = super::commit::list_metas();
            if let Some(last) = metas.last() {
                repo::write_head(&last.hash)?;
                println!("HEAD moved to {}", last.hash);
            } else {
                fs::write(repo::head_path(), "").map_err(|e| format!("clear HEAD: {e}"))?;
            }
        }
    }
    Ok(())
}

use super::repo;
use std::fs;

pub fn list() -> Result<(), String> {
    super::repo::ensure_repo()?;
    let branches = repo::list_branches();
    let cur = repo::current_branch();
    if branches.is_empty() {
        println!("No branches yet");
        println!("hint: `gyat init` creates `main`");
        return Ok(());
    }
    for b in branches {
        let marker = if Some(&b) == cur.as_ref() { "*" } else { " " };
        let hash = repo::read_branch(&b).unwrap_or_default();
        let short = if hash.is_empty() { "(no commits yet)".to_string() } else { hash[..8.min(hash.len())].to_string() };
        let head_mark = if Some(&b) == cur.as_ref() { " (current)" } else { "" };
        println!("{marker} {b:<20} {short}{head_mark}");
    }
    if cur.is_none() {
        if let Some(h) = repo::read_head() {
            println!("Note: detached HEAD at {}", &h[..8.min(h.len())]);
            println!("hint: `gyat travel <branch>` to reattach");
        }
    }
    Ok(())
}

pub fn create(name: &str) -> Result<(), String> {
    super::repo::ensure_repo()?;
    if name.is_empty() { return Err("error: branch name required\nusage: `gyat branch <name>`".to_string()); }
    if name.contains('/') || name.contains("..") || name.contains(' ') {
        return Err(format!("error: invalid branch name '{name}'\nhint: avoid `/`, `..`, spaces"));
    }
    if repo::branch_exists(name) {
        return Err(format!("fatal: branch '{name}' already exists"));
    }
    let head = repo::read_head().ok_or("fatal: no commits yet\n(hint: `gyat commit \"init\"` first)".to_string())?;
    repo::write_branch(name, &head)?;
    println!("Branch '{name}' created at {} (from HEAD)", &head[..8.min(head.len())]);
    println!("hint: `gyat travel {name}` to switch");
    Ok(())
}

pub fn delete(name: &str) -> Result<(), String> {
    super::repo::ensure_repo()?;
    if !repo::branch_exists(name) {
        return Err(format!("error: branch '{name}' not found"));
    }
    let cur = repo::current_branch();
    if Some(name.to_string()) == cur {
        return Err(format!("error: cannot delete checked out branch '{name}'\nhint: `gyat travel <other>` first"));
    }
    let hash = repo::read_branch(name).unwrap_or_default();
    fs::remove_file(repo::branch_path(name)).map_err(|e| format!("error: delete branch {name}: {e}"))?;
    println!("Deleted branch {name} (was {}).", if hash.is_empty() { "-" } else { &hash[..8.min(hash.len())] });
    Ok(())
}

pub fn rename(old: &str, new: &str) -> Result<(), String> {
    super::repo::ensure_repo()?;
    if !repo::branch_exists(old) { return Err(format!("error: branch '{old}' not found")); }
    if repo::branch_exists(new) { return Err(format!("fatal: branch '{new}' already exists")); }
    if new.contains('/') || new.contains("..") || new.contains(' ') {
        return Err(format!("error: invalid branch name '{new}'"));
    }
    let hash = repo::read_branch(old).ok_or(format!("error: branch '{old}' has no commits"))?;
    repo::write_branch(new, &hash)?;
    fs::remove_file(repo::branch_path(old)).map_err(|e| format!("error: rename {old}: {e}"))?;
    if repo::current_branch().as_deref() == Some(old) {
        repo::set_head_branch(new)?;
    }
    println!("Renamed branch '{old}' -> '{new}'");
    Ok(())
}

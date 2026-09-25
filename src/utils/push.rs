pub fn push_remote() -> Result<(), String> {
    super::repo::ensure_repo()?;
    let head = super::repo::read_head();
    if head.is_none() {
        return Err("nothing to push: no commits. `gyat commit -m \"msg\"` first".to_string());
    }
    let cfg = super::config::load().map_err(|e| format!("load config: {e}"))?;
    println!("remote push (SSH planned)");
    println!("  repo: {}  server: {}  ssh: {}", cfg.repo.name, cfg.repo.server, cfg.ssh.as_ref().map(|s| s.key_path.as_str()).unwrap_or("~/.ssh/id_ed25519"));
    println!("  HEAD: {}", head.unwrap_or_default());
    println!("  would: tar commits + deltas and `ssh -i {} {} gyat-server receive`", cfg.ssh.as_ref().map(|s| s.key_path.as_str()).unwrap_or("~/.ssh/id_ed25519"), cfg.repo.server);
    println!("  local is source of truth - `gyat log` shows commits");
    Ok(())
}

// kept for backwards compat if called via old path
pub fn push(_local: bool, _message: Option<String>) -> Result<(), String> {
    Err("use `gyat commit -m \"msg\"` for local, `gyat push` for remote".to_string())
}

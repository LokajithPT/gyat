pub fn pull(commit: Option<String>) -> Result<(), String> {
    super::repo::ensure_repo()?;
    let cfg = super::config::load().map_err(|e| format!("load config: {e}"))?;
    match commit {
        Some(hash) => {
            println!("pull commit {hash} from {} (SSH stub)", cfg.repo.server);
            // TODO: fetch specific commit via SSH
            // For local MVP, just travel to that commit if it exists locally
            super::travel::travel(&hash)
        }
        None => {
            println!("pull latest from {} (SSH stub)", cfg.repo.server);
            println!("  repo: {}  server: {}  HEAD: {:?}", cfg.repo.name, cfg.repo.server, super::repo::read_head());
            println!("  would: `ssh user@server gyat-server list` + fetch missing commits");
            println!("  local MVP: no remote yet, `gyat log` shows local commits");
            Ok(())
        }
    }
}

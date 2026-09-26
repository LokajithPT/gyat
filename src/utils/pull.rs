use std::fs;
use std::path::{Path, PathBuf};

fn server_repo_path(cfg: &super::config::Config) -> Option<PathBuf> {
    let server = cfg.repo.server.trim();
    let is_path = server.contains('/') || server.starts_with('.') || server.starts_with('/') || Path::new(server).exists() || server == "gyat-server-data" || server == "./gyat-server-data";
    if !is_path { return None; }
    if server.contains(':') && !server.contains('/') { return None; }
    let base = Path::new(server);
    let repo = &cfg.repo.name;
    if base.ends_with(repo) { Some(base.to_path_buf()) } else { Some(base.join(repo)) }
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() { return Ok(()); }
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {dst:?}: {e}"))?;
    for entry in walkdir::WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            let rel = p.strip_prefix(src).unwrap();
            let target = dst.join(rel);
            if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?; }
            fs::copy(p, &target).map_err(|e| format!("copy {rel:?}: {e}"))?;
        }
    }
    Ok(())
}

/// Ingest fetched commits+refs from fetched tree dirs into the local repo.
/// Shared by path mode (server dirs) and ssh mode (unpacked bundle temp dir).
fn ingest(server_commits: &Path, server_refs: &Path, label: &str) -> Result<(), String> {
    if !server_commits.exists() && !server_refs.exists() {
        println!("no remote repo at {label} - nothing to pull, push first");
        return Ok(());
    }
    let mut fetched = 0;
    if server_commits.exists() {
        for entry in fs::read_dir(server_commits).map_err(|e| format!("read server commits: {e}"))? {
            let entry = entry.map_err(|e| format!("read entry: {e}"))?;
            let hash = entry.file_name().to_string_lossy().to_string();
            let local_commit = super::repo::commit_path(&hash);
            if !local_commit.exists() {
                copy_dir_all(&entry.path(), &local_commit)?;
                fetched += 1;
                println!("fetched commit {hash}");
            }
        }
    }
    let mut fetched_branches = 0;
    if server_refs.exists() {
        for entry in fs::read_dir(server_refs).map_err(|e| format!("read server refs: {e}"))? {
            let entry = entry.map_err(|e| format!("read ref: {e}"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            let server_branch = entry.path();
            let local_branch = super::repo::branch_path(&name);
            let server_hash = fs::read_to_string(&server_branch).unwrap_or_default().trim().to_string();
            let local_hash = fs::read_to_string(&local_branch).unwrap_or_default().trim().to_string();
            if server_hash != local_hash {
                if let Some(p) = local_branch.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir refs: {e}"))?; }
                fs::copy(&server_branch, &local_branch).map_err(|e| format!("fetch branch {name}: {e}"))?;
                fetched_branches += 1;
                println!("fetched branch {name} -> {server_hash}");
            }
        }
    }
    println!("pull from {label} done: {fetched} commits, {fetched_branches} branches");
    println!("hint: `gyat branch` to see, `gyat travel <branch>` to checkout");
    Ok(())
}

fn pull_ssh(cfg: &super::config::Config, commit: Option<String>) -> Result<(), String> {
    let (user, host, port, path) = match super::remote::parse_remote(&cfg.repo.server, &cfg.repo.name) {
        super::remote::Remote::Ssh { user, host, port, path } => (user, host, port, path),
        _ => return Err("internal: expected ssh remote".to_string()),
    };
    let key = cfg.ssh.as_ref().map(|s| {
        let k = &s.key_path;
        if let Some(rest) = k.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return format!("{home}/{rest}");
            }
        }
        k.clone()
    });
    let target = super::remote::SshTarget { user: user.clone(), host: host.clone(), port, key_path: key };
    let dest = match &user {
        Some(u) => format!("{u}@{host}"),
        None => host.clone(),
    };

    let remote_cmd = super::remote::remote_cmd(&super::remote::server_bin(), "fetch", &path, &[]);
    println!("fetching from {dest}:{path}");
    let bytes = target.run(&remote_cmd, None)?;
    // server prints progress to stderr; fetch bundle comes on stdout.
    // (over real ssh the streams stay separate; keep that contract here.)
    let tmp = super::remote::unpack_bundle_to_temp(&bytes)?;
    let r = ingest(&tmp.join("commits"), &tmp.join("refs/heads"), &format!("{dest}:{path}"));
    let _ = fs::remove_dir_all(&tmp);
    r?;
    if let Some(hash) = commit {
        super::travel::travel(&hash)?;
    }
    Ok(())
}

pub fn pull(commit: Option<String>) -> Result<(), String> {
    super::repo::ensure_repo()?;
    let cfg = super::config::load().map_err(|e| format!("load config: {e}"))?;

    // ssh destination?
    if matches!(
        super::remote::parse_remote(&cfg.repo.server, &cfg.repo.name),
        super::remote::Remote::Ssh { .. }
    ) {
        return pull_ssh(&cfg, commit);
    }

    if let Some(hash) = commit {
        println!("pull commit {hash} from {} (local path mode)", cfg.repo.server);
        if let Some(server_path) = server_repo_path(&cfg) {
            let server_commit = server_path.join("commits").join(&hash);
            let local_commit = super::repo::commit_path(&hash);
            if server_commit.exists() && !local_commit.exists() {
                copy_dir_all(&server_commit, &local_commit)?;
                println!("fetched commit {hash} from {}", server_path.display());
                super::travel::travel(&hash)?;
                return Ok(());
            } else if local_commit.exists() {
                super::travel::travel(&hash)?;
                return Ok(());
            } else {
                return Err(format!("commit {hash} not found on server {}", server_path.display()));
            }
        }
        return super::travel::travel(&hash);
    }

    // pull latest: fetch all missing commits and branches
    if let Some(server_path) = server_repo_path(&cfg) {
        return ingest(
            &server_path.join("commits"),
            &server_path.join("refs/heads"),
            &server_path.display().to_string(),
        );
    }

    Err(format!(
        "cannot pull: server `{}` is neither a path nor an ssh destination\n(hint: use a path like ./gyat-server-data, or user@host:/path, or ssh://host/path)",
        cfg.repo.server
    ))
}

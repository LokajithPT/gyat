use std::fs;
use std::path::{Path, PathBuf};

fn server_repo_path(cfg: &super::config::Config) -> Option<PathBuf> {
    let server = cfg.repo.server.trim();
    // treat as path if contains '/' or '.' or is existing dir, else network host
    let is_path = server.contains('/') || server.starts_with('.') || server.starts_with('/') || Path::new(server).exists() || server == "gyat-server-data" || server == "./gyat-server-data";
    if !is_path { return None; }
    // also handle host:port like 127.0.0.1:8081 -> not path
    if server.contains(':') && !server.contains('/') { return None; }
    let base = Path::new(server);
    let repo = &cfg.repo.name;
    // if server already ends with repo name, use as is, else join
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

pub fn push_remote(force: bool) -> Result<(), String> {
    super::repo::ensure_repo()?;
    let head = super::repo::read_head();
    if head.is_none() { return Err("nothing to push: no commits. `gyat commit -m \"msg\"` first".to_string()); }
    let cfg = super::config::load().map_err(|e| format!("load config: {e}"))?;
    let branch = super::repo::current_branch().unwrap_or_else(|| "(detached)".to_string());
    let head_hash = head.unwrap_or_default();

    // ssh destination?
    match super::remote::parse_remote(&cfg.repo.server, &cfg.repo.name) {
        super::remote::Remote::Ssh { user, host, port, path } => {
            return push_ssh(&cfg, &branch, &head_hash, user, host, port, path, force);
        }
        super::remote::Remote::Path(_) => {}
    }

    // local path mode
    if let Some(server_path) = server_repo_path(&cfg) {
        // ensure server repo structure
        let server_commits = server_path.join("commits");
        let server_refs = server_path.join("refs/heads");
        fs::create_dir_all(&server_commits).map_err(|e| format!("create server commits: {e}"))?;
        fs::create_dir_all(&server_refs).map_err(|e| format!("create server refs: {e}"))?;

        let local_commits = super::repo::commits_root();
        let local_refs = super::repo::refs_heads_root();

        let mut pushed_commits = 0;
        for entry in fs::read_dir(&local_commits).map_err(|e| format!("read local commits: {e}"))? {
            let entry = entry.map_err(|e| format!("read entry: {e}"))?;
            let hash = entry.file_name().to_string_lossy().to_string();
            let server_commit = server_commits.join(&hash);
            if !server_commit.exists() {
                copy_dir_all(&entry.path(), &server_commit)?;
                pushed_commits += 1;
                println!("pushed commit {hash}");
            }
        }
        // push branches
        let mut pushed_branches = 0;
        for entry in fs::read_dir(&local_refs).map_err(|e| format!("read refs: {e}"))? {
            let entry = entry.map_err(|e| format!("read ref: {e}"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            let server_branch = server_refs.join(&name);
            let local_hash = fs::read_to_string(entry.path()).unwrap_or_default().trim().to_string();
            let server_hash = fs::read_to_string(&server_branch).unwrap_or_default().trim().to_string();
            if local_hash != server_hash {
                fs::copy(entry.path(), &server_branch).map_err(|e| format!("push branch {name}: {e}"))?;
                pushed_branches += 1;
                println!("pushed branch {name} -> {local_hash}");
            }
        }
        // also handle detached HEAD? push current HEAD if detached
        if branch == "(detached)" {
            let server_head = server_path.join("HEAD");
            fs::write(server_head, &head_hash).map_err(|e| format!("write server HEAD: {e}"))?;
        }

        println!("push to {} done: {pushed_commits} commits, {pushed_branches} branches ({} -> {})", server_path.display(), branch, cfg.repo.server);
        // also update gyat.toml server host if needed
        return Ok(());
    }

    // path heuristics said "not a path" but remote parsing also did: unreachable
    // in practice, but keep a clear error instead of silent junk dirs.
    Err(format!(
        "cannot push: server `{}` is neither a path nor an ssh destination\n(hint: use a path like ./gyat-server-data, or user@host:/path, or ssh://host/path)",
        cfg.repo.server
    ))
}

fn push_ssh(
    cfg: &super::config::Config,
    branch: &str,
    head_hash: &str,
    user: Option<String>,
    host: String,
    port: Option<u16>,
    path: String,
    force: bool,
) -> Result<(), String> {
    let key = cfg.ssh.as_ref().map(|s| s.key_path.clone());
    // expand ~ in key path for ssh -i
    let key = key.map(|k| {
        if let Some(rest) = k.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return format!("{home}/{rest}");
            }
        }
        k
    });
    let target = super::remote::SshTarget { user: user.clone(), host: host.clone(), port, key_path: key };
    let dest = match &user {
        Some(u) => format!("{u}@{host}"),
        None => host.clone(),
    };
    println!("pushing {branch} ({}) to {dest}:{path}", &head_hash[..8.min(head_hash.len())]);

    let bundle = super::remote::pack_local_repo()?;
    let extra: &[&str] = if force { &["--force"] } else { &[] };
    let remote_cmd = super::remote::remote_cmd(&super::remote::server_bin(), "receive", &path, extra);
    let out = target.run(&remote_cmd, Some(&bundle))?;
    let text = String::from_utf8_lossy(&out);
    for line in text.lines() {
        println!("remote: {line}");
    }
    if text.lines().any(|l| l.starts_with("rejected")) {
        return Err("push rejected by server (non-fast-forward; retry with `gyat push --force`)".to_string());
    }
    println!("push to {dest}:{path} done ({branch} -> {})", &head_hash[..8.min(head_hash.len())]);
    Ok(())
}

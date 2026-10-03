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
    let branch = super::repo::current_branch();
    let head_hash = head.unwrap_or_default();

    // resolve destination: full remote as-is, empty/bare word via static config
    match super::host::resolve(&cfg.repo.server, &cfg.repo.name)? {
        super::remote::Remote::Ssh { user, host, port, path } => {
            let key = super::host::effective_key(cfg.ssh.as_ref().map(|s| s.key_path.as_str()));
            let key = super::remote::expand_tilde(&key);
            let target = super::remote::SshTarget { user, host, port, key_path: Some(key) };
            super::remote::progress("contacting server");
            let have = super::remote::server_commit_set(&target, &path);
            let local_all = super::repo::list_commits();
            let already = local_all.iter().filter(|h| have.contains(*h)).count();
            if already > 0 {
                super::remote::progress(&format!(
                    "server already has {already} of {} commit(s)",
                    local_all.len()
                ));
            }
            let wanted: std::collections::HashSet<String> =
                local_all.iter().filter(|h| !have.contains(*h)).cloned().collect();
            return match branch {
                Some(b) => push_ssh(&b, &head_hash, target, &path, force, Some(&wanted)),
                None => push_all_ssh(target, &path, force, Some(&wanted)),
            };
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
        // Detached HEAD has no ref of its own; the branches above are the refs
        // that exist, so they are what gets published.
        let label = branch.as_deref().unwrap_or("(detached: all branches)");

        println!("push to {} done: {pushed_commits} commits, {pushed_branches} branches ({label} -> {})", server_path.display(), &head_hash[..8.min(head_hash.len())]);
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

/// Detached HEAD has no branch of its own to name on the wire, so publish every
/// local branch instead. Sending a literal "(detached)" ref made the server
/// silently ignore the push while the client still reported success.
fn push_all_ssh(
    target: super::remote::SshTarget,
    path: &str,
    force: bool,
    wanted: Option<&std::collections::HashSet<String>>,
) -> Result<(), String> {
    let branches = super::repo::list_branches();
    if branches.is_empty() {
        return Err("HEAD is detached and there are no branches to push\n(hint: `gyat travel <branch>` to attach HEAD)".to_string());
    }
    println!("HEAD is detached - pushing all {} local branch(es)", branches.len());
    for b in branches {
        if let Some(hash) = super::repo::read_branch(&b) {
            push_ssh(&b, &hash, target.clone(), path, force, wanted)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_ssh(
    branch: &str,
    head_hash: &str,
    target: super::remote::SshTarget,
    path: &str,
    force: bool,
    wanted: Option<&std::collections::HashSet<String>>,
) -> Result<(), String> {
    let dest = target.label();
    println!(
        "pushing {branch} ({}) to {dest}:{path}",
        &head_hash[..8.min(head_hash.len())]
    );

    let t0 = std::time::Instant::now();
    let bundle = super::remote::pack_for_push(wanted)?;
    super::remote::progress(&format!(
        "bundle {} ready in {:.2}s",
        super::remote::human_bytes(bundle.len() as u64),
        t0.elapsed().as_secs_f64()
    ));
    let extra: &[&str] = if force { &["--force"] } else { &[] };
    super::remote::progress(&format!("uploading {} over ssh", super::remote::human_bytes(bundle.len() as u64)));
    let t1 = std::time::Instant::now();
    let out = target.run_server("receive", path, extra, Some(&bundle))?;
    super::remote::progress(&format!("server responded in {:.2}s", t1.elapsed().as_secs_f64()));
    let text = String::from_utf8_lossy(&out);
    for line in text.lines() {
        println!("remote: {line}");
    }
    if text.lines().any(|l| l.starts_with("rejected")) {
        return Err("push rejected by server (non-fast-forward; retry with `gyat push --force`)".to_string());
    }
    println!(
        "push to {dest}:{path} done ({branch} -> {})",
        &head_hash[..8.min(head_hash.len())]
    );
    Ok(())
}

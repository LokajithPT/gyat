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
    let old_head = super::repo::read_head();
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
    let refreshed = refresh_worktree(old_head.as_deref());
    println!("pull from {label} done: {fetched} commits, {fetched_branches} branches");
    if refreshed > 0 {
        println!("updated working tree: {refreshed} file(s) from remote");
    }
    println!("hint: `gyat branch` to see, `gyat travel <branch>` to checkout");
    Ok(())
}

/// Bring the working tree and `.gyt/current` in line with the current branch
/// after a pull moved its ref. Files the incoming commits changed are restored;
/// files that also carry uncommitted local edits are reported and left alone so
/// a pull never silently destroys work.
fn refresh_worktree(old_head: Option<&str>) -> usize {
    let Some(old) = old_head else { return 0 };
    if old.is_empty() || super::repo::is_detached() { return 0; }
    let Some(new_head) = super::repo::read_head() else { return 0; };
    if new_head == old { return 0; }
    let old_snap = super::repo::commit_snapshot_root(old);
    let new_snap = super::repo::commit_snapshot_root(&new_head);
    if !new_snap.exists() { return 0; }

    let old_files = snapshot_bytes(&old_snap);
    let new_files = snapshot_bytes(&new_snap);
    let mut changed = 0usize;

    for (rel, new_bytes) in &new_files {
        let ws = Path::new(".").join(rel);
        let cur = fs::read(&ws).ok();
        if cur.as_deref() == Some(new_bytes.as_slice()) { continue; } // already current
        if let Some(cur) = &cur {
            // Clean only if the worktree still matches the commit we came from.
            // Anything else is an uncommitted edit the pull must not destroy.
            let matches_old = old_files.get(rel).map(|o| *o == *cur).unwrap_or(false);
            if !matches_old {
                eprintln!(
                    "warning: {} has local changes, not overwritten (commit them, or `gyat travel <branch>` to reset)",
                    rel.display()
                );
                continue;
            }
        }
        if let Some(p) = ws.parent() { let _ = fs::create_dir_all(p); }
        if fs::write(&ws, new_bytes).is_ok() {
            changed += 1;
        } else {
            eprintln!("warning: could not update {}", rel.display());
        }
    }

    for (rel, old_bytes) in &old_files {
        if new_files.contains_key(rel) { continue; }
        let ws = Path::new(".").join(rel);
        match fs::read(&ws) {
            Ok(cur) if cur == *old_bytes => {
                if fs::remove_file(&ws).is_ok() { changed += 1; }
            }
            Ok(_) => eprintln!("warning: {} deleted upstream but locally modified, kept", rel.display()),
            Err(_) => {}
        }
    }

    rebuild_current(&new_snap);
    changed
}

/// Every file in a commit snapshot, keyed by its uncompressed relative path,
/// with the stored bytes (transparently gunzipping `.gz` entries).
fn snapshot_bytes(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    if !root.exists() { return out; }
    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() { continue; }
        let Ok(rel) = entry.path().strip_prefix(root) else { continue };
        let stored = entry.path().to_path_buf();
        let key = match rel.to_str() {
            Some(s) if s.ends_with(".gz") => PathBuf::from(s.trim_end_matches(".gz")),
            Some(s) => PathBuf::from(s),
            None => continue,
        };
        if let Ok(bytes) = super::compression::read_bytes_maybe_compressed(&stored) {
            out.insert(key, bytes);
        }
    }
    out
}

/// Re-mirror `.gyt/current` from a commit snapshot.
fn rebuild_current(snap: &Path) {
    let current = super::repo::current_root();
    let _ = fs::remove_dir_all(&current);
    if fs::create_dir_all(&current).is_err() { return; }
    for (rel, bytes) in snapshot_bytes(snap) {
        let dst = current.join(rel);
        if let Some(p) = dst.parent() { let _ = fs::create_dir_all(p); }
        let _ = fs::write(dst, bytes);
    }
}

fn pull_ssh(cfg: &super::config::Config, commit: Option<String>) -> Result<(), String> {
    let (user, host, port, path) = match super::host::resolve(&cfg.repo.server, &cfg.repo.name)? {
        super::remote::Remote::Ssh { user, host, port, path } => (user, host, port, path),
        _ => return Err("internal: expected ssh remote".to_string()),
    };
    let key = super::host::effective_key(cfg.ssh.as_ref().map(|s| s.key_path.as_str()));
    let key = super::remote::expand_tilde(&key);
    let target = super::remote::SshTarget { user: user.clone(), host: host.clone(), port, key_path: Some(key) };
    let dest = match &user {
        Some(u) => format!("{u}@{host}"),
        None => host.clone(),
    };

    let bytes = target.run_server("fetch", &path, &[], None)?;
    // server prints progress to stderr; fetch bundle comes on stdout.
    // (over real ssh the streams stay separate; keep that contract here.)
    let tmp = super::remote::unpack_bundle_to_temp(&bytes)?;    let r = ingest(&tmp.join("commits"), &tmp.join("refs/heads"), &format!("{dest}:{path}"));
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

    // ssh destination (full remote or static-resolved)?
    if matches!(
        super::host::resolve(&cfg.repo.server, &cfg.repo.name)?,
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

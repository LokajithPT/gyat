//! `gyat list`: see all the shit that is in there (on your server).
//! ```text
//! gyat list           -> repos on the static server (or sibling repos for path mode)
//! gyat list <repo>    -> branches of that repo (bare name resolves via static)
//! ```
//! Transport mirrors push/pull: local paths read directly, everything else
//! goes through `gyat-server list` over ssh. No URLs needed with static config.

use std::fs;
use std::path::{Path, PathBuf};

use super::remote::Remote;

fn is_repo_dir(p: &Path) -> bool {
    p.join("commits").exists()
}

fn list_repos_local(base: &Path) -> Result<Vec<String>, String> {
    if !base.exists() {
        return Err(format!("path {} not found", base.display()));
    }
    let mut repos = vec![];
    for entry in fs::read_dir(base).map_err(|e| format!("read {}: {e}", base.display()))? {
        let entry = entry.map_err(|e| format!("read entry: {e}"))?;
        if entry.path().is_dir() && is_repo_dir(&entry.path()) {
            repos.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    repos.sort();
    Ok(repos)
}

fn list_branches_local(repo: &Path) -> Result<Vec<(String, String)>, String> {
    let refs = repo.join("refs/heads");
    if !refs.exists() {
        return Err(format!("{} has no branches (push first?)", repo.display()));
    }
    let mut out = vec![];
    for entry in fs::read_dir(&refs).map_err(|e| format!("read refs: {e}"))? {
        let entry = entry.map_err(|e| format!("read ref: {e}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let hash = fs::read_to_string(entry.path()).unwrap_or_default().trim().to_string();
        let short = if hash.len() >= 8 { hash[..8].to_string() } else { hash };
        out.push((name, short));
    }
    out.sort();
    Ok(out)
}

/// Run `gyat-server list <path>` over ssh and return its stdout.
fn list_remote(
    user: Option<String>,
    host: String,
    port: Option<u16>,
    path: String,
    key: Option<String>,
) -> Result<String, String> {
    let target = super::remote::SshTarget { user, host, port, key_path: key };
    let bytes = target.run_server("list", &path, &[], None)?;
    String::from_utf8(bytes).map_err(|e| format!("server list output not utf-8: {e}"))
}

fn repo_key() -> Option<String> {
    let key = super::host::effective_key(
        super::config::load()
            .ok()
            .as_ref()
            .and_then(|c| c.ssh.as_ref())
            .map(|s| s.key_path.as_str()),
    );
    Some(super::remote::expand_tilde(&key))
}

fn display_remote(user: &Option<String>, host: &str, path: &str) -> String {
    match user {
        Some(u) => format!("{u}@{host}:{path}"),
        None => format!("{host}:{path}"),
    }
}

fn print_repos(label: &str, repos: &[String]) {
    if repos.is_empty() {
        println!("no repos on {label} yet (hint: `gyat push` to publish one)");
        return;
    }
    println!("repos on {label}:");
    for r in repos {
        println!("  {r}");
    }
}

fn print_branches(label: &str, branches: &[(String, String)]) {
    if branches.is_empty() {
        println!("no branches in {label} yet");
        return;
    }
    println!("branches of {label}:");
    for (name, hash) in branches {
        println!("  {name:<20} {hash}");
    }
}

/// Decide header based on server output shape (branches print `name hash`).
fn render_remote(label: &str, out: &str) {
    let lines: Vec<&str> = out.lines().collect();
    let looks_like_branches = !lines.is_empty()
        && lines.iter().all(|l| l.split_whitespace().count() == 2);
    if looks_like_branches {
        let mut branches = vec![];
        for l in lines {
            let mut it = l.split_whitespace();
            let name = it.next().unwrap_or("").to_string();
            let hash = it.next().unwrap_or("").to_string();
            let short = if hash.len() >= 8 { hash[..8].to_string() } else { hash };
            branches.push((name, short));
        }
        print_branches(label, &branches);
    } else if lines.is_empty() {
        println!("nothing on {label} yet (hint: `gyat push` to publish)");
    } else {
        print_repos(label, &lines.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }
}

fn parent_remote_path(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    let parent = Path::new(trimmed).parent()?.to_string_lossy().to_string();
    if parent.is_empty() {
        None
    } else {
        Some(parent)
    }
}

pub fn list(repo: Option<String>) -> Result<(), String> {
    match repo {
        Some(name) => list_target(&name),
        None => list_default(),
    }
}

/// `gyat list <target>`: bare name via static, full remote/path as-is.
/// Shows branches if it's a repo, repos if it's a base dir.
fn list_target(name: &str) -> Result<(), String> {
    let repo_name = if name.contains('/') || name.contains(':') {
        name.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(name)
            .to_string()
    } else {
        name.to_string()
    };
    let remote = super::host::resolve(name, &repo_name)?;
    match remote {
        Remote::Path(p) => show_path(&p),
        Remote::Ssh { user, host, port, path } => {
            let label = display_remote(&user, &host, &path);
            let out = list_remote(user, host, port, path, repo_key())?;
            render_remote(&label, &out);
            Ok(())
        }
    }
}

/// `gyat list`: repos on the static base; sibling repos for path-mode repos.
fn list_default() -> Result<(), String> {
    if let Some(st) = super::host::load() {
        let label = st.describe();
        let base = st.server.base.clone();
        let (user, host, port) = (
            st.server.user.clone().filter(|u| !u.is_empty()),
            st.server.host.clone(),
            st.server.port,
        );
        if host.trim().is_empty() {
            return Err("no static server configured\n(hint: `gyat setup` once, then `gyat list`)".to_string());
        }
        let out = list_remote(user.clone(), host.clone(), port, base.clone(), repo_key())?;
        // static base is always a base dir, but render handles both shapes
        let full_label = match &user {
            Some(u) => format!("{u}@{host}:{base}"),
            None => format!("{host}:{base}"),
        };
        let _ = label;
        render_remote(&full_label, &out);
        return Ok(());
    }
    if super::repo::gyt_root().exists() {
        let cfg = super::config::load().map_err(|e| format!("load config: {e}"))?;
        match super::host::resolve(&cfg.repo.server, &cfg.repo.name)? {
            Remote::Path(p) => {
                let base = if p.ends_with(&cfg.repo.name) {
                    p.parent().map(PathBuf::from).unwrap_or(p.clone())
                } else {
                    p.clone()
                };
                show_path(&base)
            }
            Remote::Ssh { user, host, port, path } => {
                // list the parent dir (repo base) on the server
                let base = parent_remote_path(&path).unwrap_or(path.clone());
                let label = display_remote(&user, &host, &base);
                let out = list_remote(user, host, port, base, repo_key())?;
                render_remote(&label, &out);
                Ok(())
            }
        }
    } else {
        Err("no static server configured\n(hint: `gyat setup` once, then `gyat list`)".to_string())
    }
}

/// Show a local path: branches if it's a repo, repos if it's a base dir.
fn show_path(p: &Path) -> Result<(), String> {
    if is_repo_dir(p) || p.join("refs/heads").exists() {
        let label = p.display().to_string();
        print_branches(&label, &list_branches_local(p)?);
    } else {
        let label = p.display().to_string();
        print_repos(&label, &list_repos_local(p)?);
    }
    Ok(())
}

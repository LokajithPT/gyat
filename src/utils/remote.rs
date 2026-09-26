//! Remote endpoints: local paths (T420 file mode) or ssh destinations.
//!
//! SSH transport shells out to the system `ssh` binary exactly like git
//! does, so existing keys/agents keep working and no SSH crate is needed:
//! ```text
//! gyat push  ->  ssh user@host gyat-server receive /srv/gyat/repo < bundle
//! gyat pull  ->  ssh user@host gyat-server fetch /srv/gyat/repo > bundle
//! ```
//! Accepted server forms:
//! - `/abs/path`, `./rel/path`, `~/path` (also bare `gyat-server-data`): path mode
//! - `[user@]host:/abs/or/rel/path`: scp-like ssh
//! - `ssh://[user@]host[:port]/path`: ssh with optional port
//! Anything else (e.g. `raspi.local`, `127.0.0.1:8081`) is treated as a
//! bare host with the repo name as remote path — configure the full form
//! for real pushes.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remote {
    Path(PathBuf),
    Ssh {
        user: Option<String>,
        host: String,
        port: Option<u16>,
        path: String,
    },
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    s.to_string()
}

/// Split `[user@]host` into (user, host). Returns None if empty.
fn split_user_host(s: &str) -> Option<(Option<String>, String)> {
    if s.is_empty() {
        return None;
    }
    match s.split_once('@') {
        Some((u, h)) if !u.is_empty() && !h.is_empty() => Some((Some(u.to_string()), h.to_string())),
        Some(_) => None,
        None => Some((None, s.to_string())),
    }
}

pub fn parse_remote(server: &str, repo_name: &str) -> Remote {
    let server = server.trim();
    // ssh://[user@]host[:port]/path
    if let Some(rest) = server.strip_prefix("ssh://") {
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], rest[i..].to_string()),
            None => (rest, format!("/{repo_name}")),
        };
        let (user_host, port) = match authority.rfind(':') {
            Some(i) if !authority[i + 1..].contains(']') => {
                let p = authority[i + 1..].parse::<u16>().ok();
                (&authority[..i], p)
            }
            _ => (authority, None),
        };
        if let Some((user, host)) = split_user_host(user_host) {
            let path = if path == "/" || path.is_empty() {
                format!("/{repo_name}")
            } else {
                path
            };
            return Remote::Ssh { user, host, port, path };
        }
        // fall through to path on malformed ssh://
    } else if let Some(colon) = server.find(':') {
        // scp-like [user@]host:path — but not /abs/path, ./rel, ~/x
        let before = &server[..colon];
        let after = &server[colon + 1..];
        let looks_like_host = !before.contains('/')
            && !before.is_empty()
            && !after.is_empty()
            && !after.starts_with(':');
        if looks_like_host {
            // bare host:port (digits only after colon, no slash) -> default port, repo as path
            if !after.contains('/') {
                if let Ok(port) = after.parse::<u16>() {
                    if let Some((user, host)) = split_user_host(before) {
                        return Remote::Ssh {
                            user,
                            host,
                            port: Some(port),
                            path: repo_name.to_string(),
                        };
                    }
                }
            }
            if let Some((user, host)) = split_user_host(before) {
                return Remote::Ssh {
                    user,
                    host,
                    port: None,
                    path: after.to_string(),
                };
            }
        }
    } else if !server.is_empty() && !server.contains('/') && !server.starts_with('.') {
        // bare single word with no dots (e.g. `gyat-server-data`): local dirname
        if !server.contains('.') && !server.contains('@') {
            // fall through to path mode below
        } else if let Some((user, host)) = split_user_host(server) {
            // bare hostname (e.g. `raspi.local` from old inits): ssh, repo as remote path
            return Remote::Ssh {
                user,
                host,
                port: None,
                path: repo_name.to_string(),
            };
        }
    }
    // path mode (existing behavior): join repo name unless already there
    let expanded = expand_tilde(server);
    let base = PathBuf::from(&expanded);
    let path = if base.ends_with(repo_name) {
        base
    } else {
        base.join(repo_name)
    };
    Remote::Path(path)
}

/// Server-side binary name; override with GYAT_SERVER_BIN (useful for tests).
pub fn server_bin() -> String {
    std::env::var("GYAT_SERVER_BIN").unwrap_or_else(|_| "gyat-server".to_string())
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub struct SshTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub key_path: Option<String>,
}

impl SshTarget {
    fn dest(&self) -> String {
        match &self.user {
            Some(u) => format!("{u}@{}", self.host),
            None => self.host.clone(),
        }
    }

    fn base_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            "-o".to_string(),
            "ConnectTimeout=15".to_string(),
        ];
        if let Some(key) = &self.key_path {
            if !key.is_empty() && key != "~/.ssh/id_ed25519" {
                // default key needs no flag; custom keys do
                args.push("-i".to_string());
                args.push(key.clone());
            } else if !key.is_empty() {
                args.push("-i".to_string());
                args.push(key.clone());
            }
        }
        if let Some(port) = self.port {
            args.push("-p".to_string());
            args.push(port.to_string());
        }
        args
    }

    /// Run a remote command, piping `stdin_bytes` to it. Returns stdout.
    pub fn run(&self, remote_cmd: &str, stdin_bytes: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let ssh = std::env::var("GYAT_SSH_BIN").unwrap_or_else(|_| "ssh".to_string());
        let mut cmd = Command::new(&ssh);
        cmd.args(self.base_args());
        cmd.arg(self.dest());
        cmd.arg(remote_cmd);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        if stdin_bytes.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().map_err(|e| format!("spawn {ssh}: {e}"))?;
        if let Some(data) = stdin_bytes {
            child
                .stdin
                .as_mut()
                .ok_or("ssh stdin unavailable".to_string())?
                .write_all(data)
                .map_err(|e| format!("ssh stdin: {e}"))?;
        }
        let out = child.wait_with_output().map_err(|e| format!("ssh wait: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let detail = if !err.is_empty() { err } else { stdout };
            return Err(format!("ssh {} failed: {}", self.dest(), detail));
        }
        Ok(out.stdout)
    }
}

/// Pack local `.gyt/commits` + `.gyt/refs` into a push bundle (in memory).
pub fn pack_local_repo() -> Result<Vec<u8>, String> {
    super::repo::ensure_repo()?;
    let stage = super::repo::gyt_root().join(".bundle-stage");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| format!("bundle stage: {e}"))?;
    let r = (|| -> Result<Vec<u8>, String> {
        copy_newer_tree(&super::repo::commits_root(), &stage.join("commits"))?;
        copy_newer_tree(&super::repo::gyt_root().join("refs"), &stage.join("refs"))?;
        let mut buf = Vec::new();
        gyat_bundle::pack_dir(&stage, &mut buf)?;
        Ok(buf)
    })();
    let _ = std::fs::remove_dir_all(&stage);
    r
}

fn copy_newer_tree(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst).map_err(|e| format!("mkdir: {e}"))?;
    for entry in walkdir::WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            let rel = p.strip_prefix(src).unwrap();
            let target = dst.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
            }
            std::fs::copy(p, &target).map_err(|e| format!("copy: {e}"))?;
        }
    }
    Ok(())
}

/// Unpack a fetch bundle into a temp dir; returns the dir path.
pub fn unpack_bundle_to_temp(bytes: &[u8]) -> Result<PathBuf, String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("gyat-fetch-{}-{id}", std::process::id()));
    let mut cur: &[u8] = bytes;
    gyat_bundle::unpack_to(&mut cur, &dir)?;
    Ok(dir)
}

pub fn remote_cmd(bin: &str, sub: &str, repo_path: &str, extra: &[&str]) -> String {
    let mut parts = vec![bin.to_string(), sub.to_string(), shell_quote(repo_path)];
    parts.extend(extra.iter().map(|s| shell_quote(s)));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paths() {
        assert!(matches!(
            parse_remote("/srv/gyat/myrepo", "myrepo"),
            Remote::Path(_)
        ));
        if let Remote::Path(p) = parse_remote("/srv/gyat", "myrepo") {
            assert_eq!(p, PathBuf::from("/srv/gyat/myrepo"));
        } else {
            panic!("expected path");
        }
        assert!(matches!(parse_remote("./data", "r"), Remote::Path(_)));
        assert!(matches!(parse_remote("~/repos", "r"), Remote::Path(_)));
        assert!(matches!(parse_remote("gyat-server-data", "r"), Remote::Path(_)));
    }

    #[test]
    fn parses_scp_like() {
        match parse_remote("loki@pi:/srv/gyat/myrepo", "myrepo") {
            Remote::Ssh { user, host, port, path } => {
                assert_eq!(user.as_deref(), Some("loki"));
                assert_eq!(host, "pi");
                assert_eq!(port, None);
                assert_eq!(path, "/srv/gyat/myrepo");
            }
            _ => panic!("expected ssh"),
        }
        match parse_remote("pi:repos/myrepo", "myrepo") {
            Remote::Ssh { user, host, port, path } => {
                assert_eq!(user, None);
                assert_eq!(host, "pi");
                assert_eq!(port, None);
                assert_eq!(path, "repos/myrepo");
            }
            _ => panic!("expected ssh"),
        }
    }

    #[test]
    fn parses_ssh_urls() {
        match parse_remote("ssh://loki@pi:2222/srv/gyat/myrepo", "myrepo") {
            Remote::Ssh { user, host, port, path } => {
                assert_eq!(user.as_deref(), Some("loki"));
                assert_eq!(host, "pi");
                assert_eq!(port, Some(2222));
                assert_eq!(path, "/srv/gyat/myrepo");
            }
            _ => panic!("expected ssh"),
        }
        match parse_remote("ssh://pi/srv/gyat", "myrepo") {
            Remote::Ssh { path, port, .. } => {
                assert_eq!(port, None);
                assert_eq!(path, "/srv/gyat");
            }
            _ => panic!("expected ssh"),
        }
    }

    #[test]
    fn bare_host_and_host_port_become_ssh() {
        match parse_remote("raspi.local", "myrepo") {
            Remote::Ssh { user, host, port, path } => {
                assert_eq!(user, None);
                assert_eq!(host, "raspi.local");
                assert_eq!(port, None);
                assert_eq!(path, "myrepo");
            }
            _ => panic!("expected ssh"),
        }
        match parse_remote("127.0.0.1:8081", "myrepo") {
            Remote::Ssh { host, port, path, .. } => {
                assert_eq!(host, "127.0.0.1");
                assert_eq!(port, Some(8081));
                assert_eq!(path, "myrepo");
            }
            _ => panic!("expected ssh"),
        }
        match parse_remote("loki@pi", "myrepo") {
            Remote::Ssh { user, host, path, .. } => {
                assert_eq!(user.as_deref(), Some("loki"));
                assert_eq!(host, "pi");
                assert_eq!(path, "myrepo");
            }
            _ => panic!("expected ssh"),
        }
    }
}

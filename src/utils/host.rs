//! Static server config: your box never changes, so configure it once and
//! never type a URL again.
//!
//! File: `~/.gyatconfig.toml` (override with `GYAT_CONFIG` env, used by tests).
//! ```toml
//! [server]
//! host = "100.81.91.113"
//! user = "water"
//! port = 22
//! base = "gyat-server-data"
//! key = "~/.ssh/id_ed25519"
//! bin = "gyat-server"
//! ```
//! Resolution (`resolve`):
//! - empty repo `server` field -> static `base/<repo>`
//! - full remote (`user@host:path`, `ssh://..`, real path) -> as-is
//! - bare word (`myrepo`) -> static `base/<word>`, else a clear error
//!   (never silent junk dirs).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::remote::Remote;

fn default_base() -> String {
    "gyat-server-data".to_string()
}
fn default_key() -> String {
    "~/.ssh/id_ed25519".to_string()
}
fn default_bin() -> String {
    "gyat-server".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticServer {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default = "default_base")]
    pub base: String,
    #[serde(default = "default_key")]
    pub key: String,
    #[serde(default = "default_bin")]
    pub bin: String,
}

impl Default for StaticServer {
    fn default() -> Self {
        Self {
            host: String::new(),
            user: None,
            port: None,
            base: default_base(),
            key: default_key(),
            bin: default_bin(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StaticConfig {
    #[serde(default)]
    pub server: StaticServer,
}

pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("GYAT_CONFIG") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".gyatconfig.toml")
}

pub fn load() -> Option<StaticConfig> {
    let p = config_path();
    let s = std::fs::read_to_string(p).ok()?;
    toml::from_str(&s).ok()
}

pub fn save(cfg: &StaticConfig) -> Result<(), String> {
    let p = config_path();
    let s = toml::to_string_pretty(cfg).map_err(|e| format!("serialize static config: {e}"))?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    std::fs::write(&p, s).map_err(|e| format!("write {}: {e}", p.display()))?;
    Ok(())
}

fn join_base(base: &str, name: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

impl StaticConfig {
    fn require_host(&self) -> Result<(&str, Option<&str>, Option<u16>), String> {
        if self.server.host.trim().is_empty() {
            return Err(
                "no server configured yet\n(hint: run `gyat setup` once — host, user, base)".to_string(),
            );
        }
        Ok((
            self.server.host.trim(),
            self.server.user.as_deref().filter(|u| !u.is_empty()),
            self.server.port,
        ))
    }

    /// Remote for `<base>/<name>` on the static box.
    pub fn remote_for(&self, name: &str) -> Result<Remote, String> {
        let (host, user, port) = self.require_host()?;
        Ok(Remote::Ssh {
            user: user.map(|s| s.to_string()),
            host: host.to_string(),
            port,
            path: join_base(&self.server.base, name),
        })
    }

    pub fn describe(&self) -> String {
        match (&self.server.user, self.server.port) {
            (Some(u), Some(p)) if !u.is_empty() => {
                format!("{u}@{}:{p}:{}", self.server.host, self.server.base)
            }
            (Some(u), _) if !u.is_empty() => {
                format!("{u}@{}:{}", self.server.host, self.server.base)
            }
            _ => format!("{}:{}", self.server.host, self.server.base),
        }
    }
}

/// Resolve a repo's `server` field (or a clone source) to a Remote.
/// Never invents local junk dirs: bare words require static config.
pub fn resolve(server_field: &str, repo_name: &str) -> Result<Remote, String> {
    let f = server_field.trim();
    if f.is_empty() {
        let st = load().ok_or(
            "no server set for this repo and no static server configured\n(hint: `gyat setup` once, then just `gyat push`)".to_string(),
        )?;
        return st.remote_for(repo_name);
    }
    match super::remote::parse_remote(f, repo_name) {
        Remote::Ssh { .. } => Ok(super::remote::parse_remote(f, repo_name)),
        Remote::Path(p) => {
            // Explicit paths (separators, ./, ~/) always stay local.
            // Bare words ALWAYS mean static: a local dir that happens to share
            // the name must never shadow the server (use ./name for local).
            let explicit_path =
                f.contains('/') || f.starts_with('.') || f.starts_with('~') || f.starts_with('/');
            if explicit_path {
                Ok(Remote::Path(p))
            } else if p.exists() && load().is_none() {
                // legacy: no static configured, keep local-dir behavior
                Ok(Remote::Path(p))
            } else {
                match load() {
                    Some(st) => st.remote_for(f),
                    None => Err(format!(
                        "cannot resolve server `{f}`: not a path, not a URL, and no static server\n(hint: `gyat setup` once, then just `gyat push`)"
                    )),
                }
            }
        }
    }
}

/// Key precedence: repo config -> static config -> default.
pub fn effective_key(repo_key: Option<&str>) -> String {
    if let Some(k) = repo_key {
        if !k.trim().is_empty() {
            return k.to_string();
        }
    }
    if let Some(st) = load() {
        if !st.server.key.trim().is_empty() {
            return st.server.key.clone();
        }
    }
    default_key()
}

/// Server binary precedence: GYAT_SERVER_BIN env -> static -> default.
pub fn server_bin() -> String {
    if let Ok(b) = std::env::var("GYAT_SERVER_BIN") {
        if !b.is_empty() {
            return b;
        }
    }
    if let Some(st) = load() {
        if !st.server.bin.trim().is_empty() {
            return st.server.bin.clone();
        }
    }
    default_bin()
}

#[cfg(test)]
mod tests {
    use super::*;

    // GYAT_CONFIG is process-global: serialize all env-mutating tests.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_config(content: &str, f: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "gyat-host-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("gyatconfig.toml");
        std::fs::write(&cfg, content).unwrap();
        let old = std::env::var("GYAT_CONFIG").ok();
        unsafe { std::env::set_var("GYAT_CONFIG", &cfg) };
        f();
        match old {
            Some(v) => unsafe { std::env::set_var("GYAT_CONFIG", v) },
            None => unsafe { std::env::remove_var("GYAT_CONFIG") },
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    const CFG: &str = r#"
[server]
host = "100.81.91.113"
user = "water"
base = "gyat-server-data"
"#;

    #[test]
    fn empty_field_uses_static() {
        with_config(CFG, || {
            match resolve("", "myrepo").unwrap() {
                Remote::Ssh { user, host, path, .. } => {
                    assert_eq!(user.as_deref(), Some("water"));
                    assert_eq!(host, "100.81.91.113");
                    assert_eq!(path, "gyat-server-data/myrepo");
                }
                _ => panic!("expected ssh"),
            }
        });
    }

    #[test]
    fn bare_word_uses_static_base() {
        with_config(CFG, || {
            match resolve("other", "myrepo").unwrap() {
                Remote::Ssh { path, .. } => assert_eq!(path, "gyat-server-data/other"),
                _ => panic!("expected ssh"),
            }
        });
    }

    #[test]
    fn full_forms_pass_through() {
        with_config(CFG, || {
            assert!(matches!(
                resolve("loki@pi:/srv/x", "myrepo").unwrap(),
                Remote::Ssh { .. }
            ));
            assert!(matches!(
                resolve("/srv/local", "myrepo").unwrap(),
                Remote::Path(_)
            ));
        });
    }

    #[test]
    fn missing_config_errors_clearly() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("gyat-host-missing-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let old = std::env::var("GYAT_CONFIG").ok();
        unsafe { std::env::set_var("GYAT_CONFIG", dir.join("nope.toml")) };
        let e = resolve("", "myrepo").unwrap_err();
        assert!(e.contains("gyat setup"), "unexpected: {e}");
        let e = resolve("bareword", "myrepo").unwrap_err();
        assert!(e.contains("gyat setup"), "unexpected: {e}");
        match old {
            Some(v) => unsafe { std::env::set_var("GYAT_CONFIG", v) },
            None => unsafe { std::env::remove_var("GYAT_CONFIG") },
        }
    }
}

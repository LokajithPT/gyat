use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub repo: RepoSection,
    #[serde(default)]
    pub client: Option<ClientSection>,
    #[serde(default)]
    pub ignore: Option<IgnoreSection>,
    #[serde(default)]
    pub ssh: Option<SshSection>,
    #[serde(default)]
    pub server: Option<ServerSection>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepoSection {
    pub name: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub server: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ClientSection {
    pub username: String,
    #[serde(default)]
    pub repo: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct IgnoreSection {
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SshSection {
    #[serde(default)]
    pub key_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ServerSection {
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            repo: RepoSection { name: String::new(), username: String::new(), server: String::new() },
            client: None,
            ignore: None,
            ssh: None,
            server: None,
        }
    }
}

pub fn config_path() -> &'static Path {
    Path::new(".gyt/config.toml")
}

pub fn load() -> Result<Config, String> {
    let p = config_path();
    if !p.exists() {
        return Err("not a gyat repo: .gyt/config.toml missing".to_string());
    }
    let s = fs::read_to_string(p).map_err(|e| format!("read config: {e}"))?;
    toml::from_str(&s).map_err(|e| format!("parse config: {e}"))
}

pub fn save(cfg: &Config) -> Result<(), String> {
    let s = toml::to_string_pretty(cfg).map_err(|e| format!("serialize config: {e}"))?;
    fs::write(config_path(), s).map_err(|e| format!("write config: {e}"))?;
    Ok(())
}

pub fn is_ignored(path: &str, cfg: &Config) -> bool {
    let Some(ign) = &cfg.ignore else { return false; };
    for pat in &ign.files {
        if pat.ends_with('/') {
            if path.starts_with(pat.as_str()) || path.starts_with(pat.trim_end_matches('/')) {
                return true;
            }
        } else if pat.contains('*') {
            // very small glob: *.ext or *name* or prefix*
            if pat.starts_with("*.") {
                let ext = &pat[1..];
                if path.ends_with(ext) { return true; }
            } else if pat.ends_with('*') {
                let prefix = pat.trim_end_matches('*');
                if path.starts_with(prefix) { return true; }
            } else if pat.starts_with('*') {
                let suffix = pat.trim_start_matches('*');
                if path.ends_with(suffix) { return true; }
            } else {
                let core = pat.replace('*', "");
                if path.contains(&core) { return true; }
            }
        } else if path == pat {
            return true;
        }
    }
    false
}

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use super::config::{Config, IgnoreSection, RepoSection, SshSection};
use super::repo;

fn already_initialized() -> bool {
    Path::new(".gyt").is_dir()
}

fn prompt(q: &str) -> String {
    print!("{}: ", q);
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    input.trim().to_string()
}

pub fn init() -> Result<(), String> {
    if already_initialized() {
        return Err("already initialized (.gyt exists)".to_string());
    }
    let repo = prompt("repo name");
    if repo.is_empty() {
        return Err("repo name required".to_string());
    }
    // The repo name becomes a directory on the server. A name containing `/`
    // or shell metacharacters creates a nested path that `gyat list` cannot
    // show and `gyat clone` cannot address, so refuse it up front.
    if repo.contains('/') || repo.contains("..") || repo.contains(' ')
        || repo.contains(';') || repo.contains('&') || repo.contains('|')
        || repo.contains('$') || repo.contains('`') || repo.contains('<')
        || repo.contains('>') || repo.contains('*') || repo.contains('?')
        || repo.contains('\'') || repo.contains('"') || repo.contains('\\')
    {
        return Err(format!(
            "invalid repo name '{repo}'\nhint: use letters, digits, `-` and `_` only"
        ));
    }
    let mut username = prompt("username");
    let static_cfg = super::host::load();
    let server_prompt = match &static_cfg {
        Some(st) => format!(
            "server [static: {} — enter for static, or a full remote]",
            st.describe()
        ),
        None => "server [user@host:/path, ssh://.., local path, or empty for default]".to_string(),
    };
    let mut server = prompt(&server_prompt);
    let ssh_key = prompt("ssh key path [~/.ssh/id_ed25519]");
    if username.is_empty() {
        username = "loki".to_string();
        println!("username: loki (default)");
    } else {
        println!("username: {}", username);
    }
    if server.is_empty() {
        if static_cfg.is_some() {
            println!("server: static (from ~/.gyatconfig.toml)");
        } else {
            server = "127.0.0.1:8081".to_string();
            println!(
                "server: 127.0.0.1:8081 (default; hint: `gyat setup` once to never type this)"
            );
        }
    } else {
        println!("server: {}", server);
    }
    let ssh_key = if ssh_key.is_empty() {
        "~/.ssh/id_ed25519".to_string()
    } else {
        ssh_key
    };
    println!(
        "\n summary: repo={} username={} server={} ssh={}",
        repo,
        username,
        if server.is_empty() {
            "(static)".to_string()
        } else {
            server.clone()
        },
        ssh_key
    );
    fs::create_dir_all(".gyt").map_err(|e| format!("create .gyt: {e}"))?;
    fs::create_dir_all(repo::stages_root()).map_err(|e| format!("create stages: {e}"))?;
    fs::create_dir_all(repo::current_root()).map_err(|e| format!("create current: {e}"))?;
    fs::create_dir_all(repo::commits_root()).map_err(|e| format!("create commits: {e}"))?;
    let cfg = Config {
        repo: RepoSection {
            name: repo.clone(),
            username: username.clone(),
            server: server.clone(),
        },
        client: None,
        ignore: Some(IgnoreSection {
            files: vec![
                ".gyt/".to_string(),
                "target/".to_string(),
                ".git/".to_string(),
                "node_modules/".to_string(),
                "gyat-server-data/".to_string(),
                "server.toml".to_string(),
            ],
        }),
        ssh: Some(SshSection { key_path: ssh_key }),
        server: None,
    };
    let s = toml::to_string_pretty(&cfg).map_err(|e| format!("serialize config: {e}"))?;
    fs::write(".gyt/config.toml", s).map_err(|e| format!("write config: {e}"))?;
    fs::create_dir_all(repo::refs_heads_root()).map_err(|e| format!("create refs: {e}"))?;
    fs::write(repo::branch_path("main"), "").map_err(|e| format!("write main branch: {e}"))?;
    repo::set_head_branch("main")?;
    super::settings::ensure_default(&repo)?;
    super::gyatignore::ensure_default()?;
    println!("initialized gyat repo `{}` on branch main", cfg.repo.name);
    println!("  config: .gyt/config.toml");
    println!("  settings: gyat.toml (compression, chunks)");
    println!("  ignore: .gyatignore");
    println!("  stages: .gyt/stages/");
    println!("  commits: .gyt/commits/");
    println!("  HEAD: .gyt/HEAD");
    println!("next: `gyat add <files>` then `gyat commit -m \"msg\"` (+ `gyat push` for remote)");
    Ok(())
}

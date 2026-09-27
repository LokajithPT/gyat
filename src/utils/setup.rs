//! `gyat setup`: configure your (static, unchanging) server once.

use std::io::{self, Write};

use super::host::{StaticConfig, StaticServer};

fn prompt(q: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{q}: ");
    } else {
        print!("{q} [{default}]: ");
    }
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    let input = input.trim().to_string();
    if input.is_empty() {
        default.to_string()
    } else {
        input
    }
}

pub fn setup(
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    base: Option<String>,
    key: Option<String>,
) -> Result<(), String> {
    let existing = super::host::load().unwrap_or_default();
    let e = &existing.server;

    let host = host.unwrap_or_else(|| prompt("server host (tailscale ip or name)", &e.host));
    if host.trim().is_empty() {
        return Err("host required (e.g. 100.81.91.113)".to_string());
    }
    let user_default = e.user.clone().unwrap_or_default();
    let user = user.unwrap_or_else(|| prompt("server user", &user_default));
    let user = if user.trim().is_empty() { None } else { Some(user) };
    let base = base.unwrap_or_else(|| prompt("server base dir for repos", &e.base));
    let base = if base.trim().is_empty() { "gyat-server-data".to_string() } else { base };
    let key = key.unwrap_or_else(|| prompt("ssh key", &e.key));
    let key = if key.trim().is_empty() { "~/.ssh/id_ed25519".to_string() } else { key };

    let cfg = StaticConfig {
        server: StaticServer {
            host: host.trim().to_string(),
            user,
            port: port.or(e.port),
            base,
            key,
            bin: if e.bin.is_empty() { "gyat-server".to_string() } else { e.bin.clone() },
        },
    };
    super::host::save(&cfg)?;
    println!("static server saved to {}", super::host::config_path().display());
    println!("  {}", cfg.describe());
    println!("from now on: just `gyat push`, `gyat pull`, `gyat clone <name>` — no URLs");
    Ok(())
}

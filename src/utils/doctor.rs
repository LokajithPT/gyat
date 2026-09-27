//! `gyat doctor`: check the whole chain end to end (config -> ssh -> server).

use super::host;

fn ok(msg: &str) {
    println!("  ok    {msg}");
}
fn warn(msg: &str) {
    println!("  warn  {msg}");
}
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn bad(msg: &str) {
    FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
    println!("  fail  {msg}");
}

pub fn doctor() -> Result<(), String> {
    println!("gyat doctor");
    println!();

    // 1. static config
    print!("static server config ({})", host::config_path().display());
    println!();
    match host::load() {
        Some(st) => {
            if st.server.host.trim().is_empty() {
                bad("no host set — run `gyat setup`");
            } else {
                ok(&st.describe());
            }
            if st.server.key.ends_with("id_ed25519") || st.server.key.ends_with("id_rsa") {
                ok(&format!("key {}", st.server.key));
            } else {
                warn(&format!("unusual key path {}", st.server.key));
            }
        }
        None => bad("missing — run `gyat setup --host <ip> --user <name>`"),
    }
    println!();

    // 2. local repo (optional)
    if super::repo::gyt_root().exists() {
        print!("local repo");
        println!();
        let branch = super::repo::current_branch().unwrap_or_else(|| "(detached)".into());
        ok(&format!("on {branch}"));
        match super::config::load() {
            Ok(c) => {
                let server = if c.repo.server.trim().is_empty() {
                    "static (empty server field)".to_string()
                } else {
                    c.repo.server.clone()
                };
                ok(&format!("repo {} -> {}", c.repo.name, server));
            }
            Err(e) => bad(&format!("config: {e}")),
        }
    } else {
        print!("local repo");
        println!();
        warn("not inside a gyat repo (fine if you only want to check the server)");
    }
    println!();

    // 3. ssh + server round trip
    let st = match host::load() {
        Some(s) if !s.server.host.trim().is_empty() => s,
        _ => {
            println!("ssh / server");
            println!();
            warn("skipped: no static server configured");
            return Err("doctor found problems (see above)".to_string());
        }
    };
    print!("ssh + server");
    println!();
    let dest = match &st.server.user {
        Some(u) => format!("{u}@{}", st.server.host),
        None => st.server.host.clone(),
    };
    let key = host::effective_key(None);
    let key = if let Some(rest) = key.strip_prefix("~/") {
        std::env::var("HOME")
            .map(|h| format!("{h}/{rest}"))
            .unwrap_or(key)
    } else {
        key
    };
    // bare ssh handshake first (clear error if key/host is wrong)
    let target = super::remote::SshTarget {
        user: st.server.user.clone(),
        host: st.server.host.clone(),
        port: st.server.port,
        key_path: Some(key),
    };
    match target.run("echo ok", None) {
        Ok(_) => ok(&format!("ssh {dest} reachable")),
        Err(e) => {
            bad(&format!("ssh {dest}: {e}"));
            println!();
            println!("  check: tailscale status, the key, and that the box is online");
            return Err("doctor found problems (see above)".to_string());
        }
    }
    match target.run_server("list", &st.server.base, &[], None) {
        Ok(out) => {
            let n = String::from_utf8_lossy(&out)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count();
            ok(&format!("gyat-server responds, {n} repo(s) in {}", st.server.base));
        }
        Err(e) => bad(&format!("gyat-server: {e}")),
    }
    println!();
    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("doctor found problems (see above)".to_string());
    }
    println!("all good — `gyat push` / `gyat pull` / `gyat clone <name>` will just work");
    Ok(())
}

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(author, version, about = "gyat-server - T420-hosted gyat remote")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(long, default_value = "8081")]
    port: u16,
    #[arg(long, default_value = "./gyat-server-data")]
    dir: String,
    #[arg(long, default_value = "./server.toml")]
    config: String,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Run,
    Status,
    /// Receive a push bundle (reads stdin unless --bundle given).
    /// Transport: `gyat push` pipes a bundle through
    /// `ssh user@host gyat-server receive <repo>`.
    Receive {
        /// server-side repo path, e.g. /srv/gyat/myrepo
        repo: String,
        /// read bundle from file instead of stdin
        #[arg(long)]
        bundle: Option<String>,
        /// allow non-fast-forward branch updates
        #[arg(long)]
        force: bool,
    },
    /// Write a fetch bundle of <repo> to stdout.
    /// Transport: `gyat pull` runs `ssh user@host gyat-server fetch <repo>`.
    Fetch {
        /// server-side repo path
        repo: String,
    },
    /// List repos under a data dir, or branches of a repo.
    List {
        /// data dir or repo path (defaults to server data dir)
        path: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ServerConfig {
    server: ServerSection,
    dir: DirSection,
    chunks: ChunkSection,
    #[serde(default)]
    ssh: SshSection,
}

#[derive(Debug, Serialize, Deserialize)]
struct ServerSection {
    port: u16,
}
#[derive(Debug, Serialize, Deserialize)]
struct DirSection {
    path: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct ChunkSection {
    size: usize,
}
#[derive(Debug, Serialize, Deserialize, Default)]
struct SshSection {
    authorized_keys: String,
    host_key: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerSection { port: 8081 },
            dir: DirSection {
                path: "./gyat-server-data".to_string(),
            },
            chunks: ChunkSection { size: 4096 },
            ssh: SshSection {
                authorized_keys: "~/.ssh/authorized_keys".to_string(),
                host_key: "/etc/ssh/gyat_host_key".to_string(),
            },
        }
    }
}

fn load_config(path: &str) -> ServerConfig {
    if Path::new(path).exists() {
        if let Ok(s) = fs::read_to_string(path) {
            if let Ok(cfg) = toml::from_str(&s) {
                return cfg;
            }
        }
    }
    ServerConfig::default()
}

#[derive(Debug, Deserialize)]
struct CommitMeta {
    hash: String,
    parent: Option<String>,
    #[serde(default)]
    second_parent: Option<String>,
}

fn read_meta(repo: &Path, hash: &str) -> Option<CommitMeta> {
    let s = fs::read_to_string(repo.join("commits").join(hash).join("meta.toml")).ok()?;
    toml::from_str(&s).ok()
}

/// True if `old` is reachable from `new` following parent/second_parent links.
fn is_ancestor(repo: &Path, old: &str, new: &str) -> bool {
    if old == new {
        return true;
    }
    let mut stack = vec![new.to_string()];
    let mut seen = HashSet::new();
    while let Some(h) = stack.pop() {
        if !seen.insert(h.clone()) {
            continue;
        }
        if h == old {
            return true;
        }
        if let Some(m) = read_meta(repo, &h) {
            if let Some(p) = m.parent {
                stack.push(p);
            }
            if let Some(p) = m.second_parent {
                stack.push(p);
            }
        }
    }
    false
}

fn tmp_dir(tag: &str) -> Result<PathBuf, String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("gyat-server-{}-{}-{tag}", std::process::id(), id));
    fs::create_dir_all(&dir).map_err(|e| format!("tmpdir: {e}"))?;
    Ok(dir)
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<usize, String> {
    if !src.exists() {
        return Ok(0);
    }
    let mut n = 0;
    for entry in walkdir::WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            let rel = p.strip_prefix(src).unwrap();
            let target = dst.join(rel);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
            }
            if !target.exists() {
                fs::copy(p, &target).map_err(|e| format!("copy: {e}"))?;
                n += 1;
            }
        }
    }
    Ok(n)
}

fn do_receive(repo: &str, bundle: Option<&str>, force: bool) -> Result<(), String> {
    let repo_path = Path::new(repo);
    let incoming = tmp_dir("incoming")?;

    // read bundle from file or stdin
    if let Some(f) = bundle {
        gyat_bundle::unpack_file(Path::new(f), &incoming)?;
    } else {
        let mut stdin = std::io::stdin();
        let mut bytes = Vec::new();
        stdin
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read stdin: {e}"))?;
        if bytes.is_empty() {
            return Err("empty bundle on stdin".to_string());
        }
        let mut cur: &[u8] = &bytes;
        gyat_bundle::unpack_to(&mut cur, &incoming)?;
    }

    // validate: only commits/*/meta.toml + refs/heads/* allowed
    let mut commit_hashes: Vec<String> = vec![];
    let commits_dir = incoming.join("commits");
    if commits_dir.exists() {
        for entry in fs::read_dir(&commits_dir).map_err(|e| format!("read bundle commits: {e}"))? {
            let entry = entry.map_err(|e| format!("read entry: {e}"))?;
            let hash = entry.file_name().to_string_lossy().to_string();
            let meta = entry.path().join("meta.toml");
            if !meta.exists() {
                let _ = fs::remove_dir_all(&incoming);
                return Err(format!("bundle: commit {hash} missing meta.toml"));
            }
            let s = fs::read_to_string(&meta).map_err(|e| format!("bundle meta {hash}: {e}"))?;
            let parsed: CommitMeta =
                toml::from_str(&s).map_err(|e| format!("bundle meta {hash}: {e}"))?;
            if parsed.hash != hash {
                let _ = fs::remove_dir_all(&incoming);
                return Err(format!("bundle: commit dir {hash} != meta hash {}", parsed.hash));
            }
            commit_hashes.push(hash);
        }
    }

    // create repo on first push (friendly `git init --bare`-ish behavior)
    let created = !repo_path.exists();
    fs::create_dir_all(repo_path.join("commits")).map_err(|e| format!("mkdir commits: {e}"))?;
    fs::create_dir_all(repo_path.join("refs/heads")).map_err(|e| format!("mkdir refs: {e}"))?;

    // store missing commits (never overwrite existing — content-addressed)
    let mut new_commits = 0;
    for hash in &commit_hashes {
        let dst = repo_path.join("commits").join(hash);
        if !dst.exists() {
            copy_dir_all(&incoming.join("commits").join(hash), &dst)?;
            new_commits += 1;
        }
    }

    // update refs with fast-forward check
    let mut updated = vec![];
    let mut rejected = vec![];
    let mut created_refs = vec![];
    let incoming_refs = incoming.join("refs/heads");
    if incoming_refs.exists() {
        for entry in fs::read_dir(&incoming_refs).map_err(|e| format!("read bundle refs: {e}"))? {
            let entry = entry.map_err(|e| format!("read ref: {e}"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains('/') || name.contains("..") {
                rejected.push(format!("{name} (bad name)"));
                continue;
            }
            let new_hash = fs::read_to_string(entry.path())
                .map_err(|e| format!("read ref {name}: {e}"))?
                .trim()
                .to_string();
            if new_hash.is_empty() {
                continue;
            }
            // the commits a ref points at must be present now
            if !repo_path.join("commits").join(&new_hash).exists() {
                rejected.push(format!("{name} (missing commit {})", &new_hash[..8.min(new_hash.len())]));
                continue;
            }
            let dst = repo_path.join("refs/heads").join(&name);
            let old_hash = fs::read_to_string(&dst).unwrap_or_default().trim().to_string();
            if old_hash.is_empty() {
                fs::write(&dst, &new_hash).map_err(|e| format!("write ref {name}: {e}"))?;
                created_refs.push(format!("{name} -> {}", &new_hash[..8.min(new_hash.len())]));
            } else if old_hash == new_hash {
                // already up to date, nothing to do
            } else if force || is_ancestor(repo_path, &old_hash, &new_hash) {
                fs::write(&dst, &new_hash).map_err(|e| format!("write ref {name}: {e}"))?;
                updated.push(format!(
                    "{name} {} -> {}",
                    &old_hash[..8.min(old_hash.len())],
                    &new_hash[..8.min(new_hash.len())]
                ));
            } else {
                rejected.push(format!(
                    "{name} (non-fast-forward {} -> {})",
                    &old_hash[..8.min(old_hash.len())],
                    &new_hash[..8.min(new_hash.len())]
                ));
            }
        }
    }

    let _ = fs::remove_dir_all(&incoming);

    if created {
        println!("created repo {}", repo_path.display());
    }
    println!("ok: received {new_commits} new commit(s)");
    for r in &created_refs {
        println!("created {r}");
    }
    for r in &updated {
        println!("updated {r}");
    }
    for r in &rejected {
        println!("rejected {r}");
    }
    if rejected.is_empty() {
        Ok(())
    } else {
        Err(format!("push rejected: {}", rejected.join("; ")))
    }
}

fn do_fetch(repo: &str) -> Result<(), String> {
    let repo_path = Path::new(repo);
    if !repo_path.join("commits").exists() {
        return Err(format!("repo {} not found (no commits)", repo_path.display()));
    }
    // pack commits + refs straight to stdout
    let out = std::io::stdout();
    let mut out = out.lock();
    // pack both trees: build a staging view without touching the repo
    let stage = tmp_dir("fetch")?;
    copy_dir_all(&repo_path.join("commits"), &stage.join("commits"))?;
    copy_dir_all(&repo_path.join("refs"), &stage.join("refs"))?;
    let r = gyat_bundle::pack_dir(&stage, &mut out);
    let _ = fs::remove_dir_all(&stage);
    let n = r?;
    eprintln!("fetched {n} file(s) from {}", repo_path.display());
    Ok(())
}

fn do_list(path: Option<&str>, default_dir: &str) -> Result<(), String> {
    let target = path.unwrap_or(default_dir);
    let p = Path::new(target);
    if p.join("commits").exists() {
        // it's a repo: list branches
        let refs = p.join("refs/heads");
        if refs.exists() {
            for entry in fs::read_dir(&refs).map_err(|e| format!("read refs: {e}"))? {
                let entry = entry.map_err(|e| format!("read ref: {e}"))?;
                let name = entry.file_name().to_string_lossy().to_string();
                let hash = fs::read_to_string(entry.path()).unwrap_or_default().trim().to_string();
                println!("{name} {hash}");
            }
        } else {
            println!("(no branches)");
        }
    } else if p.exists() {
        // data dir: list repos (subdirs containing commits/)
        let mut repos = vec![];
        for entry in fs::read_dir(p).map_err(|e| format!("read dir: {e}"))? {
            let entry = entry.map_err(|e| format!("read entry: {e}"))?;
            if entry.path().join("commits").exists() {
                repos.push(entry.file_name().to_string_lossy().to_string());
            }
        }
        repos.sort();
        for r in repos {
            println!("{r}");
        }
    } else {
        return Err(format!("path {} not found", p.display()));
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();
    let cfg = load_config(&cli.config);

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Init => {
            let default_cfg = ServerConfig::default();
            let s = toml::to_string_pretty(&default_cfg).map_err(|e| e.to_string())?;
            fs::write(&cli.config, s).map_err(|e| e.to_string())?;
            fs::create_dir_all(&cli.dir).map_err(|e| e.to_string())?;
            println!("server init: config at {} (port {}, chunks {}, dir {})", cli.config, default_cfg.server.port, default_cfg.chunks.size, default_cfg.dir.path);
            println!("SSH mode: authorized_keys={} host_key={}", default_cfg.ssh.authorized_keys, default_cfg.ssh.host_key);
            println!("transport: one-shot commands over ssh, e.g.");
            println!("  ssh gyat@host gyat-server receive /srv/gyat/myrepo < bundle");
            println!("  ssh gyat@host gyat-server fetch /srv/gyat/myrepo > bundle");
            Ok(())
        }
        Commands::Run => {
            // Transport is one-shot commands over ssh (like git-receive-pack);
            // there is no daemon to run. Keep the subcommand for compat.
            println!("gyat-server has no daemon: transport is one-shot commands over ssh.");
            println!("server-side: `gyat-server receive <repo>` reads a push bundle on stdin,");
            println!("             `gyat-server fetch <repo>` writes a fetch bundle on stdout.");
            println!("client-side: `gyat push` / `gyat pull` shell out to `ssh` automatically.");
            println!("data dir: {} (config {})", cfg.dir.path, cli.config);
            Ok(())
        }
        Commands::Status => {
            println!("server config: {} (exists={})", cli.config, Path::new(&cli.config).exists());
            println!("  port: {} dir: {} chunks: {}", cfg.server.port, cfg.dir.path, cfg.chunks.size);
            let data_exists = Path::new(&cfg.dir.path).exists();
            println!("  data dir exists: {data_exists}");
            if data_exists {
                let count = fs::read_dir(&cfg.dir.path).map(|d| d.count()).unwrap_or(0);
                println!("  repos: {count}");
            }
            Ok(())
        }
        Commands::Receive { repo, bundle, force } => do_receive(&repo, bundle.as_deref(), force),
        Commands::Fetch { repo } => do_fetch(&repo),
        Commands::List { path } => do_list(path.as_deref(), &cfg.dir.path),
    }
}

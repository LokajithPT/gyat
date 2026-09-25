use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(author, version, about = "gyat-server - T420 8GB, SSH git-like auth")]
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
struct ServerSection { port: u16 }
#[derive(Debug, Serialize, Deserialize)]
struct DirSection { path: String }
#[derive(Debug, Serialize, Deserialize)]
struct ChunkSection { size: usize }
#[derive(Debug, Serialize, Deserialize, Default)]
struct SshSection {
    authorized_keys: String,
    host_key: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerSection { port: 8081 },
            dir: DirSection { path: "./gyat-server-data".to_string() },
            chunks: ChunkSection { size: 4096 },
            ssh: SshSection { authorized_keys: "~/.ssh/authorized_keys".to_string(), host_key: "/etc/ssh/gyat_host_key".to_string() },
        }
    }
}

fn load_config(path: &str) -> ServerConfig {
    if Path::new(path).exists() {
        if let Ok(s) = fs::read_to_string(path) {
            if let Ok(cfg) = toml::from_str(&s) { return cfg; }
        }
    }
    ServerConfig::default()
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
            println!("next: `gyat-server run --port {} --dir {}`", cfg.server.port, cfg.dir.path);
        }
        Commands::Run => {
            let port = if cli.port != 8081 { cli.port } else { cfg.server.port };
            let dir = if cli.dir != "./gyat-server-data" { cli.dir.clone() } else { cfg.dir.path.clone() };
            println!("gyat-server starting (T420 local, SSH planned) - port {port} dir {dir} chunks {} ssh auth", cfg.chunks.size);
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            println!("  repo storage: {dir}/<user>/<repo>/commits/");
            println!("  SSH: clients `ssh -i ~/.ssh/id_ed25519 gyat@{} gyat-server receive` (stub)", port);
            println!("  HTTP stub would listen on :{port} - currently local-only mode, push/pull via SSH soon");
            println!("  server running... (stub, exit with Ctrl+C)");
            // TODO: real server: tiny_http or axum + ssh crate `russh`
            std::thread::park();
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
        }
    }
    Ok(())
}

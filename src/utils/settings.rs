use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GyatSettings {
    #[serde(default)]
    pub repo: RepoMeta,
    #[serde(default)]
    pub compression: CompressionSection,
    #[serde(default)]
    pub chunks: ChunkSection,
    #[serde(default)]
    pub server: ServerSection,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RepoMeta {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CompressionSection {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_algorithm")]
    pub algorithm: String, // gzip, zstd, none
    #[serde(default = "default_level")]
    pub level: u32,
}
impl Default for CompressionSection {
    fn default() -> Self { Self { enabled: true, algorithm: "gzip".to_string(), level: 6 } }
}
fn default_enabled() -> bool { true }
fn default_algorithm() -> String { "gzip".to_string() }
fn default_level() -> u32 { 6 }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChunkSection {
    #[serde(default = "default_chunk_size")]
    pub size: usize,
}
impl Default for ChunkSection {
    fn default() -> Self { Self { size: 4096 } }
}
fn default_chunk_size() -> usize { 4096 }

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ServerSection {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
}

impl Default for GyatSettings {
    fn default() -> Self {
        Self {
            repo: RepoMeta::default(),
            compression: CompressionSection::default(),
            chunks: ChunkSection::default(),
            server: ServerSection::default(),
        }
    }
}

pub fn gyat_toml_path() -> &'static Path { Path::new("gyat.toml") }

pub fn load() -> GyatSettings {
    let p = gyat_toml_path();
    if !p.exists() { return GyatSettings::default(); }
    if let Ok(s) = fs::read_to_string(p) {
        if let Ok(cfg) = toml::from_str(&s) { return cfg; }
    }
    GyatSettings::default()
}

pub fn save(cfg: &GyatSettings) -> Result<(), String> {
    let s = toml::to_string_pretty(cfg).map_err(|e| format!("serialize gyat.toml: {e}"))?;
    fs::write(gyat_toml_path(), s).map_err(|e| format!("write gyat.toml: {e}"))?;
    Ok(())
}

pub fn ensure_default(repo_name: &str) -> Result<(), String> {
    if gyat_toml_path().exists() { return Ok(()); }
    let mut cfg = GyatSettings::default();
    cfg.repo.name = repo_name.to_string();
    cfg.compression.enabled = true;
    cfg.compression.algorithm = "gzip".to_string();
    cfg.compression.level = 6;
    cfg.chunks.size = 4096;
    save(&cfg)
}

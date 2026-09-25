use std::fs;
use std::path::Path;
use walkdir::WalkDir;

use super::config;

pub fn add(files: &[String]) -> Result<(), String> {
    if files.is_empty() {
        return Err("add: no files specified".to_string());
    }
    super::repo::ensure_repo()?;
    let cfg = config::load().unwrap_or_default();
    let stages_root = super::repo::stages_root();
    if !stages_root.exists() {
        return Err("not initialized: .gyt/stages missing".to_string());
    }

    let mut staged = 0usize;
    let mut ignored = 0usize;
    let mut missing = 0usize;

    for pattern in files {
        // support "." and directory recursion
        let src_path = Path::new(pattern);
        if pattern == "." {
            for entry in WalkDir::new(".").into_iter().filter_map(|e| e.ok()) {
                let p = entry.path();
                // skip .gyt and target and other ignored by config
                let rel = p.strip_prefix(".").unwrap_or(p).to_string_lossy().to_string();
                if rel.is_empty() { continue; }
                if rel.starts_with(".gyt/") || rel == ".gyt" { continue; }
                if rel.starts_with("target/") && config::is_ignored(&rel, &cfg) { continue; }
                if config::is_ignored(&rel, &cfg) { ignored += 1; continue; }
                if p.is_file() {
                    stage_file(p, &stages_root, &rel)?;
                    staged += 1;
                    println!("staged {}", rel);
                }
            }
            continue;
        }

        if !src_path.exists() {
            // try glob-like: if file not found, warn but continue
            eprintln!("add: file not found {}", pattern);
            missing += 1;
            continue;
        }

        if src_path.is_dir() {
            for entry in WalkDir::new(src_path).into_iter().filter_map(|e| e.ok()) {
                let p = entry.path();
                if p.is_file() {
                    let rel = p.strip_prefix(".").unwrap_or(p).to_string_lossy().to_string();
                    let rel = rel.trim_start_matches("./").to_string();
                    if config::is_ignored(&rel, &cfg) { ignored += 1; continue; }
                    stage_file(p, &stages_root, &rel)?;
                    staged += 1;
                    println!("staged {}", rel);
                }
            }
        } else {
            let rel = pattern.trim_start_matches("./").to_string();
            if config::is_ignored(&rel, &cfg) {
                println!("ignored {}", rel);
                ignored += 1;
                continue;
            }
            stage_file(src_path, &stages_root, &rel)?;
            staged += 1;
            println!("staged {}", rel);
        }
    }

    if staged == 0 && missing > 0 && ignored == 0 {
        return Err("nothing staged".to_string());
    }
    println!("add done: {staged} staged, {ignored} ignored, {missing} missing");
    Ok(())
}

fn stage_file(src: &Path, stages_root: &Path, rel: &str) -> Result<(), String> {
    let dest = stages_root.join(rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }
    fs::copy(src, &dest).map_err(|e| format!("stage {rel}: {e}"))?;
    Ok(())
}

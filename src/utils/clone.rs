use std::fs;
use std::path::{Path, PathBuf};

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() { return Err(format!("source {} not found", src.display())); }
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {dst:?}: {e}"))?;
    for entry in walkdir::WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            let rel = p.strip_prefix(src).unwrap();
            let target = dst.join(rel);
            if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?; }
            fs::copy(p, &target).map_err(|e| format!("copy {rel:?}: {e}"))?;
        }
    }
    Ok(())
}

pub fn clone_repo(source: &str, dest: Option<String>) -> Result<(), String> {
    // ssh source? fetch a bundle first, then build from the unpacked tree.
    if let super::remote::Remote::Ssh { user, host, port, path } =
        super::remote::parse_remote(source, "")
    {
        // repo name from remote path's last component
        let repo_name = Path::new(path.trim_end_matches('/'))
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("repo")
            .to_string();
        return clone_ssh(user, host, port, path, repo_name, dest);
    }

    let src_path = Path::new(source);
    if !src_path.exists() {
        return Err(format!("clone source {} not found (try ./gyat-server-data/<repo>)", source));
    }
    // determine repo name from source path
    let repo_name = src_path.file_name().and_then(|s| s.to_str()).unwrap_or("repo").to_string();
    let dest_path = dest.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(&repo_name));

    if dest_path.exists() {
        return Err(format!("clone dest {} already exists", dest_path.display()));
    }

    // check if source is a gyat repo (has commits/refs) or a server repo (commits/refs directly)
    // server repo layout: <repo>/commits + <repo>/refs/heads
    // if source is server data root, we need to handle but for now assume source is repo dir
    let src_commits = if src_path.join("commits").exists() {
        src_path.join("commits")
    } else if src_path.join(".gyt/commits").exists() {
        src_path.join(".gyt/commits")
    } else {
        return Err(format!("source {} has no commits", source));
    };
    let src_refs = if src_path.join("refs/heads").exists() {
        src_path.join("refs/heads")
    } else if src_path.join(".gyt/refs/heads").exists() {
        src_path.join(".gyt/refs/heads")
    } else {
        src_path.join("refs/heads")
    };

    build_from_dirs(&src_commits, &src_refs, &repo_name, source, dest_path)
}

fn clone_ssh(
    user: Option<String>,
    host: String,
    port: Option<u16>,
    path: String,
    repo_name: String,
    dest: Option<String>,
) -> Result<(), String> {
    let dest_path = dest.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(&repo_name));
    if dest_path.exists() {
        return Err(format!("clone dest {} already exists", dest_path.display()));
    }
    let dest_label = dest_path.display().to_string();
    fs::create_dir_all(&dest_path).map_err(|e| format!("mkdir dest: {e}"))?;
    let r = (|| -> Result<(), String> {
        let target = super::remote::SshTarget { user: user.clone(), host: host.clone(), port, key_path: None };
        let dest_label_ssh = match &user {
            Some(u) => format!("{u}@{host}:{path}"),
            None => format!("{host}:{path}"),
        };
        println!("cloning {dest_label_ssh} ...");
        let remote_cmd = super::remote::remote_cmd(&super::remote::server_bin(), "fetch", &path, &[]);
        let bytes = target.run(&remote_cmd, None)?;
        let tmp = super::remote::unpack_bundle_to_temp(&bytes)?;
        let r = build_from_dirs(
            &tmp.join("commits"),
            &tmp.join("refs/heads"),
            &repo_name,
            &dest_label_ssh,
            dest_path.clone(),
        );
        let _ = fs::remove_dir_all(&tmp);
        r
    })();
    if r.is_err() {
        let _ = fs::remove_dir_all(&dest_path);
    }
    r?;
    println!("cloned into {dest_label}");
    Ok(())
}

fn build_from_dirs(
    src_commits: &Path,
    src_refs: &Path,
    repo_name: &str,
    source: &str,
    dest_path: PathBuf,
) -> Result<(), String> {
    // create dest as new gyat repo
    fs::create_dir_all(&dest_path).map_err(|e| format!("mkdir dest: {e}"))?;
    let orig_dir = std::env::current_dir().map_err(|e| e.to_string())?;
    std::env::set_current_dir(&dest_path).map_err(|e| format!("chdir: {e}"))?;

    // init new repo
    // we need to init with same repo name, but we can't prompt in clone, so auto
    {
        // create .gyt structure like init but without prompt
        fs::create_dir_all(".gyt").map_err(|e| format!("create .gyt: {e}"))?;
        fs::create_dir_all(".gyt/stages").map_err(|e| format!("create stages: {e}"))?;
        fs::create_dir_all(".gyt/current").map_err(|e| format!("create current: {e}"))?;
        fs::create_dir_all(".gyt/commits").map_err(|e| format!("create commits: {e}"))?;
        fs::create_dir_all(".gyt/refs/heads").map_err(|e| format!("create refs: {e}"))?;

        // read source repo name for config? use dest repo name
        let cfg_str = format!(r#"
[repo]
name = "{repo_name}"
username = "loki"
server = "{source}"
"#);
        fs::write(".gyt/config.toml", cfg_str).map_err(|e| format!("write config: {e}"))?;

        // settings and ignore
        let settings_str = format!(r#"[repo]
name = "{repo_name}"

[compression]
enabled = true
algorithm = "gzip"
level = 6

[chunks]
size = 4096

[server]
host = ""
port = 0
"#);
        fs::write("gyat.toml", settings_str).map_err(|e| format!("write gyat.toml: {e}"))?;
        // reuse same defaults as init (target/ + node_modules/ guaranteed)
        super::gyatignore::ensure_default()?;

        // copy commits
        let dst_commits = Path::new(".gyt/commits");
        copy_dir_all(&src_commits, dst_commits)?;

        // copy refs
        let dst_refs = Path::new(".gyt/refs/heads");
        copy_dir_all(&src_refs, dst_refs)?;

        // set HEAD to main if exists, else first branch
        let branches: Vec<String> = fs::read_dir(dst_refs).map(|d| d.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        let head_branch = if branches.contains(&"main".to_string()) { "main".to_string() } else { branches.first().cloned().unwrap_or_else(|| "main".to_string()) };
        fs::write(".gyt/HEAD", format!("ref: refs/heads/{head_branch}")).map_err(|e| format!("write HEAD: {e}"))?;

        // checkout HEAD
        if let Ok(hash) = fs::read_to_string(format!(".gyt/refs/heads/{head_branch}")) {
            let hash = hash.trim().to_string();
            if !hash.is_empty() {
                let snap = PathBuf::from(format!(".gyt/commits/{hash}/snapshot"));
                if snap.exists() {
                    for entry in walkdir::WalkDir::new(&snap).into_iter().filter_map(|e| e.ok()) {
                        let p = entry.path();
                        if p.is_file() {
                            let rel = p.strip_prefix(&snap).unwrap();
                            let mut rel_str = rel.to_string_lossy().to_string();
                            let is_gz = rel_str.ends_with(".gz");
                            if is_gz { rel_str.truncate(rel_str.len() - 3); }
                            let dst = Path::new(&rel_str);
                            if let Some(parent) = dst.parent() { let _ = fs::create_dir_all(parent); }
                            if is_gz {
                                let _ = crate::utils::compression::decompress_file(p, dst);
                            } else {
                                let _ = fs::copy(p, dst);
                            }
                        }
                    }
                }
            }
        }
    }

    std::env::set_current_dir(orig_dir).map_err(|e| e.to_string())?;
    println!("cloned {} -> {} (branch {})", source, dest_path.display(), "main");
    println!("next: cd {} && gyat status", dest_path.display());
    Ok(())
}

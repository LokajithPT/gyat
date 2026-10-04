use std::fs;
use std::path::{Path, PathBuf};

pub fn gyt_root() -> &'static Path { Path::new(".gyt") }
pub fn stages_root() -> PathBuf { gyt_root().join("stages") }
pub fn commits_root() -> PathBuf { gyt_root().join("commits") }
pub fn head_path() -> PathBuf { gyt_root().join("HEAD") }
pub fn current_root() -> PathBuf { gyt_root().join("current") }
pub fn refs_heads_root() -> PathBuf { gyt_root().join("refs/heads") }
pub fn branch_path(name: &str) -> PathBuf { refs_heads_root().join(name) }

pub fn ensure_repo() -> Result<(), String> {
    if !gyt_root().exists() {
        return Err("not a gyat repo: .gyt missing. run `gyat init`".to_string());
    }
    Ok(())
}

pub fn read_head() -> Option<String> {
    let s = fs::read_to_string(head_path()).ok()?.trim().to_string();
    if s.is_empty() { return None; }
    if s.starts_with("ref: ") {
        let ref_path = s.trim_start_matches("ref: ").trim();
        let branch_file = gyt_root().join(ref_path);
        if let Ok(h) = fs::read_to_string(branch_file) {
            let h = h.trim().to_string();
            if !h.is_empty() { return Some(h); }
        }
        return None;
    }
    Some(s)
}


// ---- durable writes -------------------------------------------------------
// A plain fs::write can leave a truncated file behind if the machine loses
// power the moment after the write returns, which is how a ref or a snapshot
// ends up half-written. These helpers force the bytes to disk before the
// caller is told the write succeeded.

/// fsync a directory so a rename into it is durable.
pub fn fsync_dir(dir: &Path) {
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
}

/// Write `data` to `path` so that a crash leaves either the old contents or
/// the complete new ones, never a partial file.
pub fn write_durable(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    let file_name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = parent.join(format!(".{file_name}.tmp"));
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
        f.write_all(data).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        f.sync_all().map_err(|e| format!("fsync {}: {e}", tmp.display()))?;
    }
    fs::rename(&tmp, path).map_err(|e| format!("rename into {}: {e}", path.display()))?;
    fsync_dir(parent);
    Ok(())
}

pub fn write_head(hash: &str) -> Result<(), String> {
    let head_content = fs::read_to_string(head_path()).unwrap_or_default();
    if head_content.starts_with("ref: ") {
        let ref_path = head_content.trim_start_matches("ref: ").trim().to_string();
        let branch_file = gyt_root().join(&ref_path);
        if let Some(p) = branch_file.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir refs: {e}"))?; }
        write_durable(&branch_file, hash.as_bytes())
    } else {
        write_durable(&head_path(), hash.as_bytes())
    }
}

pub fn current_branch() -> Option<String> {
    let s = fs::read_to_string(head_path()).ok()?.trim().to_string();
    if s.starts_with("ref: ") {
        let ref_path = s.trim_start_matches("ref: ").trim();
        if let Some(name) = ref_path.strip_prefix("refs/heads/") {
            return Some(name.to_string());
        }
    }
    None
}

pub fn set_head_branch(branch: &str) -> Result<(), String> {
    let ref_str = format!("ref: refs/heads/{branch}");
    write_durable(&head_path(), ref_str.as_bytes())
}

#[allow(dead_code)]
pub fn is_detached() -> bool {
    if let Ok(s) = fs::read_to_string(head_path()) {
        !s.trim().starts_with("ref: ")
    } else { true }
}

pub fn list_branches() -> Vec<String> {
    let root = refs_heads_root();
    if !root.exists() { return vec![]; }
    let mut v = vec![];
    if let Ok(entries) = fs::read_dir(&root) {
        for e in entries.flatten() {
            if e.path().is_file() {
                if let Some(name) = e.file_name().to_str() { v.push(name.to_string()); }
            }
        }
    }
    v.sort();
    v
}

pub fn branch_exists(name: &str) -> bool { branch_path(name).exists() }

pub fn read_branch(name: &str) -> Option<String> {
    fs::read_to_string(branch_path(name)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn write_branch(name: &str, hash: &str) -> Result<(), String> {
    let p = branch_path(name);
    if let Some(parent) = p.parent() { fs::create_dir_all(parent).map_err(|e| format!("mkdir refs: {e}"))?; }
    write_durable(&p, hash.as_bytes())
}

pub fn merge_head_path() -> PathBuf { gyt_root().join("MERGE_HEAD") }
pub fn read_merge_head() -> Option<String> {
    fs::read_to_string(merge_head_path()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
pub fn write_merge_head(hash: &str) -> Result<(), String> {
    write_durable(&merge_head_path(), hash.as_bytes())
}
pub fn clear_merge_head() -> Result<(), String> {
    let p = merge_head_path();
    if p.exists() { fs::remove_file(p).map_err(|e| format!("clear MERGE_HEAD: {e}"))?; }
    Ok(())
}

pub fn list_commits() -> Vec<String> {
    let root = commits_root();
    if !root.exists() { return vec![]; }
    let mut v = vec![];
    if let Ok(entries) = fs::read_dir(&root) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    if name.len() >= 6 { v.push(name.to_string()); }
                }
            }
        }
    }
    v.sort();
    v
}

/// Tracked files whose worktree contents differ from HEAD's snapshot.
///
/// These are the files a checkout would silently overwrite. Untracked files
/// are not at risk (a checkout never removes them) and are left out.
pub fn dirty_tracked() -> Vec<String> {
    let Some(head) = read_head() else { return vec![] };
    let snap = commit_snapshot_root(&head);
    if !snap.exists() { return vec![]; }
    let mut out = vec![];
    collect_dir(&snap, &snap, &mut out);
    out.sort();
    out.into_iter()
        .filter_map(|rel| {
            let key = rel.to_string_lossy().to_string();
            let key = key.strip_suffix(".gz").map(|s| s.to_string()).unwrap_or(key);
            let stored = snap.join(&rel);
            let worktree = Path::new(".").join(&key);
            let head_bytes = crate::utils::compression::read_bytes_maybe_compressed(&stored).ok()?;
            match fs::read(&worktree) {
                Ok(cur) if cur == head_bytes => None,
                Ok(_) => Some(key),
                // tracked but missing from the worktree: also a local change
                Err(_) => Some(key),
            }
        })
        .collect()
}

fn collect_dir(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() { collect_dir(root, &p, out); }
            else if let Ok(rel) = p.strip_prefix(root) { out.push(rel.to_path_buf()); }
        }
    }
}

pub fn commit_path(hash: &str) -> PathBuf { commits_root().join(hash) }
pub fn commit_meta_path(hash: &str) -> PathBuf { commit_path(hash).join("meta.toml") }
pub fn commit_snapshot_root(hash: &str) -> PathBuf { commit_path(hash).join("snapshot") }
pub fn commit_deltas_root(hash: &str) -> PathBuf { commit_path(hash).join("deltas") }

/// One `~n` / `^n` step parsed off the end of a revision expression.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Step {
    /// `~n` — walk n first parents.
    First(usize),
    /// `^2` — the second parent of a merge commit.
    Second,
    /// `^3` and up: a commit can only have two parents, so this is an error.
    TooDeep(usize),
}

/// Resolve a revision expression to a full commit hash.
///
/// The base may be `HEAD`, `@`, a branch name, or a full/abbreviated hash.
/// It can be followed by any number of git-style ancestry operators:
/// `~n` (n first parents), `^`/`^1` (first parent) and `^2` (merge's second
/// parent). Operators apply left to right, so `HEAD~2^2` means "the second
/// parent of the second commit back".
pub fn resolve_rev(spec: &str) -> Result<String, String> {
    let spec = spec.trim();
    if spec.is_empty() { return Err("empty revision".to_string()); }
    let (base, steps, complete) = split_steps(spec);
    if !complete {
        return Err(format!("revision `{spec}`: unrecognised trailing characters"));
    }
    if base.is_empty() {
        return Err(format!(
            "revision `{spec}` has no base before `{}`",
            steps_label(&steps)
        ));
    }
    let mut hash = resolve_base(&base)?;
    for step in steps {
        hash = apply_step(&hash, step, spec)?;
    }
    Ok(hash)
}

fn steps_label(steps: &[Step]) -> String {
    steps.iter().map(|s| match s {
        Step::First(n) => format!("~{n}"),
        Step::Second => "^2".to_string(),
        Step::TooDeep(n) => format!("^{n}"),
    }).collect()
}

/// Split a revision into its base and its ancestry operators.
///
/// Scans left to right: the base is the leading run of characters that are
/// not `~`/`^`, then each operator is `<op>[digits]`. Note the digits follow
/// the operator (`HEAD~2`), which is why this cannot be parsed right to left.
fn split_steps(spec: &str) -> (String, Vec<Step>, bool) {
    let bytes = spec.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] != b'~' && bytes[i] != b'^' { i += 1; }
    let base = spec[..i].to_string();

    let rest = &spec[i..];
    let rb = rest.as_bytes();
    let mut steps: Vec<Step> = Vec::new();
    let mut j = 0;
    while j < rb.len() {
        let op = rb[j];
        if op != b'~' && op != b'^' { break; }
        j += 1;
        let num_start = j;
        while j < rb.len() && rb[j].is_ascii_digit() { j += 1; }
        let n = if num_start == j {
            1
        } else {
            rest[num_start..j].parse::<usize>().unwrap_or(usize::MAX)
        };
        steps.push(match op {
            b'~' if n == 0 => Step::First(0),
            b'~' => Step::First(n),
            _ if n == 0 => Step::First(0),
            _ if n == 1 => Step::First(1),
            _ if n == 2 => Step::Second,
            _ => Step::TooDeep(n),
        });
    }
    (base, steps, j == rb.len())
}

fn resolve_base(base: &str) -> Result<String, String> {
    if base == "HEAD" || base == "@" {
        return read_head().ok_or_else(|| "no commits yet (HEAD is unborn)".to_string());
    }
    if branch_exists(base) {
        return read_branch(base).ok_or_else(|| format!("branch {base} has no commits yet"));
    }
    if commit_path(base).exists() { return Ok(base.to_string()); }
    let mut matches: Vec<String> = list_commits().into_iter().filter(|h| h.starts_with(base)).collect();
    matches.sort();
    match matches.len() {
        0 => Err(format!("commit {base} not found")),
        1 => Ok(matches.remove(0)),
        _ => Err(format!("ambiguous commit prefix {base}: {matches:?}")),
    }
}

fn apply_step(hash: &str, step: Step, spec: &str) -> Result<String, String> {
    match step {
        Step::First(n) => {
            let mut cur = hash.to_string();
            for _ in 0..n {
                let meta = super::commit::load_meta(&cur)
                    .map_err(|e| format!("revision `{spec}`: {e}"))?;
                cur = meta.parent.ok_or_else(|| {
                    format!("revision `{spec}`: {cur} is a root commit, no parent to walk")
                })?;
            }
            Ok(cur)
        }
        Step::Second => {
            let meta = super::commit::load_meta(hash)
                .map_err(|e| format!("revision `{spec}`: {e}"))?;
            match meta.second_parent {
                Some(p) => Ok(p),
                None if meta.parent.is_some() => Err(format!(
                    "revision `{spec}`: {hash} is not a merge commit, it has no second parent"
                )),
                None => Err(format!("revision `{spec}`: {hash} is a root commit, no parent to walk")),
            }
        }
        Step::TooDeep(n) => Err(format!(
            "revision `{spec}`: {hash} has at most 2 parents, cannot take ^{n}"
        )),
    }
}

pub fn deletions_file() -> PathBuf { stages_root().join(".gyat-deleted") }

pub fn staged_deletions() -> Vec<String> {
    let p = deletions_file();
    if !p.exists() { return vec![]; }
    fs::read_to_string(&p)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

pub fn record_deletion(rel: &str) -> Result<(), String> {
    let mut cur = staged_deletions();
    if !cur.iter().any(|x| x == rel) {
        cur.push(rel.to_string());
        cur.sort();
        fs::write(deletions_file(), cur.join("\n") + "\n")
            .map_err(|e| format!("record deletion {rel}: {e}"))?;
    }
    // make sure a stale staged copy doesn't linger
    let staged_copy = stages_root().join(rel);
    if staged_copy.exists() {
        let _ = fs::remove_file(&staged_copy);
    }
    Ok(())
}

pub fn unrecord_deletion(rel: &str) -> Result<(), String> {
    let cur: Vec<String> = staged_deletions().into_iter().filter(|x| x != rel).collect();
    if cur.is_empty() {
        let _ = fs::remove_file(deletions_file());
    } else {
        fs::write(deletions_file(), cur.join("\n") + "\n")
            .map_err(|e| format!("unrecord deletion {rel}: {e}"))?;
    }
    Ok(())
}

pub fn staged_files() -> Vec<PathBuf> {
    let root = stages_root();
    let mut out = vec![];
    if !root.exists() { return out; }
    collect_files(&root, &root, &mut out);
    out.into_iter()
        .filter(|p| {
            let s = p.to_string_lossy();
            s != ".gyat-deleted" && !s.starts_with(".gyat-deleted/")
        })
        .collect()
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() { collect_files(base, &p, out); } else if p.is_file() { if let Ok(rel) = p.strip_prefix(base) { out.push(rel.to_path_buf()); } }
        }
    }
}

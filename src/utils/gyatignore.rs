use std::fs;
use std::path::Path;
use glob::Pattern;

pub fn load_patterns() -> Vec<String> {
    let mut pats = vec![];
    // from .gyt/config.toml [ignore] is handled elsewhere; this is for .gyatignore file
    let p = Path::new(".gyatignore");
    if !p.exists() { return pats; }
    if let Ok(s) = fs::read_to_string(p) {
        for line in s.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') { continue; }
            // support ! negation later? for now just patterns
            if l.starts_with('!') { continue; }
            pats.push(l.to_string());
        }
    }
    pats
}

pub fn is_ignored_by_file(path: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        // directory pattern "target/" or "foo/" - match anywhere
        if pat.ends_with('/') {
            let dir = pat.trim_end_matches('/');
            if path == dir || path.starts_with(pat.as_str()) || path.contains(&format!("/{dir}/")) || path.contains(&format!("/{dir}")) || path.starts_with(&format!("{dir}/")) { return true; }
            continue;
        }
        // exact match
        if pat == path { return true; }
        // glob match using glob crate: covers *, **, ?, [...]
        if let Ok(pattern) = Pattern::new(pat) {
            if pattern.matches(path) { return true; }
            // also match basename for patterns like "*.log"
            if let Some(basename) = Path::new(path).file_name().and_then(|s| s.to_str()) {
                if pattern.matches(basename) { return true; }
            }
        }
        // fallback simple contains for patterns with **
        if pat.contains("**") {
            let core = pat.replace("**", "");
            if path.contains(&core) { return true; }
        }
    }
    false
}

const REQUIRED: &[&str] = &[
    ".gyt/",
    "target/",
    ".git/",
    "node_modules/",
    "gyat-server-data/",
    "server.toml",
    "gyat",
    "gyat-server",
    "*.delta",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_pattern_matches_anywhere() {
        let pats = vec!["node_modules/".to_string(), "target/".to_string()];
        assert!(is_ignored_by_file("node_modules/react/index.js", &pats));
        assert!(is_ignored_by_file("frontend/node_modules/lodash/index.js", &pats));
        assert!(is_ignored_by_file("packages/a/node_modules/foo/index.js", &pats));
        assert!(is_ignored_by_file("target/debug/out.txt", &pats));
        assert!(is_ignored_by_file("delta/target/debug/x.d", &pats));
        assert!(!is_ignored_by_file("src/main.rs", &pats));
        assert!(!is_ignored_by_file("my_target/file.txt", &pats));
    }

    #[test]
    fn glob_and_exact_patterns() {
        let pats = vec!["*.delta".to_string(), "server.toml".to_string(), "*.log".to_string()];
        assert!(is_ignored_by_file("a.delta", &pats));
        assert!(is_ignored_by_file("sub/b.delta", &pats));
        assert!(is_ignored_by_file("server.toml", &pats));
        assert!(is_ignored_by_file("sub/skip.log", &pats));
        assert!(!is_ignored_by_file("delta.txt", &pats));
        assert!(!is_ignored_by_file("src/main.rs", &pats));
    }
}

pub fn ensure_default() -> Result<(), String> {
    let p = Path::new(".gyatignore");
    if !p.exists() {
        let default = r#"# gyat ignore - like .gitignore
# dir/ matches that dir ANYWHERE (root or nested)
.gyt/
target/
.git/
node_modules/
gyat-server-data/
server.toml
gyat
gyat-server
*.delta
*.log
# js builds
dist/
build/
.next/
# add more
"#;
        fs::write(p, default).map_err(|e| format!("write .gyatignore: {e}"))?;
        return Ok(());
    }
    // file exists - merge missing required entries so every init/clone ends up with target/ + node_modules/
    let content = fs::read_to_string(p).map_err(|e| format!("read .gyatignore: {e}"))?;
    let existing: std::collections::HashSet<String> =
        content.lines().map(|l| l.trim().to_string()).collect();
    let mut missing = vec![];
    for r in REQUIRED {
        if !existing.contains(*r) {
            missing.push(r.to_string());
        }
    }
    if !missing.is_empty() {
        let mut out = content;
        if !out.ends_with('\n') {
            out.push('\n');
        }
        for m in &missing {
            out.push_str(&format!("{m}\n"));
        }
        fs::write(p, out).map_err(|e| format!("update .gyatignore: {e}"))?;
        println!("updated .gyatignore with: {}", missing.join(", "));
    }
    Ok(())
}

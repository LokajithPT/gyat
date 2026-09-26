//! End-to-end tests driving the real `gyat` binary through local flows:
//! init -> add -> commit -> status/log -> branch/travel/merge -> snip ->
//! push/pull/clone via path server.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use walkdir::WalkDir;

static N: AtomicU64 = AtomicU64::new(0);

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gyat"))
}

fn fresh_dir(name: &str) -> PathBuf {
    let id = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "gyat-e2e-{}-{}-{name}",
        std::process::id(),
        id
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct Out {
    ok: bool,
    text: String,
}

fn run(dir: &Path, args: &[&str], stdin: Option<&str>) -> Out {
    let mut cmd = Command::new(bin());
    cmd.current_dir(dir).args(args);
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn gyat");
    if let Some(input) = stdin {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Out {
        ok: out.status.success(),
        text,
    }
}

fn init_repo(dir: &Path, name: &str, server: &str) -> Out {
    // prompts: repo name, username, server, ssh key path
    let input = format!("{name}\nloki\n{server}\n\n");
    run(dir, &["init"], Some(&input))
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_default()
}

fn head(dir: &Path) -> String {
    read(&dir.join(".gyt/HEAD")).trim().to_string()
}

fn branch_hash(dir: &Path, branch: &str) -> String {
    read(&dir.join(format!(".gyt/refs/heads/{branch}")))
        .trim()
        .to_string()
}

fn snapshot_files(dir: &Path, hash: &str) -> Vec<String> {
    let root = dir.join(format!(".gyt/commits/{hash}/snapshot"));
    let mut out = vec![];
    if !root.exists() {
        return out;
    }
    for e in WalkDir::new(&root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if e.path().is_file() {
            if let Ok(rel) = e.path().strip_prefix(&root) {
                let mut s = rel.to_string_lossy().to_string();
                if s.ends_with(".gz") {
                    s.truncate(s.len() - 3);
                }
                out.push(s);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn init_add_commit_then_noop_add_is_clean() {
    let dir = fresh_dir("init-commit");
    let server = dir.join("server");
    fs::create_dir_all(&server).unwrap();

    let o = init_repo(&dir, "repo1", server.to_str().unwrap());
    assert!(o.ok, "init failed: {}", o.text);
    // /tmp can be slow to settle in sandboxes; poll briefly for the file
    let mut seen = false;
    for _ in 0..50 {
        if fs::symlink_metadata(dir.join(".gyatignore")).is_ok() {
            seen = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(seen, ".gyatignore missing after init");
    let ign = read(&dir.join(".gyatignore"));
    assert!(ign.contains("target/"), "missing target/\n{ign}");
    assert!(ign.contains("node_modules/"), "missing node_modules/\n{ign}");

    fs::write(dir.join("a.txt"), "v1\n").unwrap();
    fs::write(dir.join("b.txt"), "v1\n").unwrap();
    // explicit add stages only what changed
    let o = run(&dir, &["add", "a.txt", "b.txt"], None);
    assert!(o.ok, "add failed: {}", o.text);
    assert!(o.text.contains("2 staged"), "unexpected: {}", o.text);

    let o = run(&dir, &["commit", "first"], None);
    assert!(o.ok, "commit failed: {}", o.text);
    assert!(o.text.contains("files changed") || o.text.contains("file changed"), "unexpected: {}", o.text);
    assert!(!head(&dir).is_empty());

    // untracked repo files (gyat.toml/.gyatignore) are picked up by add .
    let o = run(&dir, &["add", "."], None);
    assert!(o.ok, "add . failed: {}", o.text);
    let o = run(&dir, &["commit", "track meta"], None);
    assert!(o.ok, "commit failed: {}", o.text);

    // second add with no changes stages nothing (the reported bug scenario)
    let o = run(&dir, &["add", "."], None);
    assert!(o.ok, "second add failed: {}", o.text);
    assert!(
        o.text.contains("up to date"),
        "expected up-to-date, got: {}",
        o.text
    );

    let o = run(&dir, &["status"], None);
    assert!(o.ok, "status failed: {}", o.text);
    assert!(o.text.contains("working tree clean"), "unexpected: {}", o.text);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn modify_delete_untracked_flow() {
    let dir = fresh_dir("modify-delete");
    let server = dir.join("server");
    fs::create_dir_all(&server).unwrap();
    assert!(init_repo(&dir, "repo2", server.to_str().unwrap()).ok);

    fs::write(dir.join("a.txt"), "v1\n").unwrap();
    fs::write(dir.join("b.txt"), "v1\n").unwrap();
    assert!(run(&dir, &["add", "."], None).ok);
    assert!(run(&dir, &["commit", "-m", "base"], None).ok);

    // modify a, delete b, add untracked c
    fs::write(dir.join("a.txt"), "v2\n").unwrap();
    fs::remove_file(dir.join("b.txt")).unwrap();
    fs::write(dir.join("c.txt"), "new\n").unwrap();

    let o = run(&dir, &["status"], None);
    assert!(o.ok, "status failed: {}", o.text);
    assert!(o.text.contains("not staged"), "missing unstaged section: {}", o.text);
    assert!(o.text.contains("Untracked"), "missing untracked section: {}", o.text);

    let o = run(&dir, &["add", "."], None);
    assert!(o.ok, "add . failed: {}", o.text);
    assert!(o.text.contains("deleted"), "expected staged deletion: {}", o.text);

    let o = run(&dir, &["status"], None);
    assert!(o.text.contains("to be committed"), "missing staged section: {}", o.text);
    assert!(o.text.contains("deleted"), "missing deleted entry: {}", o.text);

    let o = run(&dir, &["commit", "second"], None);
    assert!(o.ok, "commit failed: {}", o.text);
    assert!(o.text.contains("delete mode: b.txt"), "unexpected: {}", o.text);

    let h = branch_hash(&dir, "main");
    assert!(!h.is_empty());
    let files = snapshot_files(&dir, &h);
    assert!(files.contains(&"a.txt".to_string()), "missing a.txt: {files:?}");
    assert!(files.contains(&"c.txt".to_string()), "missing c.txt: {files:?}");
    assert!(!files.contains(&"b.txt".to_string()), "b.txt should be gone: {files:?}");

    // unchanged single-file add is a no-op
    let o = run(&dir, &["add", "a.txt"], None);
    assert!(o.ok);
    assert!(o.text.contains("up to date"), "unexpected: {}", o.text);

    // empty commit errors cleanly
    let o = run(&dir, &["commit", "empty"], None);
    assert!(!o.ok);
    assert!(o.text.contains("nothing to commit"), "unexpected: {}", o.text);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn branch_travel_and_conflicting_merge() {
    let dir = fresh_dir("merge-conflict");
    let server = dir.join("server");
    fs::create_dir_all(&server).unwrap();
    assert!(init_repo(&dir, "repo3", server.to_str().unwrap()).ok);

    fs::write(dir.join("a.txt"), "base\n").unwrap();
    assert!(run(&dir, &["add", "a.txt"], None).ok);
    assert!(run(&dir, &["commit", "base"], None).ok);
    assert!(run(&dir, &["branch", "feature"], None).ok);

    fs::write(dir.join("a.txt"), "main line\n").unwrap();
    assert!(run(&dir, &["add", "a.txt"], None).ok);
    assert!(run(&dir, &["commit", "main change"], None).ok);

    let o = run(&dir, &["travel", "feature"], None);
    assert!(o.ok, "travel failed: {}", o.text);
    assert_eq!(read(&dir.join("a.txt")), "base\n");

    fs::write(dir.join("a.txt"), "feature line\n").unwrap();
    assert!(run(&dir, &["add", "a.txt"], None).ok);
    assert!(run(&dir, &["commit", "feature change"], None).ok);
    assert!(run(&dir, &["travel", "main"], None).ok);

    // conflicting merge
    let o = run(&dir, &["merge", "feature"], None);
    assert!(!o.ok, "merge should conflict");
    assert!(o.text.contains("conflict"), "unexpected: {}", o.text);
    assert!(dir.join(".gyt/MERGE_HEAD").exists(), "MERGE_HEAD missing");
    let ws = read(&dir.join("a.txt"));
    assert!(ws.contains("<<<<<<<"), "missing conflict markers:\n{ws}");

    // resolve and commit -> merge commit with second parent
    fs::write(dir.join("a.txt"), "resolved\n").unwrap();
    assert!(run(&dir, &["add", "a.txt"], None).ok);
    let o = run(&dir, &["commit", "merge resolve"], None);
    assert!(o.ok, "resolve commit failed: {}", o.text);
    assert!(!dir.join(".gyt/MERGE_HEAD").exists(), "MERGE_HEAD not cleared");
    let h = branch_hash(&dir, "main");
    let meta = read(&dir.join(format!(".gyt/commits/{h}/meta.toml")));
    assert!(meta.contains("second_parent"), "not a merge commit:\n{meta}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn clean_merge_auto_commits_and_fast_forward() {
    let dir = fresh_dir("merge-clean");
    let server = dir.join("server");
    fs::create_dir_all(&server).unwrap();
    assert!(init_repo(&dir, "repo4", server.to_str().unwrap()).ok);

    fs::write(dir.join("a.txt"), "base\n").unwrap();
    assert!(run(&dir, &["add", "a.txt"], None).ok);
    assert!(run(&dir, &["commit", "base"], None).ok);
    assert!(run(&dir, &["branch", "feature"], None).ok);

    // disjoint changes: main adds b, feature adds c
    fs::write(dir.join("b.txt"), "b\n").unwrap();
    assert!(run(&dir, &["add", "b.txt"], None).ok);
    assert!(run(&dir, &["commit", "main adds b"], None).ok);

    assert!(run(&dir, &["travel", "feature"], None).ok);
    fs::write(dir.join("c.txt"), "c\n").unwrap();
    assert!(run(&dir, &["add", "c.txt"], None).ok);
    assert!(run(&dir, &["commit", "feature adds c"], None).ok);
    assert!(run(&dir, &["travel", "main"], None).ok);

    let o = run(&dir, &["merge", "feature"], None);
    assert!(o.ok, "clean merge failed: {}", o.text);
    assert!(dir.join("b.txt").exists() && dir.join("c.txt").exists());

    // fast-forward: branch off, commit only there, merge back
    assert!(run(&dir, &["branch", "ff"], None).ok);
    assert!(run(&dir, &["travel", "ff"], None).ok);
    fs::write(dir.join("d.txt"), "d\n").unwrap();
    assert!(run(&dir, &["add", "d.txt"], None).ok);
    assert!(run(&dir, &["commit", "ff work"], None).ok);
    assert!(run(&dir, &["travel", "main"], None).ok);
    let o = run(&dir, &["merge", "ff"], None);
    assert!(o.ok, "ff merge failed: {}", o.text);
    assert!(o.text.contains("fast-forward"), "expected fast-forward: {}", o.text);
    assert!(dir.join("d.txt").exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snip_top_drops_head() {
    let dir = fresh_dir("snip");
    let server = dir.join("server");
    fs::create_dir_all(&server).unwrap();
    assert!(init_repo(&dir, "repo5", server.to_str().unwrap()).ok);

    fs::write(dir.join("a.txt"), "1\n").unwrap();
    assert!(run(&dir, &["add", "a.txt"], None).ok);
    assert!(run(&dir, &["commit", "c1"], None).ok);
    fs::write(dir.join("a.txt"), "2\n").unwrap();
    assert!(run(&dir, &["add", "a.txt"], None).ok);
    assert!(run(&dir, &["commit", "c2"], None).ok);

    let before = run(&dir, &["log"], None);
    assert!(before.text.contains("c1") && before.text.contains("c2"), "{}", before.text);
    assert!(run(&dir, &["snip", "top"], None).ok);
    let after = run(&dir, &["log"], None);
    assert!(after.text.contains("c1"), "{}", after.text);
    assert!(!after.text.contains("c2"), "c2 should be gone: {}", after.text);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn push_pull_clone_over_path_server() {
    let root = fresh_dir("remote");
    let server = root.join("server");
    fs::create_dir_all(&server).unwrap();

    // repo1 pushes
    let repo1 = root.join("repo1");
    fs::create_dir_all(&repo1).unwrap();
    assert!(init_repo(&repo1, "myrepo", server.to_str().unwrap()).ok);
    fs::write(repo1.join("a.txt"), "a\n").unwrap();
    assert!(run(&repo1, &["add", "a.txt"], None).ok);
    assert!(run(&repo1, &["commit", "init"], None).ok);
    let o = run(&repo1, &["push"], None);
    assert!(o.ok, "push failed: {}", o.text);
    assert!(server.join("myrepo/commits").exists(), "server has no commits");

    // clone
    let o = run(&root, &["clone", server.join("myrepo").to_str().unwrap(), "myclone"], None);
    assert!(o.ok, "clone failed: {}", o.text);
    let clone = root.join("myclone");
    assert_eq!(read(&clone.join("a.txt")), "a\n");

    // commit in clone, push
    fs::write(clone.join("b.txt"), "b\n").unwrap();
    assert!(run(&clone, &["add", "b.txt"], None).ok);
    assert!(run(&clone, &["commit", "clone work"], None).ok);
    assert!(run(&clone, &["push"], None).ok);

    // pull in repo1, checkout, file appears
    let o = run(&repo1, &["pull"], None);
    assert!(o.ok, "pull failed: {}", o.text);
    assert!(run(&repo1, &["travel", "main"], None).ok);
    assert_eq!(read(&repo1.join("b.txt")), "b\n");

    let _ = fs::remove_dir_all(&root);
}

// ---- ssh-transport protocol tests --------------------------------------
// The client shells out to `$GYAT_SSH_BIN` exactly like git shells out to
// ssh. These tests point that at a shim which execs the real `gyat-server`
// locally, exercising the full push/fetch/receive protocol minus TCP.

fn target_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CARGO_BUILD_TARGET_DIR") {
        return PathBuf::from(d);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug")
}

/// Server binary lives in another workspace package, so CARGO_BIN_EXE_* is
/// not set for it here. Locate it under target/debug, building on demand.
fn server_bin() -> PathBuf {
    let exe = if cfg!(windows) { "gyat-server.exe" } else { "gyat-server" };
    let path = target_dir().join(exe);
    if !path.exists() {
        let st = Command::new("cargo")
            .args(["build", "-p", "gyat-server"])
            .env("CARGO_TERM_COLOR", "never")
            .status()
            .expect("cargo build -p gyat-server");
        assert!(st.success(), "could not build gyat-server");
    }
    assert!(path.exists(), "gyat-server binary missing at {}", path.display());
    path
}

fn write_ssh_shim(dir: &Path) -> PathBuf {
    let shim = dir.join("fake-ssh.sh");
    fs::write(
        &shim,
        r#"#!/bin/sh
# fake ssh for tests: skip ssh flags + dest, run the rest via gyat-server
while [ $# -gt 0 ]; do
  case "$1" in
    -i|-p|-o) shift; shift;;
    -*) shift;;
    *) shift; break;;
  esac
done
# remaining $1 is the remote command string; re-split it like a remote shell
eval "set -- $1"
shift
exec "$GYAT_FAKE_SERVER_BIN" "$@"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim, perms).unwrap();
    }
    shim
}

fn run_ssh(dir: &Path, shim: &Path, server: &Path, args: &[&str], stdin: Option<&str>) -> Out {
    let mut cmd = Command::new(bin());
    cmd.current_dir(dir).args(args);
    cmd.env("GYAT_SSH_BIN", shim);
    cmd.env("GYAT_FAKE_SERVER_BIN", server_bin());
    let _ = server;
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn gyat");
    if let Some(input) = stdin {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Out {
        ok: out.status.success(),
        text,
    }
}

#[test]
fn ssh_push_pull_clone_roundtrip() {
    let root = fresh_dir("ssh-remote");
    let shim = write_ssh_shim(&root);
    let srv = root.join("srv");
    fs::create_dir_all(&srv).unwrap();
    let remote = format!("loki@fakehost:{}/myrepo", srv.display());

    let repo1 = root.join("repo1");
    fs::create_dir_all(&repo1).unwrap();
    assert!(init_repo(&repo1, "myrepo", &remote).ok);
    fs::write(repo1.join("a.txt"), "a\n").unwrap();
    assert!(run_ssh(&repo1, &shim, &srv, &["add", "a.txt"], None).ok);
    assert!(run_ssh(&repo1, &shim, &srv, &["commit", "init"], None).ok);

    let o = run_ssh(&repo1, &shim, &srv, &["push"], None);
    assert!(o.ok, "ssh push failed: {}", o.text);
    assert!(o.text.contains("ok: received"), "unexpected: {}", o.text);
    assert!(srv.join("myrepo/commits").exists(), "server has no commits");

    // clone over ssh
    let o = run_ssh(&root, &shim, &srv, &["clone", &remote, "myclone"], None);
    assert!(o.ok, "ssh clone failed: {}", o.text);
    let clone = root.join("myclone");
    assert_eq!(read(&clone.join("a.txt")), "a\n");

    // commit in clone, push, pull in repo1
    fs::write(clone.join("b.txt"), "b\n").unwrap();
    assert!(run_ssh(&clone, &shim, &srv, &["add", "b.txt"], None).ok);
    assert!(run_ssh(&clone, &shim, &srv, &["commit", "clone work"], None).ok);
    assert!(run_ssh(&clone, &shim, &srv, &["push"], None).ok);

    let o = run_ssh(&repo1, &shim, &srv, &["pull"], None);
    assert!(o.ok, "ssh pull failed: {}", o.text);
    assert!(run_ssh(&repo1, &shim, &srv, &["travel", "main"], None).ok);
    assert_eq!(read(&repo1.join("b.txt")), "b\n");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ssh_push_rejects_non_fast_forward() {
    let root = fresh_dir("ssh-ff");
    let shim = write_ssh_shim(&root);
    let srv = root.join("srv");
    fs::create_dir_all(&srv).unwrap();
    let remote = format!("fakehost:{}/myrepo", srv.display());

    // A pushes base
    let a = root.join("a");
    fs::create_dir_all(&a).unwrap();
    assert!(init_repo(&a, "myrepo", &remote).ok);
    fs::write(a.join("f.txt"), "base\n").unwrap();
    assert!(run_ssh(&a, &shim, &srv, &["add", "f.txt"], None).ok);
    assert!(run_ssh(&a, &shim, &srv, &["commit", "base"], None).ok);
    assert!(run_ssh(&a, &shim, &srv, &["push"], None).ok);

    // B clones at base
    assert!(run_ssh(&root, &shim, &srv, &["clone", &remote, "b"], None).ok);
    let b = root.join("b");

    // A advances + pushes (fast-forward, ok)
    fs::write(a.join("f.txt"), "A2\n").unwrap();
    assert!(run_ssh(&a, &shim, &srv, &["add", "f.txt"], None).ok);
    assert!(run_ssh(&a, &shim, &srv, &["commit", "A2"], None).ok);
    assert!(run_ssh(&a, &shim, &srv, &["push"], None).ok);

    // B diverges from base and pushes -> must be rejected
    fs::write(b.join("f.txt"), "B2\n").unwrap();
    assert!(run_ssh(&b, &shim, &srv, &["add", "f.txt"], None).ok);
    assert!(run_ssh(&b, &shim, &srv, &["commit", "B2"], None).ok);
    let o = run_ssh(&b, &shim, &srv, &["push"], None);
    assert!(!o.ok, "divergent push should be rejected: {}", o.text);
    assert!(
        o.text.contains("non-fast-forward") || o.text.contains("rejected"),
        "unexpected: {}",
        o.text
    );

    // force push overrides
    let o = run_ssh(&b, &shim, &srv, &["push", "--force"], None);
    assert!(o.ok, "force push failed: {}", o.text);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn server_list_shows_branches() {
    let root = fresh_dir("ssh-list");
    let shim = write_ssh_shim(&root);
    let srv = root.join("srv");
    fs::create_dir_all(&srv).unwrap();
    let remote = format!("fakehost:{}/myrepo", srv.display());

    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    assert!(init_repo(&repo, "myrepo", &remote).ok);
    fs::write(repo.join("a.txt"), "a\n").unwrap();
    assert!(run_ssh(&repo, &shim, &srv, &["add", "a.txt"], None).ok);
    assert!(run_ssh(&repo, &shim, &srv, &["commit", "init"], None).ok);
    assert!(run_ssh(&repo, &shim, &srv, &["push"], None).ok);

    // list via the server binary directly (same command pull/clone rely on)
    let out = Command::new(server_bin())
        .arg("list")
        .arg(srv.join("myrepo"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(text.contains("main"), "unexpected list output: {text}");

    let _ = fs::remove_dir_all(&root);
}

//! Push bundle format: a gzipped archive of a repo's `commits/` + `refs/heads/`
//! trees, streamed over stdin/stdout (or a file) between `gyat` and
//! `gyat-server receive` / `gyat-server fetch`.
//!
//! Layout (all integers little-endian):
//! ```text
//! magic      "GYATB1\n" (7 bytes)
//! u64        entry count
//! per entry: u64 path len, path bytes (utf-8, `/`-separated, relative),
//!            u64 content len, content bytes
//! ```
//! Entries are sorted by path for determinism. Paths must be relative and
//! must not escape (`..`, absolute) — unpack rejects them.

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

pub const MAGIC: &[u8] = b"GYATB1\n";

fn read_u64(r: &mut impl Read) -> Result<u64, String> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf).map_err(|e| format!("bundle: truncated ({e})"))?;
    Ok(u64::from_le_bytes(buf))
}

fn check_magic(r: &mut impl Read) -> Result<(), String> {
    let mut magic = [0u8; 7];
    r.read_exact(&mut magic)
        .map_err(|_| "bundle: bad magic (not a gyat bundle)".to_string())?;
    if magic != MAGIC {
        return Err("bundle: bad magic (not a gyat bundle)".to_string());
    }
    Ok(())
}

fn sane_rel(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Err(format!("bundle: absolute path rejected: {path}"));
    }
    for c in p.components() {
        match c {
            Component::Normal(_) => {}
            _ => return Err(format!("bundle: escaping path rejected: {path}")),
        }
    }
    Ok(p.to_path_buf())
}

/// Pack every file under `src_dir` (recursively) into `writer`.
/// Returns the number of entries packed.
pub fn pack_dir(src_dir: &Path, writer: &mut impl Write) -> Result<usize, String> {
    if !src_dir.exists() {
        return Err(format!("bundle pack: {} not found", src_dir.display()));
    }
    let mut files: Vec<PathBuf> = vec![];
    for entry in walkdir::WalkDir::new(src_dir).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            files.push(p.to_path_buf());
        }
    }
    files.sort();

    let mut enc = GzEncoder::new(writer, Compression::new(6));
    enc.write_all(MAGIC).map_err(|e| format!("bundle pack: {e}"))?;
    enc.write_all(&(files.len() as u64).to_le_bytes())
        .map_err(|e| format!("bundle pack: {e}"))?;
    for path in &files {
        let rel = path
            .strip_prefix(src_dir)
            .map_err(|e| format!("bundle pack: {e}"))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let data = fs::read(path).map_err(|e| format!("bundle pack {}: {e}", path.display()))?;
        enc.write_all(&(rel_str.len() as u64).to_le_bytes())
            .map_err(|e| format!("bundle pack: {e}"))?;
        enc.write_all(rel_str.as_bytes())
            .map_err(|e| format!("bundle pack: {e}"))?;
        enc.write_all(&(data.len() as u64).to_le_bytes())
            .map_err(|e| format!("bundle pack: {e}"))?;
        enc.write_all(&data).map_err(|e| format!("bundle pack: {e}"))?;
    }
    enc.finish().map_err(|e| format!("bundle pack finish: {e}"))?;
    Ok(files.len())
}

fn read_exact_vec(r: &mut impl Read, len: u64) -> Result<Vec<u8>, String> {
    if len > 256 * 1024 * 1024 {
        return Err("bundle: entry too large (>256MB)".to_string());
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)
        .map_err(|e| format!("bundle: truncated entry ({e})"))?;
    Ok(buf)
}

/// Unpack a bundle from `reader` into `dst_dir` (created if missing).
/// Returns the number of entries unpacked.
pub fn unpack_to(reader: &mut impl Read, dst_dir: &Path) -> Result<usize, String> {
    let mut dec = GzDecoder::new(reader);
    // decompress fully first so a corrupt stream fails before writing anything
    let mut raw = Vec::new();
    dec.read_to_end(&mut raw)
        .map_err(|e| format!("bundle: gunzip failed ({e})"))?;
    let mut cur = std::io::Cursor::new(raw);
    check_magic(&mut cur)?;
    let count = read_u64(&mut cur)?;
    if count > 200_000 {
        return Err("bundle: too many entries".to_string());
    }
    let mut staged: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let path_len = read_u64(&mut cur)?;
        if path_len > 4096 {
            return Err("bundle: path too long".to_string());
        }
        let path_bytes = read_exact_vec(&mut cur, path_len)?;
        let path_str = String::from_utf8(path_bytes)
            .map_err(|_| "bundle: path not utf-8".to_string())?;
        let rel = sane_rel(&path_str)?;
        let content_len = read_u64(&mut cur)?;
        let data = read_exact_vec(&mut cur, content_len)?;
        staged.push((rel, data));
    }
    // trailing garbage means corruption
    let mut tail = [0u8; 1];
    if cur.read(&mut tail).map_err(|e| format!("bundle: {e}"))? != 0 {
        return Err("bundle: trailing garbage".to_string());
    }

    fs::create_dir_all(dst_dir).map_err(|e| format!("bundle unpack mkdir: {e}"))?;
    for (rel, data) in &staged {
        let target = dst_dir.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("bundle unpack mkdir: {e}"))?;
        }
        fs::write(&target, data).map_err(|e| format!("bundle unpack write: {e}"))?;
    }
    Ok(staged.len())
}

/// Convenience: pack `src_dir` into a file.
pub fn pack_to_file(src_dir: &Path, bundle_path: &Path) -> Result<usize, String> {
    let file = fs::File::create(bundle_path)
        .map_err(|e| format!("bundle create {}: {e}", bundle_path.display()))?;
    let mut file = file;
    pack_dir(src_dir, &mut file)
}

/// Convenience: unpack a bundle file into `dst_dir`.
pub fn unpack_file(bundle_path: &Path, dst_dir: &Path) -> Result<usize, String> {
    let file =
        fs::File::open(bundle_path).map_err(|e| format!("bundle open {}: {e}", bundle_path.display()))?;
    let mut file = file;
    unpack_to(&mut file, dst_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::env::temp_dir().join(format!("gyat-bundle-test-{}-{id}-{name}", std::process::id()))
    }

    fn write_tree(root: &Path) {
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("top.txt"), "top\n").unwrap();
        fs::write(root.join("a/b/deep.txt"), "deep\n").unwrap();
        fs::write(root.join("bin.dat"), (0u8..=255).collect::<Vec<u8>>()).unwrap();
    }

    fn tree_map(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut m = std::collections::BTreeMap::new();
        for e in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            if e.path().is_file() {
                let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().to_string();
                m.insert(rel, fs::read(e.path()).unwrap());
            }
        }
        m
    }

    #[test]
    fn roundtrip_dir() {
        let src = tmp("src");
        let dst = tmp("dst");
        let bundle = tmp("b.gyatbundle");
        write_tree(&src);
        let n = pack_to_file(&src, &bundle).unwrap();
        assert_eq!(n, 3);
        // gzip magic, not plaintext
        assert_eq!(&fs::read(&bundle).unwrap()[..2], &[0x1f, 0x8b]);
        let m = unpack_file(&bundle, &dst).unwrap();
        assert_eq!(m, 3);
        assert_eq!(tree_map(&src), tree_map(&dst));
        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
        let _ = fs::remove_file(&bundle);
    }

    #[test]
    fn roundtrip_in_memory_sorted_deterministic() {
        let src = tmp("src2");
        write_tree(&src);
        let mut b1 = Vec::new();
        let mut b2 = Vec::new();
        pack_dir(&src, &mut b1).unwrap();
        pack_dir(&src, &mut b2).unwrap();
        assert_eq!(b1, b2);
        let _ = fs::remove_dir_all(&src);
    }

    #[test]
    fn rejects_bad_magic() {
        let dst = tmp("dst3");
        let mut bad: &[u8] = b"definitely not a bundle";
        assert!(unpack_to(&mut bad, &dst).is_err());
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn rejects_escaping_paths() {
        // craft a bundle with ../evil by hand (uncompressed inner + gzip it)
        let mut inner = Vec::new();
        inner.extend_from_slice(MAGIC);
        inner.extend_from_slice(&1u64.to_le_bytes());
        let evil = "../evil.txt";
        inner.extend_from_slice(&(evil.len() as u64).to_le_bytes());
        inner.extend_from_slice(evil.as_bytes());
        inner.extend_from_slice(&4u64.to_le_bytes());
        inner.extend_from_slice(b"evil");
        let mut enc = GzEncoder::new(Vec::new(), Compression::new(6));
        enc.write_all(&inner).unwrap();
        let bytes = enc.finish().unwrap();
        let dst = tmp("dst4");
        let mut cur: &[u8] = &bytes;
        assert!(unpack_to(&mut cur, &dst).is_err());
        assert!(!dst.join("evil.txt").exists());
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn rejects_absolute_paths() {
        let mut inner = Vec::new();
        inner.extend_from_slice(MAGIC);
        inner.extend_from_slice(&1u64.to_le_bytes());
        let evil = "/tmp/evil.txt";
        inner.extend_from_slice(&(evil.len() as u64).to_le_bytes());
        inner.extend_from_slice(evil.as_bytes());
        inner.extend_from_slice(&4u64.to_le_bytes());
        inner.extend_from_slice(b"evil");
        let mut enc = GzEncoder::new(Vec::new(), Compression::new(6));
        enc.write_all(&inner).unwrap();
        let bytes = enc.finish().unwrap();
        let dst = tmp("dst5");
        let mut cur: &[u8] = &bytes;
        assert!(unpack_to(&mut cur, &dst).is_err());
        let _ = fs::remove_dir_all(&dst);
    }
}

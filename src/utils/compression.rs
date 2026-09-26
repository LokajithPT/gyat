use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

pub fn compress_file(src: &Path, dst: &Path, level: u32) -> Result<(), String> {
    let data = fs::read(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    if let Some(p) = dst.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir {p:?}: {e}"))?; }
    let file = fs::File::create(dst).map_err(|e| format!("create {}: {e}", dst.display()))?;
    let mut enc = GzEncoder::new(file, Compression::new(level));
    enc.write_all(&data).map_err(|e| format!("compress: {e}"))?;
    enc.finish().map_err(|e| format!("finish: {e}"))?;
    Ok(())
}

pub fn decompress_file(src: &Path, dst: &Path) -> Result<(), String> {
    let file = fs::File::open(src).map_err(|e| format!("open {}: {e}", src.display()))?;
    let mut decoder = GzDecoder::new(file);
    let mut data = Vec::new();
    decoder.read_to_end(&mut data).map_err(|e| format!("decompress {}: {e}", src.display()))?;
    if let Some(p) = dst.parent() { fs::create_dir_all(p).map_err(|e| format!("mkdir {p:?}: {e}"))?; }
    fs::write(dst, data).map_err(|e| format!("write {}: {e}", dst.display()))?;
    Ok(())
}

pub fn is_compressed(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("gz")
}

pub fn read_text_maybe_compressed(path: &Path) -> Result<String, String> {
    if path.exists() {
        if is_compressed(path) {
            let file = fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
            let mut dec = GzDecoder::new(file);
            let mut s = String::new();
            dec.read_to_string(&mut s).map_err(|e| format!("decompress text {}: {e}", path.display()))?;
            Ok(s)
        } else {
            fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))
        }
    } else {
        let gz_path_str = format!("{}.gz", path.display());
        let gz_path = Path::new(&gz_path_str);
        if gz_path.exists() {
            return read_text_maybe_compressed(gz_path);
        }
        Err(format!("not found {}", path.display()))
    }
}

pub fn read_bytes_maybe_compressed(path: &Path) -> Result<Vec<u8>, String> {
    if path.exists() {
        if is_compressed(path) {
            let file = fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
            let mut dec = GzDecoder::new(file);
            let mut data = Vec::new();
            dec.read_to_end(&mut data).map_err(|e| format!("decompress {}: {e}", path.display()))?;
            Ok(data)
        } else {
            fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))
        }
    } else {
        let gz_path = std::path::PathBuf::from(format!("{}.gz", path.display()));
        if gz_path.exists() {
            return read_bytes_maybe_compressed(&gz_path);
        }
        Err(format!("not found {}", path.display()))
    }
}

pub fn should_compress(settings: &crate::utils::settings::GyatSettings) -> bool {
    settings.compression.enabled && settings.compression.algorithm == "gzip"
}

pub fn level(settings: &crate::utils::settings::GyatSettings) -> u32 {
    settings.compression.level.min(9)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static N: AtomicU64 = AtomicU64::new(0);

    fn tmp_file(name: &str) -> std::path::PathBuf {
        let id = N.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "gyat-compression-test-{}-{}-{name}",
            std::process::id(),
            id
        ))
    }

    #[test]
    fn gzip_roundtrip() {
        let src = tmp_file("src.txt");
        let dst = tmp_file("dst.txt.gz");
        let out = tmp_file("out.txt");
        let content = "hello gyat\nline with :: colons\nlast\n";
        fs::write(&src, content).unwrap();
        compress_file(&src, &dst, 6).unwrap();
        assert!(dst.exists());
        // compressed bytes differ from input
        assert_ne!(fs::read(&src).unwrap(), fs::read(&dst).unwrap());
        decompress_file(&dst, &out).unwrap();
        assert_eq!(fs::read_to_string(&out).unwrap(), content);
        // byte reader handles both plain and gz
        assert_eq!(
            String::from_utf8(read_bytes_maybe_compressed(&dst).unwrap()).unwrap(),
            content
        );
        assert_eq!(
            String::from_utf8(read_bytes_maybe_compressed(&src).unwrap()).unwrap(),
            content
        );
        let _ = fs::remove_file(src);
        let _ = fs::remove_file(dst);
        let _ = fs::remove_file(out);
    }

    #[test]
    fn gzip_roundtrip_binary() {
        let src = tmp_file("bin");
        let dst = tmp_file("bin.gz");
        let bytes: Vec<u8> = (0u8..=255).collect();
        fs::write(&src, &bytes).unwrap();
        compress_file(&src, &dst, 6).unwrap();
        assert_eq!(read_bytes_maybe_compressed(&dst).unwrap(), bytes);
        let _ = fs::remove_file(src);
        let _ = fs::remove_file(dst);
    }
}

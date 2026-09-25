use super::commit;

pub fn log() -> Result<(), String> {
    super::repo::ensure_repo()?;
    let metas = commit::list_metas();
    if metas.is_empty() {
        println!("no commits yet");
        return Ok(());
    }
    for m in metas.iter().rev() {
        let head_marker = if super::repo::read_head().as_deref() == Some(&m.hash) { " <- HEAD" } else { "" };
        println!("commit {}{}", m.hash, head_marker);
        println!("  author: {}  ts: {}", m.author, m.timestamp);
        println!("  message: {}", m.message);
        println!("  files: {}  parent: {}", m.files.len(), m.parent.as_deref().unwrap_or("-"));
        println!();
    }
    Ok(())
}

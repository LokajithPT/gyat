use super::commit;

fn fmt_time(ts: u64) -> String {
    // lightweight human time without extra deps: show raw + relative bucket
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(ts);
    let ago = now.saturating_sub(ts);
    let rel = if ago < 60 {
        format!("{ago}s ago")
    } else if ago < 3600 {
        format!("{}m ago", ago / 60)
    } else if ago < 86400 {
        format!("{}h ago", ago / 3600)
    } else {
        format!("{}d ago", ago / 86400)
    };
    format!("{ts} ({rel})")
}

pub fn log(oneline: bool) -> Result<(), String> {
    super::repo::ensure_repo()?;
    let metas = commit::list_metas();
    if metas.is_empty() {
        println!("No commits yet");
        println!("hint: `gyat add .` then `gyat commit \"init\"`");
        return Ok(());
    }
    if oneline {
        let head = super::repo::read_head().unwrap_or_default();
        for m in metas.iter().rev() {
            let short = &m.hash[..8.min(m.hash.len())];
            let mark = if m.hash == head { " <- HEAD" } else { "" };
            let merge = if m.second_parent.is_some() { " [merge]" } else { "" };
            println!("{short}{mark}{merge} {}", m.message);
        }
        return Ok(());
    }
    let branches = super::repo::list_branches();
    let head = super::repo::read_head().unwrap_or_default();
    let cur_branch = super::repo::current_branch().unwrap_or_default();
    for m in metas.iter().rev() {
        let short = &m.hash[..8.min(m.hash.len())];
        let mut decorations = vec![];
        if m.hash == head {
            decorations.push("HEAD".to_string());
        }
        for b in &branches {
            if super::repo::read_branch(b).as_deref() == Some(&m.hash) {
                if b == &cur_branch {
                    decorations.push(format!("{b}"));
                } else {
                    decorations.push(format!("{b}"));
                }
            }
        }
        let deco = if decorations.is_empty() { String::new() } else { format!(" ({})", decorations.join(", ")) };
        let merge_mark = if m.second_parent.is_some() { " [merge]" } else { "" };
        println!("commit {short}{deco}{merge_mark}");
        println!("Author: {}", m.author);
        println!("Date:   {}", fmt_time(m.timestamp));
        println!();
        println!("    {}", m.message);
        println!();
        let parent_short = m.parent.as_ref().map(|p| p[..8.min(p.len())].to_string()).unwrap_or_else(|| "-".to_string());
        let second = m.second_parent.as_ref().map(|s| format!(" + {}", &s[..8.min(s.len())])).unwrap_or_default();
        println!("  files: {}  parent: {parent_short}{second}", m.files.len());
        println!();
    }
    Ok(())
}

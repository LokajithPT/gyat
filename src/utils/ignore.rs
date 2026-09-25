use super::config;

pub fn ignore_show() -> Result<(), String> {
    super::repo::ensure_repo()?;
    let cfg = config::load().map_err(|e| format!("load config: {e}"))?;
    match cfg.ignore {
        Some(ig) if !ig.files.is_empty() => {
            println!("ignore patterns:");
            for p in ig.files { println!("  {p}"); }
        }
        _ => println!("no ignore patterns"),
    }
    Ok(())
}

pub fn ignore_add(pattern: &str) -> Result<(), String> {
    super::repo::ensure_repo()?;
    let mut cfg = config::load().map_err(|e| format!("load config: {e}"))?;
    let ign = cfg.ignore.get_or_insert_with(|| config::IgnoreSection { files: vec![] });
    if ign.files.contains(&pattern.to_string()) {
        println!("already ignored: {pattern}");
        return Ok(());
    }
    ign.files.push(pattern.to_string());
    config::save(&cfg)?;
    println!("added ignore: {pattern}");
    Ok(())
}

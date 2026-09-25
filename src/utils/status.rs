use std::fs;
use std::path::Path;

pub fn status() {
    let config_path = Path::new(".gyt/config.toml");
    if !config_path.exists() {
        println!("not a gyat repository .gyt/config.toml missing");
        return;
    }

    let config_content = fs::read_to_string(config_path).unwrap_or_default();
    println!("Repo config:");
    println!("{}", config_content);

    let stages_path = Path::new(".gyt/stages");
    if stages_path.exists() {
        match fs::read_dir(stages_path) {
            Ok(entries) => {
                let count = entries.count();
                println!("Stages: {} item(s)", count);
            }
            Err(_) => println!("Stages: unreadable"),
        }
    } else {
        println!("Stages: not initialized");
    }

    let current_path = Path::new(".gyt/current");
    if current_path.exists() {
        println!("Current worktree present");
    } else {
        println!("Current worktree missing");
    }
}

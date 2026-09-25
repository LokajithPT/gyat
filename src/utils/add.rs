use std::fs;
use std::path::Path;

pub fn add(files: &[String]) {
    if files.is_empty() {
        println!("add: no files specified");
        return;
    }

    let stages_root = Path::new(".gyt/stages");
    if !stages_root.exists() {
        println!("not initialized: .gyt/stages missing");
        return;
    }

    for file in files {
        let src_path = Path::new(&file);
        if !src_path.exists() {
            println!("add: file not found {}", file);
            continue;
        }

        // simple staging: copy file into .gyt/stages with same name
        let dest_path = stages_root.join(&file);
        if let Some(parent) = dest_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        match fs::copy(&src_path, &dest_path) {
            Ok(_) => println!("staged {}", file),
            Err(e) => println!("failed to stage {}: {}", file, e),
        }
    }
}

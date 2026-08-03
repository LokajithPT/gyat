use std::fs;
use std::io::{self, Write};
use std::path::Path;

fn repoExists(repo: &str) -> bool {
    // let path = Path::new(".gyt");
    // path.is_dir()
    // here i have to check the server if the repo already exists
    false
}

fn alreadyInitialized(repo: &str) -> bool {
    let path = Path::new(".gyt");
    path.is_dir()
}

fn createRepo(repo: &str, username: &str, server: &str) {
    println!("creating repo: {}", repo);

    let config = format!(
        r#"
[repo]
name = "{}"
username = "{}"
server = "{}"
    "#,
        repo, username, server
    );

    //we will make the repo in here now
    fs::create_dir_all(".gyt/").unwrap();
    fs::create_dir_all(".gyt/stages").unwrap();
    fs::create_dir_all(".gyt/current").unwrap();
    fs::write(".gyt/config.toml", config).unwrap();
}

fn prompt(q: &str) -> String {
    print!("{}: ", q);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

pub fn init() {
    if alreadyInitialized(".gyt") {
        println!("already initialized");
        std::process::exit(0);
    }

    let repo = prompt("repo name");

    if repoExists(&repo) {
        println!("repo already exists");
    }
    let mut username = prompt("username");
    let mut server = prompt("server");

    if username.is_empty() {
        username = "loki".to_string();
        println!("username: loki");
    } else {
        println!("username: {}", username);
    }

    if server.is_empty() {
        server = "raspi.local".to_string();
        println!("server: wasnt given so im using the default");
    } else {
        println!("server: {}", server);
    }

    //lemme show you the summary
    println!(
        "\n summary: repo={} \n username={} \n server={}",
        repo, username, server
    );

    createRepo(&repo, &username, &server);

    std::process::exit(0);
}

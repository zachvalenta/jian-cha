use comfy_table::presets::ASCII_FULL;
use comfy_table::{Cell, Color, Table};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    directories: Vec<String>,
}

#[derive(Debug)]
struct GitInfo {
    branch: String,
    last_commit: String,
    clean: bool,
    has_unpushed: Option<bool>,
}

#[derive(Debug)]
struct RepoResult {
    directory: String,
    branch: Option<String>,
    last_commit: Option<String>,
    clean: Option<bool>,
    has_unpushed: Option<bool>,
    error: Option<String>,
}

fn is_git_repo(directory: &Path) -> bool {
    Command::new("git")
        .args(["-C", directory.to_str().unwrap(), "status"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn get_git_info(directory: &Path) -> Option<GitInfo> {
    let dir_str = directory.to_str()?;

    // Get branch
    let branch = Command::new("git")
        .args(["-C", dir_str, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())?;

    // Get last commit
    let last_commit = Command::new("git")
        .args(["-C", dir_str, "log", "-1", "--pretty=%s"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())?;

    // Check if clean
    let status_output = Command::new("git")
        .args(["-C", dir_str, "status", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())?;
    let clean = status_output.trim().is_empty();

    // Check for unpushed commits
    let has_unpushed = Command::new("git")
        .args(["-C", dir_str, "rev-list", "@{u}..HEAD", "--count"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|s| s.trim().parse::<i32>().ok())
        .map(|count| count > 0);

    Some(GitInfo {
        branch,
        last_commit,
        clean,
        has_unpushed,
    })
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let username = std::env::var("USER").unwrap_or_else(|_| String::from("unknown"));
    let config_path = if username == "zach" {
        "config-home.json"
    } else {
        "config-work.json"
    };

    let contents = std::fs::read_to_string(config_path)?;
    let config: Config = serde_json::from_str(&contents)?;
    Ok(config)
}

fn main() {
    let config = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            std::process::exit(1);
        }
    };

    let mut results = Vec::new();

    for directory in config.directories {
        let dir_path = PathBuf::from(&directory);
        let resolved_path = match dir_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                results.push(RepoResult {
                    directory: directory.clone(),
                    branch: None,
                    last_commit: None,
                    clean: None,
                    has_unpushed: None,
                    error: Some("Not a valid directory".to_string()),
                });
                continue;
            }
        };

        if !is_git_repo(&resolved_path) {
            results.push(RepoResult {
                directory: resolved_path.to_string_lossy().to_string(),
                branch: None,
                last_commit: None,
                clean: None,
                has_unpushed: None,
                error: Some("Not a Git repository".to_string()),
            });
            continue;
        }

        match get_git_info(&resolved_path) {
            Some(git_info) => {
                results.push(RepoResult {
                    directory: resolved_path.to_string_lossy().to_string(),
                    branch: Some(git_info.branch),
                    last_commit: Some(git_info.last_commit),
                    clean: Some(git_info.clean),
                    has_unpushed: git_info.has_unpushed,
                    error: None,
                });
            }
            None => {
                results.push(RepoResult {
                    directory: resolved_path.to_string_lossy().to_string(),
                    branch: None,
                    last_commit: None,
                    clean: None,
                    has_unpushed: None,
                    error: Some("Failed to retrieve Git info".to_string()),
                });
            }
        }
    }

    // Create table
    let mut table = Table::new();
    table
        .load_preset(ASCII_FULL)
        .set_header(vec![
            Cell::new("Directory").fg(Color::Cyan),
            Cell::new("Branch").fg(Color::Magenta),
            Cell::new("Status"),
            Cell::new("Last Commit").fg(Color::Yellow),
            Cell::new("Error").fg(Color::Red),
        ]);

    for result in results {
        let (status_symbol, status_color) = if let Some(_error) = &result.error {
            ("?", Color::Yellow)
        } else {
            let clean = result.clean.unwrap_or(false);
            let has_unpushed = result.has_unpushed;

            match (clean, has_unpushed) {
                (true, Some(false)) => ("✓", Color::Green),
                (true, Some(true)) => ("↑", Color::Yellow),
                (true, None) => ("⚠", Color::Yellow),
                _ => ("✗", Color::Red),
            }
        };

        table.add_row(vec![
            Cell::new(&result.directory),
            Cell::new(result.branch.unwrap_or_default()),
            Cell::new(status_symbol).fg(status_color),
            Cell::new(result.last_commit.unwrap_or_default()),
            Cell::new(result.error.unwrap_or_else(|| "-".to_string())),
        ]);
    }

    println!();
    println!("{}", table);
}

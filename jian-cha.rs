use comfy_table::presets::ASCII_FULL;
use comfy_table::{Cell, Color, ColumnConstraint, ContentArrangement, Table, Width};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
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

    // Group results by parent directory
    let mut grouped: HashMap<String, Vec<RepoResult>> = HashMap::new();
    for result in results {
        let parent = PathBuf::from(&result.directory)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        grouped.entry(parent).or_insert_with(Vec::new).push(result);
    }

    // Find common ancestors for nested paths and merge them
    let mut final_grouped: HashMap<String, Vec<RepoResult>> = HashMap::new();
    let parent_paths: Vec<String> = grouped.keys().cloned().collect();

    for (parent, repos) in grouped {
        // Check if this parent is nested under another parent in our list
        let mut common_ancestor = parent.clone();
        for other_parent in &parent_paths {
            if parent.starts_with(other_parent) && parent != *other_parent {
                common_ancestor = other_parent.clone();
                break;
            }
        }

        // Add to final grouped using the common ancestor
        final_grouped.entry(common_ancestor).or_insert_with(Vec::new).extend(repos);
    }

    // Sort parent directories for consistent output
    let mut parents: Vec<_> = final_grouped.keys().cloned().collect();
    parents.sort();

    // Create a table for each parent directory
    for parent in parents {
        let repos = final_grouped.get(&parent).unwrap();

        // Extract just the parent directory name and make it uppercase
        let parent_name = PathBuf::from(&parent)
            .file_name()
            .map(|n| n.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| "UNKNOWN".to_string());

        println!();
        // Print section header with bold, underline, and white color
        println!("\x1b[1;4;37m{}\x1b[0m", parent_name);

        let mut table = Table::new();
        table
            .load_preset(ASCII_FULL)
            .set_content_arrangement(ContentArrangement::Disabled)
            .set_header(vec![
                Cell::new("Repository").fg(Color::Cyan),
                Cell::new("Branch").fg(Color::Magenta),
                Cell::new("Status").fg(Color::Yellow),
                Cell::new("Last Commit").fg(Color::Yellow),
                Cell::new("Error").fg(Color::Red),
            ]);

        // Set fixed widths for each column
        table.column_mut(0).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(15)));  // Repository
        table.column_mut(1).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(18)));  // Branch
        table.column_mut(2).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(6)));   // Status
        table.column_mut(3).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(35)));  // Last Commit
        table.column_mut(4).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(8)));   // Error

        // Set minimal padding
        table.column_mut(0).unwrap().set_padding((0, 1));
        table.column_mut(1).unwrap().set_padding((0, 1));
        table.column_mut(2).unwrap().set_padding((0, 1));
        table.column_mut(3).unwrap().set_padding((0, 1));
        table.column_mut(4).unwrap().set_padding((0, 1));

        for result in repos {
            // Get relative path from parent directory
            let repo_path = PathBuf::from(&result.directory);
            let parent_path = PathBuf::from(&parent);
            let repo_name = repo_path
                .strip_prefix(&parent_path)
                .ok()
                .and_then(|p| Some(p.to_string_lossy().to_string()))
                .unwrap_or_else(|| {
                    repo_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| result.directory.clone())
                });

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

            let truncated_repo = truncate_string(&repo_name, 13);

            let branch = result.branch.clone().unwrap_or_default();
            let truncated_branch = truncate_string(&branch, 16);

            let last_commit = result.last_commit.clone().unwrap_or_default();
            let truncated_commit = truncate_string(&last_commit, 33);

            let error = result.error.clone().unwrap_or_else(|| "-".to_string());
            let truncated_error = truncate_string(&error, 6);

            table.add_row(vec![
                Cell::new(truncated_repo),
                Cell::new(truncated_branch),
                Cell::new(status_symbol).fg(status_color),
                Cell::new(truncated_commit),
                Cell::new(truncated_error),
            ]);
        }

        println!("{}", table);
    }

    println!();
}

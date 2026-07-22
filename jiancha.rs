use comfy_table::presets::ASCII_FULL;
use comfy_table::{Attribute, Cell, Color, ColumnConstraint, ContentArrangement, Table, Width};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    #[serde(flatten)]
    sections: IndexMap<String, IndexMap<String, String>>,
}

#[derive(Debug, Clone)]
enum FetchStatus {
    Pending,
    UpToDate,
    Behind(u32),
    Error,
}

#[derive(Debug)]
struct RepoRow {
    repo_key: String,
    directory: String,
    branch: Option<String>,
    last_commit: Option<String>,
    clean: Option<bool>,
    has_unpushed: Option<bool>,
    local_error: Option<String>,
    fetch_status: FetchStatus,
}

fn is_git_repo(dir: &Path) -> bool {
    Command::new("git")
        .args(["-C", dir.to_str().unwrap_or(""), "rev-parse", "--git-dir"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn git_cmd(dir: &str, args: &[&str]) -> Option<String> {
    let mut full_args = vec!["-C", dir];
    full_args.extend_from_slice(args);
    Command::new("git")
        .args(&full_args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}

struct LocalInfo {
    branch: String,
    last_commit: String,
    clean: bool,
    has_unpushed: Option<bool>,
}

fn get_local_info(dir: &str) -> Option<LocalInfo> {
    let branch = git_cmd(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let last_commit = git_cmd(dir, &["log", "-1", "--pretty=%s"])?;
    let status_out = git_cmd(dir, &["status", "--porcelain"])?;
    let clean = status_out.is_empty();
    let has_unpushed = git_cmd(dir, &["rev-list", "--count", "@{u}..HEAD"])
        .and_then(|s| s.parse::<u32>().ok())
        .map(|n| n > 0);
    Some(LocalInfo { branch, last_commit, clean, has_unpushed })
}

fn run_git_fetch(dir: &str) -> FetchStatus {
    let ok = Command::new("git")
        .args(["-C", dir, "fetch", "--quiet"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !ok {
        return FetchStatus::Error;
    }

    match git_cmd(dir, &["rev-list", "--count", "HEAD..@{u}"]) {
        Some(s) => match s.parse::<u32>() {
            Ok(0) => FetchStatus::UpToDate,
            Ok(n) => FetchStatus::Behind(n),
            Err(_) => FetchStatus::Error,
        },
        None => FetchStatus::UpToDate,
    }
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let config_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config")
    } else {
        return Err("HOME not set".into());
    };
    let path = config_dir.join("jiancha").join("config.toml");
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    Ok(toml::from_str(&contents)?)
}

fn terminal_width() -> Option<u16> {
    Command::new("sh")
        .args(["-c", "stty size < /dev/tty"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().nth(1).and_then(|w| w.parse::<u16>().ok()))
        .filter(|&w| w > 0)
        .or_else(|| {
            std::env::var("COLUMNS")
                .ok()
                .and_then(|s| s.parse::<u16>().ok())
                .filter(|&w| w > 0)
        })
        .or_else(|| Table::new().width())
}

const DEFAULT_TABLE_WIDTH: u16 = 101;
const DEFAULT_SECTION_RULE_WIDTH: u16 = 27;

fn section_rule(width: Option<u16>) -> String {
    let len = width
        .map(|w| w.min(DEFAULT_SECTION_RULE_WIDTH))
        .unwrap_or(DEFAULT_SECTION_RULE_WIDTH) as usize;
    "═".repeat(len)
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

fn render_all(repos: &[RepoRow], sections: &IndexMap<String, Vec<usize>>) -> String {
    let mut output = String::new();
    let viewport_width = terminal_width().map(|w| w.saturating_sub(2));
    let full_size = viewport_width.map_or(true, |w| w >= DEFAULT_TABLE_WIDTH);
    let compact = viewport_width.is_some_and(|w| w < 80);
    let narrow = viewport_width.is_some_and(|w| w < 60);
    let tiny = viewport_width.is_some_and(|w| w < 40);
    let ultra_tiny = viewport_width.is_some_and(|w| w < 28);
    let show_branch = full_size || !tiny;
    let show_last_commit = full_size || !narrow;
    let show_remote = full_size || !ultra_tiny;
    let show_error = full_size || !compact;
    let rule = section_rule(viewport_width);

    for section_name in sections.keys() {
        let repo_indices = &sections[section_name];

        output.push('\n');
        output.push_str(&format!("\x1b[1;38;2;255;140;0m{}\x1b[0m\n", rule));
        output.push_str(&format!("\x1b[1;38;2;255;140;0m    {}\x1b[0m\n", section_name.to_uppercase()));
        output.push_str(&format!("\x1b[1;38;2;255;140;0m{}\x1b[0m\n", rule));

        let mut table = Table::new();
        table.load_preset(ASCII_FULL);
        if full_size {
            table.set_content_arrangement(ContentArrangement::Disabled);
        } else if let Some(width) = viewport_width {
            table
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_width(width);
        }

        let mut header = vec![Cell::new(if full_size { "Repository" } else { "Repo" }).fg(Color::Cyan)];
        if show_branch {
            header.push(Cell::new(if full_size { "Branch" } else { "Br" }).fg(Color::Magenta));
        }
        header.push(Cell::new(if full_size { "Status" } else { "St" }).fg(Color::Rgb { r: 119, g: 136, b: 153 }));
        if show_last_commit {
            header.push(Cell::new(if full_size { "Last Commit" } else { "Last" }).fg(Color::Rgb { r: 184, g: 134, b: 11 }));
        }
        if show_remote {
            header.push(Cell::new(if full_size { "Remote" } else { "R" }).fg(Color::Rgb { r: 100, g: 200, b: 100 }));
        }
        if show_error {
            header.push(Cell::new(if full_size { "Error" } else { "Err" }).fg(Color::Red));
        }
        table.set_header(header);

        if full_size {
            table.column_mut(0).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(15))).set_padding((0, 1));
            table.column_mut(1).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(18))).set_padding((0, 1));
            table.column_mut(2).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(8))).set_padding((0, 1));
            table.column_mut(3).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(35))).set_padding((0, 1));
            table.column_mut(4).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(10))).set_padding((0, 1));
            table.column_mut(5).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(8))).set_padding((0, 1));
        } else {
            let mut col = 0;
            table.column_mut(col).unwrap().set_constraint(ColumnConstraint::UpperBoundary(Width::Percentage(if tiny { 60 } else { 25 }))).set_padding((0, 1));
            col += 1;
            if show_branch {
                table.column_mut(col).unwrap().set_constraint(ColumnConstraint::UpperBoundary(Width::Percentage(25))).set_padding((0, 1));
                col += 1;
            }
            table.column_mut(col).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(4))).set_padding((0, 1));
            col += 1;
            if show_last_commit {
                table.column_mut(col).unwrap().set_constraint(ColumnConstraint::UpperBoundary(Width::Percentage(30))).set_padding((0, 1));
                col += 1;
            }
            if show_remote {
                table.column_mut(col).unwrap().set_constraint(ColumnConstraint::Absolute(Width::Fixed(4))).set_padding((0, 1));
                col += 1;
            }
            if show_error {
                table.column_mut(col).unwrap().set_constraint(ColumnConstraint::UpperBoundary(Width::Percentage(14))).set_padding((0, 1));
            }
        }

        for &idx in repo_indices {
            let repo = &repos[idx];

            let (status_symbol, status_color) = if repo.local_error.is_some() {
                ("?", Color::Yellow)
            } else {
                match (repo.clean.unwrap_or(false), repo.has_unpushed) {
                    (true, Some(false)) => ("✓", Color::Green),
                    (true, Some(true)) => ("↑", Color::Yellow),
                    (true, None) => ("⚠", Color::Yellow),
                    _ => ("✗", Color::Red),
                }
            };

            let (remote_text, remote_color) = match &repo.fetch_status {
                FetchStatus::Pending  => ("...".to_string(),    Color::Grey),
                FetchStatus::UpToDate => ("✓".to_string(),      Color::Green),
                FetchStatus::Behind(n) => (format!("↓ {}", n), Color::Yellow),
                FetchStatus::Error    => ("err".to_string(),    Color::Red),
            };

            let branch     = repo.branch.as_deref().unwrap_or("");
            let last_commit = repo.last_commit.as_deref().unwrap_or("");
            let error      = repo.local_error.as_deref().unwrap_or("-");

            if full_size {
                table.add_row(vec![
                    Cell::new(truncate_string(&repo.repo_key, 13)),
                    Cell::new(truncate_string(branch, 16)),
                    Cell::new(status_symbol).fg(status_color).add_attribute(Attribute::Bold),
                    Cell::new(truncate_string(last_commit, 33)),
                    Cell::new(remote_text).fg(remote_color),
                    Cell::new(truncate_string(error, 6)),
                ]);
            } else {
                let mut row = vec![Cell::new(truncate_string(&repo.repo_key, 40))];
                if show_branch {
                    row.push(Cell::new(truncate_string(branch, 40)));
                }
                row.push(Cell::new(status_symbol).fg(status_color).add_attribute(Attribute::Bold));
                if show_last_commit {
                    row.push(Cell::new(truncate_string(last_commit, 80)));
                }
                if show_remote {
                    row.push(Cell::new(remote_text).fg(remote_color));
                }
                if show_error {
                    row.push(Cell::new(truncate_string(error, 40)));
                }
                table.add_row(row);
            }
        }

        output.push_str(&table.to_string());
        output.push('\n');
    }

    output.push('\n');
    output
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config()?;

    let mut repos: Vec<RepoRow> = Vec::new();
    let mut sections: IndexMap<String, Vec<usize>> = IndexMap::new();

    for (section_name, section) in &config.sections {
        for (repo_name, dir_str) in section {
            let idx = repos.len();
            sections.entry(section_name.clone()).or_default().push(idx);

            let dir_path = PathBuf::from(dir_str);
            let resolved = match dir_path.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    repos.push(RepoRow {
                        repo_key: repo_name.clone(),
                        directory: dir_str.clone(),
                        branch: None, last_commit: None, clean: None, has_unpushed: None,
                        local_error: Some("Not a valid directory".into()),
                        fetch_status: FetchStatus::Pending,
                    });
                    continue;
                }
            };

            if !is_git_repo(&resolved) {
                repos.push(RepoRow {
                    repo_key: repo_name.clone(),
                    directory: resolved.to_string_lossy().into_owned(),
                    branch: None, last_commit: None, clean: None, has_unpushed: None,
                    local_error: Some("Not a Git repository".into()),
                    fetch_status: FetchStatus::Pending,
                });
                continue;
            }

            let dir_s = resolved.to_string_lossy().into_owned();
            let (branch, last_commit, clean, has_unpushed, local_error) =
                match get_local_info(&dir_s) {
                    Some(info) => (Some(info.branch), Some(info.last_commit), Some(info.clean), info.has_unpushed, None),
                    None => (None, None, None, None, Some("Failed to get git info".into())),
                };

            repos.push(RepoRow {
                repo_key: repo_name.clone(),
                directory: dir_s,
                branch, last_commit, clean, has_unpushed, local_error,
                fetch_status: FetchStatus::Pending,
            });
        }
    }

    // Fetch all remotes in parallel, then join before rendering
    let handles: Vec<(String, thread::JoinHandle<FetchStatus>)> = repos
        .iter()
        .filter(|r| r.local_error.is_none())
        .map(|repo| {
            let dir = repo.directory.clone();
            let key = repo.repo_key.clone();
            let handle = thread::spawn(move || run_git_fetch(&dir));
            (key, handle)
        })
        .collect();

    for (key, handle) in handles {
        let status = handle.join().unwrap_or(FetchStatus::Error);
        if let Some(idx) = repos.iter().position(|r| r.repo_key == key) {
            repos[idx].fetch_status = status;
        }
    }

    print!("{}", render_all(&repos, &sections));
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
    }
}

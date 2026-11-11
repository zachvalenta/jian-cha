use comfy_table::presets::ASCII_FULL;
use comfy_table::{Attribute, Cell, Color, ColumnConstraint, ContentArrangement, Table, Width};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use indexmap::IndexMap;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    group: Vec<GroupConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LegacyConfig {
    #[serde(flatten)]
    sections: IndexMap<String, IndexMap<String, LegacyRepoConfig>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum LegacyRepoConfig {
    Path(String),
    Sync { path: String, sync: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupConfig {
    name: String,
    #[serde(default)]
    repo: Vec<RepoConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepoConfig {
    name: String,
    fs: String,
    working_branch: Option<String>,
    /// Branch that `jiancha --sync` should reconcile with its remote.
    sync: Option<String>,
    remote: Option<String>,
    allow_reset: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FetchStatus {
    Pending,
    UpToDate,
    Behind(u32),
    Error,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    last_run_at: Option<u64>,
    repos: HashMap<String, CacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    repo_key: String,
    branch: String,
    upstream: Option<String>,
    fetched_at: u64,
    fetch_status: FetchStatus,
}

#[derive(Debug)]
struct Args {
    fresh: bool,
    offline: bool,
    output: OutputMode,
    command: CommandMode,
}

#[derive(Debug, PartialEq, Eq)]
enum OutputMode {
    Full,
    Compact,
    Summary,
}

#[derive(Debug)]
enum CommandMode {
    Check,
    Tui,
    Sync(SyncArgs),
    AutoSync,
}

#[derive(Debug, Default)]
struct SyncArgs {
    group: Option<String>,
    pull_requested: bool,
    reset: bool,
}

const REMOTE_TTL_SECS: u64 = 30 * 60;
const ERROR_RETRY_TTL_SECS: u64 = 2 * 60;
const SESSION_GAP_REFRESH_SECS: u64 = 90 * 60;

#[derive(Debug, Clone)]
struct RepoRow {
    repo_key: String,
    group: String,
    directory: String,
    working_branch: Option<String>,
    sync_branch: Option<String>,
    remote: Option<String>,
    allow_reset: bool,
    branch: Option<String>,
    last_commit: Option<String>,
    clean: Option<bool>,
    has_unpushed: Option<bool>,
    upstream: Option<String>,
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

fn git_status(dir: &str, args: &[&str]) -> Result<String, String> {
    let mut full_args = vec!["-C", dir];
    full_args.extend_from_slice(args);
    let out = Command::new("git")
        .args(&full_args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        })
    }
}

struct LocalInfo {
    branch: String,
    last_commit: String,
    clean: bool,
    has_unpushed: Option<bool>,
    upstream: Option<String>,
}

fn get_local_info(dir: &str) -> Option<LocalInfo> {
    let branch = git_cmd(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let last_commit = git_cmd(dir, &["log", "-1", "--pretty=%s"])?;
    let status_out = git_cmd(dir, &["status", "--porcelain"])?;
    let clean = status_out.is_empty();
    let has_unpushed = git_cmd(dir, &["rev-list", "--count", "@{u}..HEAD"])
        .and_then(|s| s.parse::<u32>().ok())
        .map(|n| n > 0);
    let upstream = git_cmd(
        dir,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    Some(LocalInfo {
        branch,
        last_commit,
        clean,
        has_unpushed,
        upstream,
    })
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
    let config_path = config_dir.join("jiancha");
    let yaml_path = config_path.join("config.yaml");
    match std::fs::read_to_string(&yaml_path) {
        Ok(contents) => parse_yaml_config(&contents),
        Err(yaml_error) if yaml_error.kind() == io::ErrorKind::NotFound => {
            let toml_path = config_path.join("config.toml");
            let contents = std::fs::read_to_string(&toml_path).map_err(|toml_error| {
                format!(
                    "Failed to read {} ({}) or {} ({})",
                    yaml_path.display(),
                    yaml_error,
                    toml_path.display(),
                    toml_error
                )
            })?;
            parse_toml_config(&contents)
        }
        Err(e) => Err(format!("Failed to read {}: {}", yaml_path.display(), e).into()),
    }
}

fn legacy_to_config(legacy: LegacyConfig) -> Config {
    let group = legacy
        .sections
        .into_iter()
        .map(|(group_name, repos)| GroupConfig {
            name: group_name,
            repo: repos
                .into_iter()
                .map(|(repo_name, repo)| {
                    let (fs, sync) = match repo {
                        LegacyRepoConfig::Path(path) => (path, None),
                        LegacyRepoConfig::Sync { path, sync } => (path, sync),
                    };
                    RepoConfig {
                        name: repo_name,
                        fs,
                        working_branch: None,
                        sync,
                        remote: None,
                        allow_reset: None,
                    }
                })
                .collect(),
        })
        .collect();
    Config { group }
}

fn parse_yaml_config(contents: &str) -> Result<Config, Box<dyn std::error::Error>> {
    if let Ok(config) = serde_yaml::from_str::<Config>(contents) {
        return Ok(config);
    }
    let legacy: LegacyConfig = serde_yaml::from_str(contents)?;
    Ok(legacy_to_config(legacy))
}

fn parse_toml_config(contents: &str) -> Result<Config, Box<dyn std::error::Error>> {
    if let Ok(config) = toml::from_str::<Config>(contents) {
        return Ok(config);
    }
    let legacy: LegacyConfig = toml::from_str(contents)?;
    Ok(legacy_to_config(legacy))
}

fn parse_args() -> Result<Args, Box<dyn std::error::Error>> {
    let mut args = Args {
        fresh: false,
        offline: false,
        output: OutputMode::Full,
        command: CommandMode::Check,
    };
    let mut iter = std::env::args().skip(1).peekable();

    match iter.peek().map(|s| s.as_str()) {
        Some("tui") => {
            iter.next();
            args.command = CommandMode::Tui;
            return Ok(args);
        }
        Some("sync") => {
            iter.next();
            let mut sync = SyncArgs::default();
            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "--group" => sync.group = iter.next(),
                    "--pull" => {
                        if sync.reset {
                            return Err("--pull and --reset cannot be used together".into());
                        }
                        sync.pull_requested = true;
                    }
                    "--reset" => {
                        if sync.pull_requested {
                            return Err("--pull and --reset cannot be used together".into());
                        }
                        sync.reset = true;
                    }
                    "-h" | "--help" => {
                        println!("Usage: jiancha sync [--group NAME] [--pull|--reset]");
                        std::process::exit(0);
                    }
                    _ => return Err(format!("Unknown sync argument: {arg}").into()),
                }
            }
            args.command = CommandMode::Sync(sync);
            return Ok(args);
        }
        _ => {}
    }

    for arg in iter {
        match arg.as_str() {
            "--fresh" | "--refresh" => args.fresh = true,
            "--offline" => args.offline = true,
            "--sync" => args.command = CommandMode::AutoSync,
            "--compact" => args.output = OutputMode::Compact,
            "--summary" => args.output = OutputMode::Summary,
            "-h" | "--help" => {
                println!(
                    "Usage: jiancha [--compact|--summary] [--fresh|--refresh] [--offline] [--sync]"
                );
                println!("       jiancha tui");
                println!("       jiancha sync [--group NAME] [--pull|--reset]");
                std::process::exit(0);
            }
            _ => return Err(format!("Unknown argument: {arg}").into()),
        }
    }

    if args.fresh && args.offline {
        return Err("--fresh and --offline cannot be used together".into());
    }
    Ok(args)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let cache_dir = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache")
    } else {
        return Err("HOME not set".into());
    };
    Ok(cache_dir.join("jiancha").join("cache.toml"))
}

fn load_cache(path: &Path) -> Cache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| toml::from_str(&contents).ok())
        .unwrap_or_default()
}

fn save_cache(path: &Path, cache: &Cache) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("toml.tmp");
    std::fs::write(&tmp_path, toml::to_string_pretty(cache)?)?;
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

fn cached_fetch_status(entry: &CacheEntry, repo: &RepoRow, now: u64) -> Option<FetchStatus> {
    if entry.branch != repo.branch.as_deref().unwrap_or("") || entry.upstream != repo.upstream {
        return None;
    }
    let age = now.saturating_sub(entry.fetched_at);
    let ttl = match entry.fetch_status {
        FetchStatus::Error => ERROR_RETRY_TTL_SECS,
        _ => REMOTE_TTL_SECS,
    };
    (age <= ttl).then(|| entry.fetch_status.clone())
}

fn terminal_width() -> Option<u16> {
    Command::new("sh")
        .args(["-c", "stty size < /dev/tty"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.split_whitespace()
                .nth(1)
                .and_then(|w| w.parse::<u16>().ok())
        })
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

fn status_symbol(repo: &RepoRow) -> &'static str {
    if repo.local_error.is_some() {
        "?"
    } else {
        match (repo.clean.unwrap_or(false), repo.has_unpushed) {
            (true, Some(false)) => "✓",
            (true, Some(true)) => "↑",
            (true, None) => "⚠",
            _ => "✗",
        }
    }
}

fn status_color(repo: &RepoRow) -> Color {
    match status_symbol(repo) {
        "✓" => Color::Green,
        "✗" => Color::Red,
        _ => Color::Yellow,
    }
}

fn remote_text(repo: &RepoRow) -> String {
    match &repo.fetch_status {
        FetchStatus::Pending => "...".to_string(),
        FetchStatus::UpToDate => "✓".to_string(),
        FetchStatus::Behind(n) => format!("↓ {n}"),
        FetchStatus::Error => "err".to_string(),
    }
}

fn remote_color(repo: &RepoRow) -> Color {
    match repo.fetch_status {
        FetchStatus::UpToDate => Color::Green,
        FetchStatus::Behind(_) => Color::Yellow,
        FetchStatus::Error => Color::Red,
        FetchStatus::Pending => Color::Grey,
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
        output.push_str(&format!(
            "\x1b[1;38;2;255;140;0m    {}\x1b[0m\n",
            section_name.to_uppercase()
        ));
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

        let mut header =
            vec![Cell::new(if full_size { "Repository" } else { "Repo" }).fg(Color::Cyan)];
        if show_branch {
            header.push(Cell::new(if full_size { "Branch" } else { "Br" }).fg(Color::Magenta));
        }
        header.push(
            Cell::new(if full_size { "Status" } else { "St" }).fg(Color::Rgb {
                r: 119,
                g: 136,
                b: 153,
            }),
        );
        if show_last_commit {
            header.push(
                Cell::new(if full_size { "Last Commit" } else { "Last" }).fg(Color::Rgb {
                    r: 184,
                    g: 134,
                    b: 11,
                }),
            );
        }
        if show_remote {
            header.push(
                Cell::new(if full_size { "Remote" } else { "R" }).fg(Color::Rgb {
                    r: 100,
                    g: 200,
                    b: 100,
                }),
            );
        }
        if show_error {
            header.push(Cell::new(if full_size { "Error" } else { "Err" }).fg(Color::Red));
        }
        table.set_header(header);

        if full_size {
            for (col, width) in [15, 18, 8, 35, 10, 8].iter().enumerate() {
                table
                    .column_mut(col)
                    .unwrap()
                    .set_constraint(ColumnConstraint::Absolute(Width::Fixed(*width)))
                    .set_padding((0, 1));
            }
        }

        for &idx in repo_indices {
            let repo = &repos[idx];
            let branch = repo.branch.as_deref().unwrap_or("");
            let last_commit = repo.last_commit.as_deref().unwrap_or("");
            let error = repo.local_error.as_deref().unwrap_or("-");
            if full_size {
                table.add_row(vec![
                    Cell::new(truncate_string(&repo.repo_key, 13)),
                    Cell::new(truncate_string(branch, 16)),
                    Cell::new(status_symbol(repo))
                        .fg(status_color(repo))
                        .add_attribute(Attribute::Bold),
                    Cell::new(truncate_string(last_commit, 33)),
                    Cell::new(remote_text(repo)).fg(remote_color(repo)),
                    Cell::new(truncate_string(error, 6)),
                ]);
            } else {
                let mut row = vec![Cell::new(truncate_string(&repo.repo_key, 40))];
                if show_branch {
                    row.push(Cell::new(truncate_string(branch, 40)));
                }
                row.push(
                    Cell::new(status_symbol(repo))
                        .fg(status_color(repo))
                        .add_attribute(Attribute::Bold),
                );
                if show_last_commit {
                    row.push(Cell::new(truncate_string(last_commit, 80)));
                }
                if show_remote {
                    row.push(Cell::new(remote_text(repo)).fg(remote_color(repo)));
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

fn render_compact(repos: &[RepoRow], sections: &IndexMap<String, Vec<usize>>) -> String {
    let mut out = String::new();
    for (section, indices) in sections {
        out.push_str(&format!("{}\n", section.to_uppercase()));
        for &idx in indices {
            let repo = &repos[idx];
            let branch = repo.branch.as_deref().unwrap_or("-");
            let note = repo
                .local_error
                .as_deref()
                .unwrap_or_else(|| repo.last_commit.as_deref().unwrap_or(""));
            out.push_str(&format!(
                "  {} {:>4}  {:<14} {:<16}  {}\n",
                status_symbol(repo),
                remote_text(repo),
                truncate_string(&repo.repo_key, 14),
                truncate_string(branch, 16),
                truncate_string(note, 48)
            ));
        }
        out.push('\n');
    }
    out
}

fn render_summary(repos: &[RepoRow], sections: &IndexMap<String, Vec<usize>>) -> String {
    let dirty = repos.iter().filter(|r| r.clean == Some(false)).count();
    let ahead = repos
        .iter()
        .filter(|r| r.has_unpushed == Some(true))
        .count();
    let behind = repos
        .iter()
        .filter(|r| matches!(r.fetch_status, FetchStatus::Behind(_)))
        .count();
    let errors = repos.iter().filter(|r| r.local_error.is_some()).count();
    let mut out = format!(
        "repos {} | dirty {} | ahead {} | behind {} | errors {}\n",
        repos.len(),
        dirty,
        ahead,
        behind,
        errors
    );
    for (section, indices) in sections {
        let mut problems = Vec::new();
        for &idx in indices {
            let repo = &repos[idx];
            if repo.local_error.is_some()
                || repo.clean == Some(false)
                || repo.has_unpushed == Some(true)
                || matches!(
                    repo.fetch_status,
                    FetchStatus::Behind(_) | FetchStatus::Error
                )
            {
                problems.push(format!(
                    "{}:{}{}",
                    repo.repo_key,
                    status_symbol(repo),
                    remote_text(repo)
                ));
            }
        }
        if problems.is_empty() {
            out.push_str(&format!("{}: ✓\n", section));
        } else {
            out.push_str(&format!("{}: {}\n", section, problems.join("  ")));
        }
    }
    out
}

fn load_repos(config: &Config) -> (Vec<RepoRow>, IndexMap<String, Vec<usize>>) {
    let mut repos = Vec::new();
    let mut sections: IndexMap<String, Vec<usize>> = IndexMap::new();
    for group in &config.group {
        for repo_cfg in &group.repo {
            let idx = repos.len();
            sections.entry(group.name.clone()).or_default().push(idx);
            let dir_path = PathBuf::from(&repo_cfg.fs);
            let resolved = match dir_path.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    repos.push(repo_error(
                        group,
                        repo_cfg,
                        repo_cfg.fs.clone(),
                        "Not a valid directory",
                    ));
                    continue;
                }
            };
            if !is_git_repo(&resolved) {
                repos.push(repo_error(
                    group,
                    repo_cfg,
                    resolved.to_string_lossy().into_owned(),
                    "Not a Git repository",
                ));
                continue;
            }
            let dir_s = resolved.to_string_lossy().into_owned();
            let (branch, last_commit, clean, has_unpushed, upstream, local_error) =
                match get_local_info(&dir_s) {
                    Some(info) => (
                        Some(info.branch),
                        Some(info.last_commit),
                        Some(info.clean),
                        info.has_unpushed,
                        info.upstream,
                        None,
                    ),
                    None => (
                        None,
                        None,
                        None,
                        None,
                        None,
                        Some("Failed to get git info".into()),
                    ),
                };
            repos.push(RepoRow {
                repo_key: repo_cfg.name.clone(),
                group: group.name.clone(),
                directory: dir_s,
                working_branch: repo_cfg.working_branch.clone(),
                sync_branch: repo_cfg.sync.clone(),
                remote: repo_cfg.remote.clone(),
                allow_reset: repo_cfg.allow_reset.unwrap_or(false),
                branch,
                last_commit,
                clean,
                has_unpushed,
                upstream,
                local_error,
                fetch_status: FetchStatus::Pending,
            });
        }
    }
    (repos, sections)
}

fn repo_error(
    group: &GroupConfig,
    repo_cfg: &RepoConfig,
    directory: String,
    error: &str,
) -> RepoRow {
    RepoRow {
        repo_key: repo_cfg.name.clone(),
        group: group.name.clone(),
        directory,
        working_branch: repo_cfg.working_branch.clone(),
        sync_branch: repo_cfg.sync.clone(),
        remote: repo_cfg.remote.clone(),
        allow_reset: repo_cfg.allow_reset.unwrap_or(false),
        branch: None,
        last_commit: None,
        clean: None,
        has_unpushed: None,
        upstream: None,
        local_error: Some(error.into()),
        fetch_status: FetchStatus::Pending,
    }
}

fn refresh_remote_status(repos: &mut [RepoRow], args: &Args, cache: &mut Cache, now: u64) {
    let force_session_refresh = cache
        .last_run_at
        .map(|last_run_at| now.saturating_sub(last_run_at) > SESSION_GAP_REFRESH_SECS)
        .unwrap_or(true);
    let mut fetch_indices = Vec::new();
    for (idx, repo) in repos.iter_mut().enumerate() {
        if repo.local_error.is_some() {
            continue;
        }
        let cached_status = cache
            .repos
            .get(&repo.directory)
            .and_then(|entry| cached_fetch_status(entry, repo, now));
        if !args.fresh && !force_session_refresh {
            if let Some(status) = cached_status.clone() {
                repo.fetch_status = status;
                continue;
            }
        }
        if args.offline {
            if let Some(status) = cached_status {
                repo.fetch_status = status;
            }
            continue;
        }
        fetch_indices.push(idx);
    }
    let handles: Vec<(usize, thread::JoinHandle<FetchStatus>)> = fetch_indices
        .into_iter()
        .map(|idx| {
            let dir = repos[idx].directory.clone();
            (idx, thread::spawn(move || run_git_fetch(&dir)))
        })
        .collect();
    for (idx, handle) in handles {
        let status = handle.join().unwrap_or(FetchStatus::Error);
        repos[idx].fetch_status = status.clone();
        cache.repos.insert(
            repos[idx].directory.clone(),
            CacheEntry {
                repo_key: repos[idx].repo_key.clone(),
                branch: repos[idx].branch.clone().unwrap_or_default(),
                upstream: repos[idx].upstream.clone(),
                fetched_at: now,
                fetch_status: status,
            },
        );
    }
    cache.last_run_at = Some(now);
}

fn selected_for_sync(repo: &RepoRow, sync: &SyncArgs) -> bool {
    sync.group
        .as_deref()
        .map(|group| group == repo.group.as_str())
        .unwrap_or(true)
}

fn sync_repo(repo: &RepoRow, reset: bool, yes: bool) -> (bool, String) {
    if let Some(err) = &repo.local_error {
        return (false, format!("skip {}: {}", repo.repo_key, err));
    }
    let remote = repo.remote.as_deref().unwrap_or("origin");
    if let Err(e) = git_status(&repo.directory, &["fetch", "--all", "--prune"]) {
        return (false, format!("fail {}: fetch: {}", repo.repo_key, e));
    }
    if reset {
        let branch = match &repo.branch {
            Some(branch) => branch,
            None => return (false, format!("skip {}: unknown branch", repo.repo_key)),
        };
        let working_branch = match &repo.working_branch {
            Some(branch) => branch,
            None => {
                return (
                    false,
                    format!("skip {}: missing working_branch", repo.repo_key),
                )
            }
        };
        if branch != working_branch {
            return (
                false,
                format!(
                    "skip {}: on {}, expected {}",
                    repo.repo_key, branch, working_branch
                ),
            );
        }
        if !repo.allow_reset && !yes {
            return (
                false,
                format!(
                    "skip {}: reset needs allow_reset=true or --yes",
                    repo.repo_key
                ),
            );
        }
        let target = format!("{remote}/{working_branch}");
        match git_status(&repo.directory, &["reset", "--hard", &target]) {
            Ok(_) => (true, format!("reset {} -> {}", repo.repo_key, target)),
            Err(e) => (false, format!("fail {}: reset: {}", repo.repo_key, e)),
        }
    } else {
        if repo.clean != Some(true) {
            return (false, format!("skip {}: dirty working tree", repo.repo_key));
        }
        match git_status(&repo.directory, &["pull", "--ff-only"]) {
            Ok(_) => (true, format!("sync {}", repo.repo_key)),
            Err(e) => (false, format!("fail {}: pull: {}", repo.repo_key, e)),
        }
    }
}

fn auto_sync_repo(repo: &RepoRow) -> (bool, String) {
    if let Some(err) = &repo.local_error {
        return (false, format!("skip {}: {}", repo.repo_key, err));
    }
    let branch = match &repo.branch {
        Some(branch) => branch,
        None => return (false, format!("skip {}: unknown branch", repo.repo_key)),
    };
    let sync_branch = match &repo.sync_branch {
        Some(branch) => branch,
        None => {
            return (
                false,
                format!("skip {}: missing sync branch", repo.repo_key),
            )
        }
    };
    if branch != sync_branch {
        return (
            false,
            format!(
                "skip {}: on {}, expected {}",
                repo.repo_key, branch, sync_branch
            ),
        );
    }

    let remote = repo.remote.as_deref().unwrap_or("origin");
    if let Err(e) = git_status(&repo.directory, &["fetch", "--all", "--prune"]) {
        return (false, format!("fail {}: fetch: {}", repo.repo_key, e));
    }
    if git_status(&repo.directory, &["pull", "--ff-only"]).is_ok() {
        return (true, format!("sync {}", repo.repo_key));
    }

    let target = format!("{remote}/{sync_branch}");
    match git_status(&repo.directory, &["reset", "--hard", &target]) {
        Ok(_) => (true, format!("reset {} -> {}", repo.repo_key, target)),
        Err(e) => (false, format!("fail {}: reset: {}", repo.repo_key, e)),
    }
}

fn run_auto_sync(repos: &[RepoRow]) {
    let mut ok = 0;
    let mut skipped_or_failed = 0;
    for repo in repos {
        let (success, msg) = auto_sync_repo(repo);
        if success {
            ok += 1;
        } else {
            skipped_or_failed += 1;
        }
        println!("{msg}");
    }
    println!("done: {ok} ok, {skipped_or_failed} skipped/failed");
}

fn run_sync(repos: &[RepoRow], sync: &SyncArgs) {
    let mut ok = 0;
    let mut skipped_or_failed = 0;
    for repo in repos.iter().filter(|r| selected_for_sync(r, sync)) {
        let (success, msg) = sync_repo(repo, sync.reset, true);
        if success {
            ok += 1;
        } else {
            skipped_or_failed += 1;
        }
        println!("{msg}");
    }
    println!("done: {ok} ok, {skipped_or_failed} skipped/failed");
}

fn repo_counts(repos: &[RepoRow]) -> (usize, usize, usize, usize, usize) {
    (
        repos.iter().filter(|r| r.clean == Some(true)).count(),
        repos.iter().filter(|r| r.clean == Some(false)).count(),
        repos
            .iter()
            .filter(|r| r.has_unpushed == Some(true))
            .count(),
        repos
            .iter()
            .filter(|r| matches!(r.fetch_status, FetchStatus::Behind(_)))
            .count(),
        repos.iter().filter(|r| r.local_error.is_some()).count(),
    )
}

fn to_ratatui_color(color: Color) -> ratatui::style::Color {
    match color {
        Color::Green => ratatui::style::Color::Green,
        Color::Red => ratatui::style::Color::Red,
        Color::Yellow => ratatui::style::Color::Yellow,
        Color::Grey => ratatui::style::Color::Gray,
        Color::Cyan => ratatui::style::Color::Cyan,
        Color::Magenta => ratatui::style::Color::Magenta,
        _ => ratatui::style::Color::White,
    }
}

struct App {
    repos: Vec<RepoRow>,
    selected: usize,
    message: String,
}

impl App {
    fn selected_repo(&self) -> Option<&RepoRow> {
        self.repos.get(self.selected)
    }
    fn next(&mut self) {
        if !self.repos.is_empty() {
            self.selected = (self.selected + 1).min(self.repos.len() - 1);
        }
    }
    fn prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
    fn next_group(&mut self) {
        let Some(current) = self.selected_repo().map(|r| r.group.clone()) else {
            return;
        };
        if let Some((idx, _)) = self
            .repos
            .iter()
            .enumerate()
            .skip(self.selected + 1)
            .find(|(_, r)| r.group != current)
        {
            self.selected = idx;
        }
    }
    fn prev_group(&mut self) {
        let Some(current) = self.selected_repo().map(|r| r.group.clone()) else {
            return;
        };
        if let Some((idx, _)) = self
            .repos
            .iter()
            .enumerate()
            .take(self.selected)
            .rev()
            .find(|(_, r)| r.group != current)
        {
            let group = self.repos[idx].group.clone();
            self.selected = self
                .repos
                .iter()
                .position(|r| r.group == group)
                .unwrap_or(idx);
        }
    }
}

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn repo_card_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let card_width = width.saturating_sub(4).max(32) as usize;
    let inner_width = card_width.saturating_sub(2);
    let mut lines = Vec::new();
    let mut idx = 0;

    while idx < app.repos.len() {
        let group = app.repos[idx].group.clone();
        let title = format!(" {} ", group.to_uppercase());
        let rule_len = inner_width.saturating_sub(display_width(&title) + 1);
        lines.push(Line::from(vec![
            Span::styled("╭", Style::default().fg(ratatui::style::Color::DarkGray)),
            Span::styled(
                format!("─{}{}", title, "─".repeat(rule_len)),
                Style::default()
                    .fg(ratatui::style::Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("╮", Style::default().fg(ratatui::style::Color::DarkGray)),
        ]));

        while idx < app.repos.len() && app.repos[idx].group == group {
            let repo = &app.repos[idx];
            let selected = idx == app.selected;
            let row_style = if selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            let branch = repo.branch.as_deref().unwrap_or("-");
            let note = repo
                .local_error
                .as_deref()
                .or(repo.last_commit.as_deref())
                .unwrap_or("");
            let status = format!(" {} ", status_symbol(repo));
            let remote = format!("{:>4} ", remote_text(repo));
            let fixed_width = 1 + 14 + 1 + 13 + 1;
            let note_width = inner_width
                .saturating_sub(display_width(&status) + display_width(&remote) + fixed_width);
            let rest = format!(
                " {:<14} {:<13} {}",
                truncate_string(&repo.repo_key, 14),
                truncate_string(branch, 13),
                truncate_string(note, note_width)
            );
            let used = display_width(&status) + display_width(&remote) + display_width(&rest);
            let pad = " ".repeat(inner_width.saturating_sub(used));
            let icon_modifier = if selected {
                Modifier::REVERSED
            } else {
                Modifier::empty()
            };
            lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(ratatui::style::Color::DarkGray)),
                Span::styled(
                    status,
                    Style::default()
                        .fg(to_ratatui_color(status_color(repo)))
                        .add_modifier(icon_modifier),
                ),
                Span::styled(
                    remote,
                    Style::default()
                        .fg(to_ratatui_color(remote_color(repo)))
                        .add_modifier(icon_modifier),
                ),
                Span::styled(rest, row_style),
                Span::styled(pad, row_style),
                Span::styled("│", Style::default().fg(ratatui::style::Color::DarkGray)),
            ]));
            idx += 1;
        }

        lines.push(Line::from(vec![
            Span::styled("╰", Style::default().fg(ratatui::style::Color::DarkGray)),
            Span::styled(
                "─".repeat(inner_width),
                Style::default().fg(ratatui::style::Color::DarkGray),
            ),
            Span::styled("╯", Style::default().fg(ratatui::style::Color::DarkGray)),
        ]));
        lines.push(Line::from(""));
    }

    lines
}

fn draw_tui(f: &mut ratatui::Frame, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(root[1]);

    let (clean, dirty, ahead, behind, errors) = repo_counts(&app.repos);
    let header = Paragraph::new(Line::from(vec![
        Span::raw(format!("repos: {}  ", app.repos.len())),
        Span::styled(
            format!("clean: {clean}  "),
            Style::default().fg(ratatui::style::Color::Green),
        ),
        Span::styled(
            format!("dirty: {dirty}  "),
            Style::default().fg(ratatui::style::Color::Red),
        ),
        Span::styled(
            format!("ahead: {ahead}  "),
            Style::default().fg(ratatui::style::Color::Yellow),
        ),
        Span::styled(
            format!("behind: {behind}  "),
            Style::default().fg(ratatui::style::Color::Yellow),
        ),
        Span::styled(
            format!("errors: {errors}"),
            Style::default().fg(ratatui::style::Color::Yellow),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, root[0]);

    let repo_cards = repo_card_lines(app, body[0].width);
    f.render_widget(
        Paragraph::new(repo_cards)
            .wrap(Wrap { trim: false })
            .block(Block::default().title("repos").borders(Borders::ALL)),
        body[0],
    );

    let detail_title = app
        .selected_repo()
        .map(|r| format!("detail: {}", r.repo_key))
        .unwrap_or_else(|| "detail".to_string());
    let detail = if let Some(repo) = app.selected_repo() {
        vec![
            Line::from(format!("repo: {}", repo.repo_key)),
            Line::from(format!("group: {}", repo.group)),
            Line::from(format!("fs: {}", repo.directory)),
            Line::from(format!("branch: {}", repo.branch.as_deref().unwrap_or("-"))),
            Line::from(format!(
                "working_branch: {}",
                repo.working_branch.as_deref().unwrap_or("-")
            )),
            Line::from(format!(
                "upstream: {}",
                repo.upstream.as_deref().unwrap_or("-")
            )),
            Line::from(format!(
                "status: {} remote {}",
                status_symbol(repo),
                remote_text(repo)
            )),
            Line::from(format!(
                "last: {}",
                repo.last_commit.as_deref().unwrap_or("-")
            )),
        ]
    } else {
        vec![Line::from("no repos")]
    };
    f.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .block(Block::default().title(detail_title).borders(Borders::ALL)),
        body[1],
    );

    let footer = Line::from(vec![
        Span::raw(format!("{}  ", app.message)),
        Span::styled("j/k", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" move  "),
        Span::styled("J/K", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" group  "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" refresh  "),
        Span::styled("s", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" sync  "),
        Span::styled("S", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" sync-all  "),
        Span::styled("x", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" reset  "),
        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quit"),
    ]);
    f.render_widget(
        Paragraph::new(footer).block(Block::default().title("keys").borders(Borders::ALL)),
        root[2],
    );
}

fn reload_app(
    config: &Config,
    args: &Args,
    cache_path: &Path,
) -> Result<App, Box<dyn std::error::Error>> {
    let mut cache = load_cache(cache_path);
    let (mut repos, _) = load_repos(config);
    refresh_remote_status(&mut repos, args, &mut cache, now_secs());
    save_cache(cache_path, &cache)?;
    Ok(App {
        repos,
        selected: 0,
        message: "ready".into(),
    })
}

fn run_tui(
    config: &Config,
    args: &Args,
    cache_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = reload_app(config, args, cache_path)?;

    let result = loop {
        terminal.draw(|f| draw_tui(f, &mut app))?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                    KeyCode::Char('j') | KeyCode::Down => app.next(),
                    KeyCode::Char('k') | KeyCode::Up => app.prev(),
                    KeyCode::Char('J') => app.next_group(),
                    KeyCode::Char('K') => app.prev_group(),
                    KeyCode::Char('g') => app.selected = 0,
                    KeyCode::Char('G') => {
                        if !app.repos.is_empty() {
                            app.selected = app.repos.len() - 1;
                        }
                    }
                    KeyCode::Char('r') => match reload_app(config, args, cache_path) {
                        Ok(next) => {
                            let selected = app.selected.min(next.repos.len().saturating_sub(1));
                            app = next;
                            app.selected = selected;
                            app.message = "refreshed".into();
                        }
                        Err(e) => app.message = format!("refresh failed: {e}"),
                    },
                    KeyCode::Char('s') => {
                        if let Some(repo) = app.selected_repo() {
                            let (ok, msg) = sync_repo(repo, false, false);
                            app.message = msg;
                            if ok {
                                if let Ok(next) = reload_app(config, args, cache_path) {
                                    let selected =
                                        app.selected.min(next.repos.len().saturating_sub(1));
                                    app = next;
                                    app.selected = selected;
                                }
                            }
                        }
                    }
                    KeyCode::Char('S') => {
                        let mut ok = 0;
                        let mut skipped = 0;
                        for repo in &app.repos {
                            let (success, _) = sync_repo(repo, false, false);
                            if success {
                                ok += 1;
                            } else {
                                skipped += 1;
                            }
                        }
                        app.message = format!("sync all safe: {ok} ok, {skipped} skipped/failed");
                        if let Ok(next) = reload_app(config, args, cache_path) {
                            let selected = app.selected.min(next.repos.len().saturating_sub(1));
                            app = next;
                            app.selected = selected;
                        }
                    }
                    KeyCode::Char('x') => {
                        if let Some(repo) = app.selected_repo() {
                            let (_, msg) = sync_repo(repo, true, false);
                            app.message = msg;
                        }
                    }
                    _ => {}
                }
            }
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let config = load_config()?;
    let cache_path = cache_path()?;
    let (mut repos, sections) = load_repos(&config);
    let mut cache = load_cache(&cache_path);

    match &args.command {
        CommandMode::Check => {
            refresh_remote_status(&mut repos, &args, &mut cache, now_secs());
            save_cache(&cache_path, &cache)?;
            match args.output {
                OutputMode::Full => print!("{}", render_all(&repos, &sections)),
                OutputMode::Compact => print!("{}", render_compact(&repos, &sections)),
                OutputMode::Summary => print!("{}", render_summary(&repos, &sections)),
            }
        }
        CommandMode::Tui => run_tui(&config, &args, &cache_path)?,
        CommandMode::Sync(sync) => run_sync(&repos, sync),
        CommandMode::AutoSync => run_auto_sync(&repos),
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
    }
}

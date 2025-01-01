import json
from pathlib import Path
import subprocess

from rich.table import Table
from rich.console import Console


def is_git_repo(directory):
    try:
        subprocess.run(["git", "-C", directory, "status"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        return True
    except subprocess.CalledProcessError:
        return False


def get_git_info(directory):
    """Retrieve Git information for the repository."""
    try:
        branch = subprocess.check_output(
            ["git", "-C", directory, "rev-parse", "--abbrev-ref", "HEAD"],
            text=True
        ).strip()
        last_commit = subprocess.check_output(
            ["git", "-C", directory, "log", "-1", "--pretty=%B"],
            text=True
        ).strip()
        status_output = subprocess.check_output(
            ["git", "-C", directory, "status", "--porcelain"],
            text=True
        )
        clean = not bool(status_output.strip())
        print(f"[DEBUG] Directory: {directory}, Clean: {clean}")
        return {
            "branch": branch,
            "last_commit": last_commit,
            "clean": clean
        }
    except subprocess.CalledProcessError as e:
        print(f"[DEBUG] Error retrieving git info for {directory}: {e}")
        return None


def load_config(config_path):
    with open(config_path, "r") as f:
        return json.load(f)


def main(config_path):
    config = load_config(config_path)
    directories = config.get("directories", [])
    results = []
    for directory in directories:
        dir_path = Path(directory).resolve()
        if not dir_path.is_dir():
            results.append({"directory": str(dir_path), "error": "Not a valid directory"})
            continue
        if not is_git_repo(dir_path):
            results.append({"directory": str(dir_path), "error": "Not a Git repository"})
            continue
        git_info = get_git_info(dir_path)
        if git_info:
            results.append({
                "directory": str(dir_path),
                "branch": git_info["branch"],
                "last_commit": git_info["last_commit"],
                "clean": git_info["clean"],
                "error": None
            })
        else:
            results.append({"directory": str(dir_path), "error": "Failed to retrieve Git info"})
    console = Console()
    table = Table(title="Git Repository Overview", show_lines=True)
    table.add_column("Directory", style="cyan", no_wrap=True)
    table.add_column("Branch", style="magenta")
    table.add_column("Clean", style="green")
    table.add_column("Last Commit", style="yellow", overflow="fold")
    table.add_column("Error", style="red")
    for result in results:
        print(f"[DEBUG] Result: {result}")
        style = "on red" if result.get("clean") is False else None
        table.add_row(
            result.get("directory", ""),
            result.get("branch", ""),
            "Yes" if result.get("clean") else "No",
            result.get("last_commit", ""),
            result.get("error", "") or "-",
            style=style
        )
    console.print(table)


if __name__ == "__main__":
    import sys
    if len(sys.argv) != 2:
        print("Usage: python script.py <config.json>")
        exit(1)
    main(sys.argv[1])

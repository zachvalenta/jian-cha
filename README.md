# basics

Keep track of git repos across machines.

`jiancha` shows local Git state live and caches remote fetch status so repeated checks stay fast.

```sh
# logging off for the day...

# "oh wait, do I need to push anything?"

$ jc

═══════════════════════════
    PROJECTS
═══════════════════════════
+------------+------------------+--------+-----------------------------------+----------+--------+
|Repository  |Branch            |Status  |Last Commit                        |Remote    |Error   |
+================================================================================================+
|foo         |main              |✓       |tidy: cursor on 'now' col on T...  |✓         |-       |
|------------+------------------+--------+-----------------------------------+----------+--------|
|bar         |main              |✗       |rdd: schedule                      |✓         |-       |
|------------+------------------+--------+-----------------------------------+----------+--------|
|baz         |main              |✓       |carl                               |✓         |-       |
+------------+------------------+--------+-----------------------------------+----------+--------+

# logging in the next morning...

# "Did I commit anything last night?"

$ jc

═══════════════════════════
    PROJECTS
═══════════════════════════
+------------+------------------+--------+-----------------------------------+----------+--------+
|Repository  |Branch            |Status  |Last Commit                        |Remote    |Error   |
+================================================================================================+
|foo         |main              |✓       |tidy: cursor on 'now' col on T...  |✓         |-       |
|------------+------------------+--------+-----------------------------------+----------+--------|
|bar         |main              |✗       |rdd: schedule                      |✓         |-       |
|------------+------------------+--------+-----------------------------------+----------+--------|
|baz         |main              |✓       |carl                               |✓         |-       |
+------------+------------------+--------+-----------------------------------+----------+--------+
```

# caching

`jiancha` caches remote status at:

```sh
$XDG_CACHE_HOME/jiancha/cache.toml
# fallback: ~/.cache/jiancha/cache.toml
```

Caching policy:

* local state is always live: branch, last commit, clean/dirty, unpushed commits
* remote state is cached: `git fetch` result and behind count
* default remote TTL: 30 minutes
* error retry TTL: 2 minutes
* session-gap refresh: if the previous `jiancha` run was more than 90 minutes ago, refresh all remotes
* cache entries are keyed by canonical repo path
* cache is invalidated when branch or upstream changes
* cache writes are atomic: write temp file, then rename

Flags:

```sh
jiancha --compact  # dense grouped output
jiancha --summary  # fleet summary + group exceptions
jiancha --fresh    # force fetch remotes, ignoring cache
jiancha --refresh  # alias for --fresh
jiancha --offline  # never fetch; use only fresh cached remote state
```

Hybrid UI / sync commands:

```sh
jiancha tui                # modal grouped repo browser
jiancha sync --all         # safe sync all repos
jiancha sync --group rdd   # safe sync one group
jiancha sync --repo tasks  # safe sync one repo
```

TUI keys:

```text
j/k       move repo
J/K       next/previous group
g/G       top/bottom
r         refresh
s         safe sync selected repo
S         safe sync all repos
x         reset selected repo, gated by allow_reset=true
q/Esc     quit
```

Safe sync runs:

```sh
git fetch --all --prune
git pull --ff-only
```

Reset sync runs:

```sh
git fetch --all --prune
git reset --hard origin/<working_branch>
```

Config format:

```toml
[[group]]
name = "context"

  [[group.repo]]
  name = "sw"
  fs = "/Users/zach/Documents/zv/notes/sw"
  working_branch = "sprint"

[[group]]
name = "rdd"

  [[group.repo]]
  name = "corkboard"
  fs = "/Users/zach/Documents/zv/projects/tools/corkboard"
  working_branch = "main"
  remote = "origin"
  allow_reset = true
```

The previous `[group] repo = "/path"` config style still works. Sync commands skip repos that do not define `working_branch`.

This is tuned for bouncing between machines every few hours: normal repeated checks avoid network fetches, while checks after a long gap usually refresh remote truth.

# TUI

CLI still works:

 ```sh
   jiancha
   jiancha --compact
   jiancha --summary
 ```

 New TUI:

 ```sh
   jiancha tui
 ```

 TUI keys

 ```text
   j/k       move repo
   J/K       next/previous group
   g/G       top/bottom
   r         refresh
   s         safe sync selected repo
   S         safe sync all repos
   x         reset selected repo, gated by allow_reset=true
   q/Esc     quit
 ```

# new sync

 New sync CLI:

 ```sh
   jiancha sync --all
   jiancha sync --group rdd
   jiancha sync --repo tasks
   jiancha sync --all --reset -y
 ```

 config

 Supports new shape:

 ```toml
   [[group]]
   name = "context"

     [[group.repo]]
     name = "sw"
     fs = "/Users/zach/Documents/zv/notes/sw"
     working_branch = "sprint"
 ```

 Legacy config still works. Sync skips legacy repos without working_branch.

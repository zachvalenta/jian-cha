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
jiancha --sync     # sync configured repos; reset after a failed fast-forward pull
```

Hybrid UI / sync commands:

```sh
jiancha tui                # modal grouped repo browser
jiancha sync --pull        # fast-forward pull all configured repos
jiancha sync --reset       # hard-reset all configured repos to their remotes
jiancha sync --group rdd   # fast-forward pull one group
```

Sync operations run up to 8 repositories concurrently. After `sync --pull` or
`sync --reset`, jiancha refreshes and saves remote status for the post-sync
working trees, so the next normal `jiancha` run uses fresh cached status.

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

Configuration is read from `$XDG_CONFIG_HOME/jiancha/config.yaml` (or `~/.config/jiancha/config.yaml`).

`jiancha --sync` uses an explicit `sync` branch. Repos without that key are
shown by normal `jiancha`, but skipped by `--sync`.

```yaml
notes:
  sw:
    path: /Users/zach/Documents/zv/notes/sw
    sync: sprint
  tasks:
    path: /Users/zach/Documents/zv/personal/corkboard-tasks
    sync: main
colligo:
  eng:
    path: /Users/zach/Documents/zv/work/colligo/docs/eng
    sync: main
```

Each configured repo is fetched, fast-forwarded, then hard-reset to
`origin/<sync>` when the pull cannot fast-forward (for example after a
force-push):

`--sync` only operates when the checked-out
branch matches the configured branch; its reset deliberately discards local
working-tree changes and commits.

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


# sync

thinking of a new flag for /Users/zach/Documents/zv/projects/tools/jian-cha

```sh
jiancha --sync
```

99% of my usage is: work on desktop|laptop, switch to other machine later, and try to remember what repos need to be updated:
```sh
jiancha  # see what needs to be updated

# cd around to various repos
# if I can merge cleanly
git pull

# if I can't merge, it's almost always bc there's a force push bc I didn't want to make a new commit
# so merge strat is just "`remote $BRANCH (sprint | main | master)" depending on what repo we're talking about
git fetch --all; git reset --hard origin/sprint
git fetch --all; git reset --hard origin/main
git fetch --all; git reset --hard origin/master

# double check I've got everything
jiancha --fresh
```

so I think we can resolve this with a code change + update the config:

```yaml
notes:
  sw:
    path: /Users/zach/Documents/zv/notes/sw
    sync: sprint
  tasks:
    path: /Users/zach/Documents/zv/personal/corkboard-tasks
    sync: main
colligo:
  eng:
    path: /Users/zach/Documents/zv/work/colligo/docs/eng
    sync: main
```

## cmds

```sh
jiancha sync --group rdd

# git pull for all
jiancha sync --pull

# hard-resets every configured working_branch repo to its remote branch
jiancha sync --reset
```

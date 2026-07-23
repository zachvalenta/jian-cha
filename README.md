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

## caching

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
jiancha --fresh    # force fetch remotes, ignoring cache
jiancha --refresh  # alias for --fresh
jiancha --offline  # never fetch; use only fresh cached remote state
```

This is tuned for bouncing between machines every few hours: normal repeated checks avoid network fetches, while checks after a long gap usually refresh remote truth.


# 👋 TLDR

This is a tool to:

* specify a list of repos I'm working with daily, especially those that exist on multiple machines and for which I want to avoid merge hell
* run gfold before leaving for/from the office and see "do I need to commit anything?"

# ☑️  TODO

- [x] alias for CLI
- [ ] check upstream
- [ ] handle parents (in addition to leaves)
- [ ] publish
- [ ] handle multiple statuses

# 🎛️ USAGE

All commands are runnable via the `Makefile`; just run `make` to see the documentation:
```sh
$ make

======================================================================

🚀  MAIN

run:       run app
repl:      start REPL

📦 DEPENDENCIES

env:        show environment info
deps:       list prod dependencies

======================================================================
```


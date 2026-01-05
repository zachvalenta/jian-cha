help:
	@echo
	@echo "======================================================================"
	@echo
	@echo "🚀  MAIN"
	@echo
	@echo "build:     build Rust binary to project root"
	@echo "install:   build and install to ~/Documents/denv/bin"
	@echo "run:       run Rust app"
	@echo "run-py:    run Python app"
	@echo "repl:      start REPL"
	@echo
	@echo "📦 DEPENDENCIES"
	@echo
	@echo "env:        show environment info"
	@echo "deps:       list prod dependencies"
	@echo
	@echo "======================================================================"
	@echo

#
# 🚀  MAIN
#

build:
	cargo build --release
	cp target/release/jiancha ./jiancha

install:
	cargo build --release
	cp target/release/jiancha /Users/zach/Documents/denv/bin/jiancha

run:
	./jiancha

run-py:
	python app.py

repl:
	export PYTHONSTARTUP='./startup.py' && ipython

#
# 📦 DEPENDENCIES
#

env:
	poetry run poetry env info

deps:
	poetry run poetry show --tree --no-dev

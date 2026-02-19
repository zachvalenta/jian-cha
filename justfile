#
# 🚀  MAIN
#

# build Rust binary to project root
build:
    cargo build --release
    cp target/release/jiancha ./jiancha

# build and install to ~/Documents/denv/bin
install:
    cargo build --release
    cp target/release/jiancha /Users/zach/Documents/denv/bin/jiancha

# run Rust app
run:
    ./jiancha

# run Python app
run-py:
    python app.py

# start REPL
repl:
    PYTHONSTARTUP='./startup.py' ipython

#
# 📦 DEPENDENCIES
#

# show environment info
env:
    poetry run poetry env info

# list prod dependencies
deps:
    poetry run poetry show --tree --no-dev

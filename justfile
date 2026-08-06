# run dev mode
run:
    ./jiancha

# run TUI
tui:
    ./jiancha tui

# build bin to project root
build:
    cargo build --release
    cp target/release/jiancha ./jiancha

# build bin and install to ~/Documents/denv/bin
install:
    cargo build --release
    cp target/release/jiancha /Users/zach/Documents/denv/bin/jiancha

#
# 🚀  MAIN
#

# run Rust app
run:
    ./jiancha

# build Rust binary to project root
build:
    cargo build --release
    cp target/release/jiancha ./jiancha

# build and install to ~/Documents/denv/bin
install:
    cargo build --release
    cp target/release/jiancha /Users/zach/Documents/denv/bin/jiancha

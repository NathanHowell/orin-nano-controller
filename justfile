check:
    cargo fmt --check
    cargo clippy
    cargo nextest run --target $(rustc -Vv | sed -n 's/^host: //p')

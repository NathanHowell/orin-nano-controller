check:
    cargo fmt --check
    cargo clippy
    cargo nextest run --target $(rustc -Vv | sed -n 's/^host: //p')

test:
    # Host compilation smoke check
    cargo check -p controller-core
    # Cross-compilation smoke check for the MCU target
    cargo check -p controller-core --target thumbv6m-none-eabi --no-default-features

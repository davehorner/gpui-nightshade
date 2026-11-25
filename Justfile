set windows-shell := ["pwsh", "-NoLogo", "-NoProfile", "-Command"]

# Initialize pinned upstream sources and apply the isolated Nightshade patch.
init:
    pwsh -NoLogo -NoProfile -File scripts/init.ps1

check:
    cargo check --workspace --all-targets --locked

build:
    cargo build --workspace --locked

test:
    cargo test --workspace --locked

test-gpu:
    cargo test -p gpui-nightshade --locked -- --include-ignored

headless:
    cargo run --locked --example headless

cube backend="auto":
    cargo run --locked -p gpui-nightshade-viewer -- --presentation={{backend}}

viewer backend="auto":
    cargo run --locked -p gpui-nightshade-viewer -- --presentation={{backend}}

multi-viewport backend="auto":
    cargo run --locked -p gpui-nightshade-viewer -- --presentation={{backend}} --multi-viewport

benchmark:
    cargo run --locked --example presentation_benchmark

fmt:
    cargo fmt -p gpui-nightshade -p gpui-nightshade-components -p gpui-nightshade-viewer

clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

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

test-core:
    cargo test --manifest-path upstream/nightshade/Cargo.toml -p viewer --lib core::tests --locked --target-dir target/upstream-check

test-gpu:
    cargo test -p gpui-nightshade --locked -- --include-ignored --test-threads=1
    cargo test -p gpui-nightshade-components --locked -- --include-ignored --test-threads=1
    cargo test -p gpui-pre-windows external_eviction_retains_import_until_consumer_completion --locked -- --ignored --test-threads=1

test-sampling:
    cargo test -p gpui-nightshade-components windows::sampling_test --locked -- --ignored --nocapture --test-threads=1

test-shared frames="1200" port="19790":
    cargo build -p gpui-nightshade-viewer --locked
    pwsh -NoLogo -NoProfile -File scripts/verify-windows-shared.ps1 -Frames {{frames}} -Port {{port}}

test-shared-stress frames="1200" port="19791":
    cargo build -p gpui-nightshade-viewer --locked
    pwsh -NoLogo -NoProfile -File scripts/verify-windows-shared.ps1 -Frames {{frames}} -Port {{port}} -Stress

headless:
    cargo run --locked --example headless

cube backend="auto":
    cargo run --locked -p gpui-nightshade-viewer -- --cube --presentation={{backend}}

viewer backend="readback":
    cargo run --locked -p gpui-nightshade-viewer -- --presentation={{backend}}

gltf model backend="readback":
    cargo run --locked -p gpui-nightshade-viewer -- --presentation={{backend}} --model="{{model}}"

multi-viewport backend="auto":
    cargo run --locked -p gpui-nightshade-viewer -- --presentation={{backend}} --multi-viewport

benchmark:
    cargo run --locked --example presentation_benchmark

fmt:
    cargo fmt -p gpui-nightshade -p gpui-nightshade-components -p gpui-nightshade-viewer -p nightshade-viewer-core

clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

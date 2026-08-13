# Twentytoo development commands.
#
# Run `just` (or `just --list`) to see all recipes.

set shell := ["sh", "-c"]

# List available recipes.
default:
    @just --list

# Build the whole workspace.
build:
    cargo build

# Run the demo app (two resources on InMemoryAdapter) at http://127.0.0.1:3000.
demo:
    cargo run -p twentytoo --example demo

# Run unit + doctests.
test:
    cargo test --workspace

# Lint with the CI gate: warnings are errors.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Check formatting (CI gate).
fmt:
    cargo fmt --all --check

# Auto-format in place.
fmt-fix:
    cargo fmt --all

# Run every CI gate locally.
ci: fmt clippy test

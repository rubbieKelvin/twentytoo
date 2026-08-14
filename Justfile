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

# Run the demo app (two resources on InMemoryAdapter behind the login flow)
# at http://127.0.0.1:3000. Needs the compose Postgres.
demo: db-up
    cargo run -p twentytoo --example demo

# Run the demo app, rebuilding on source change (needs `cargo install cargo-watch`).
watch:
    cargo watch -x "run -p twentytoo --example demo"

# Start the Postgres dev database (Docker).
db-up:
    docker compose up -d db

# Run the db-crate integration tests against the Docker Postgres.
db-test:
    DATABASE_URL=postgres://twentytoo:twentytoo@localhost:5433/twentytoo cargo test -p twentytoo-db

# Run the whole stack (app + Postgres) in Docker.
up:
    docker compose up --build

# Stop the whole stack.
down:
    docker compose down

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

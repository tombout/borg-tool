default:
    @just --list

# Run the borg-tool
run:
    cargo run

# Format the entire workspace / crate
fmt:
    cargo fmt

# Fast compile-like check without producing binaries
check:
    cargo check

# Small / fast test set
test:
    cargo test --quiet

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# ------------------------------------------
# Pre-commit checks
# ------------------------------------------

# Run all pre-commit checks for the agent
pre-commit: fmt check test
    @echo "✅ pre-commit checks for agent passed"

# ------------------------------------------
# Pre-push checks
# ------------------------------------------

# Slightly heavier checks before pushing to remote
pre-push: fmt check test clippy
    @echo "✅ pre-push checks passed"

# ------------------------------------------
# Helpers for heavier checks
# ------------------------------------------

# Run the full test suite (unit + integration)
tests-all:
    cargo test --all --quiet

# Build an optimized release binary
build-release:
    cargo build --release

# ------------------------------------------
# Pre-release checks
# ------------------------------------------

# Strong gate before cutting a release
pre-release: fmt clippy tests-all build-release
    @echo "🚀 pre-release checks passed – ready to release"

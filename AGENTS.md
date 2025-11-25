# AGENTS.md – Rust Dev Playbook

Concise rules for working on any Rust code in this repo. Favor correctness, clarity, and small, reviewable changes.

---

## Commands You Should Run
- Build / check: `cargo check` for fast iteration; `cargo build` before pushing.
- Tests: `cargo test` (or `cargo test <name>` to scope). Keep tests fast and deterministic.
- Lints & fmt: `cargo fmt` then `cargo clippy --all-targets --all-features` **before every commit**.
- Run binary: `cargo run -- <args>` (avoid hard‑coding secrets in args).
- Add crates: edit `Cargo.toml`, then `cargo check` to verify the lockfile change.

## Testing Expectations
- Add tests with new behavior: unit tests inline with modules; integration tests in `tests/` for workflows or public APIs.
- Name tests descriptively (e.g., `handles_empty_input`, `fails_on_invalid_config`); prefer table-driven cases for variants.
- Avoid flakiness: no network or time dependence without guards; seed RNGs.
- Treat panics as bugs unless truly unrecoverable; assert error variants instead.

## Project Structure
- Binary: entry in `src/main.rs`; move logic into modules so `main` wires dependencies only.
- Library: core API in `src/lib.rs`; keep public surface minimal and documented.
- Modules: group by concern (`config`, `domain`, `infra`, `cli`); keep nesting shallow and cohesive.
- Features: add cargo features only for optional behavior; keep graphs simple and documented in `Cargo.toml`.
- Dependencies: prefer stable, well‑maintained crates; justify non-standard additions and keep them scoped.

## Code Style
- Rust edition: assume stable 2021 unless `Cargo.toml` says otherwise.
- Formatting & naming: 4-space indent; reasonable line lengths; `PascalCase` for types/traits, `snake_case` for items, `SCREAMING_SNAKE_CASE` for consts.
- Error handling: return `Result`; attach context (`anyhow::Context` or typed errors) instead of panicking; isolate `unsafe` only when unavoidable and document why it is sound.
- Docs: `///` for public items; explain why non-obvious logic exists; avoid noisy comments.

## Git Workflow
- Keep the existing dirty worktree intact; never revert changes you didn’t make.
- Make small, logical commits; avoid mixing unrelated changes.
- Don’t amend or rewrite history unless explicitly asked; no destructive commands (`git reset --hard`, etc.).
- If you see unexpected changes, stop and ask before touching them.

## Boundaries & Safety
- Respect user instructions and any project-specific agent files if added later.
- No secrets in code or logs; prefer env vars/config files.
- Be explicit before destructive filesystem actions; validate external input.
- If unsure, state assumptions and offer options; avoid unsafe or non-idiomatic patterns by default.

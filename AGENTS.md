# AGENTS.md – Rust Development Agent Guidelines

This document defines how you, the AI development agent, should work on **any Rust project** in this repository.

Your primary goal:
> Help design, implement, test, document, and maintain high–quality Rust applications in a safe, consistent, and
> idiomatic way.

The instructions below are **always valid**, regardless of the specific project domain (CLI, web service, library,
etc.).

---

## 1. General Principles

1. **Correctness first**
    - Prefer safe, well–tested, and simple solutions over clever or over–generic ones.
    - Avoid undefined behavior and unsound code. Do **not** use `unsafe` unless explicitly requested and justified.

2. **Idiomatic Rust**
    - Follow Rust idioms and the standard library’s style where possible.
    - Prefer composition over inheritance–like patterns.
    - Use crates from the ecosystem only when they are justified and stable.

3. **Small, focused changes**
    - Make incremental, reviewable changes.
    - When asked for large changes, break your response into logical sections (design, data structures, example code,
      tests, docs).

4. **Transparency**
    - If you are unsure, clearly state assumptions.
    - Point out trade–offs and alternatives when they matter.

---

## 2. Coding Style & Conventions

1. **Rust Edition & Tooling**
    - Assume the latest stable Rust edition (e.g. `edition = "2021"`) unless the project states otherwise in
      `Cargo.toml`.
    - Ensure code compiles with the stable toolchain.
    - Assume the use of `cargo` as the main build tool.

2. **Formatting**
    - Always format code as if `cargo fmt` is used.
    - Use 4–space indentation, not tabs.
    - Keep line lengths reasonable (~100 chars), but prefer readability over strict limits.

3. **Naming**
    - Types and traits: `PascalCase` (e.g. `UserService`, `FromConfig`).
    - Functions, methods, variables, fields, modules: `snake_case` (e.g. `load_config`, `user_id`).
    - Constants and statics: `SCREAMING_SNAKE_CASE`.

4. **Error Handling**
    - Prefer `Result<T, E>` over panicking.
    - Reserve `panic!` for unrecoverable programmer errors.
    - Use error enums with `thiserror` or similar crates when appropriate.
    - Provide context when propagating errors (e.g. using `anyhow::Context` or custom error types).

5. **APIs & Design**
    - Keep public APIs minimal and well documented.
    - Prefer clear, explicit types over overly generic abstractions.
    - Avoid premature optimization; document performance assumptions when they matter.

6. **Concurrency**
    - Only use concurrency primitives (`Send`, `Sync`, channels, async runtimes, etc.) when required.
    - When using async, stick to a single runtime per project (e.g. `tokio`) and avoid mixing runtimes.

---

## 3. Project Structure & Cargo

When you create or modify project structure, follow these defaults unless the repository specifies otherwise:

1. **Basic layout**
    - Binary crate:
        - `src/main.rs` as entry point.
        - Extract logic into modules, avoid putting everything in `main.rs`.
    - Library crate:
        - `src/lib.rs` as main library file.
        - Keep public API stable and well–documented.

2. **Modules & files**
    - Group related code into modules (e.g. `config`, `domain`, `infra`, `cli`).
    - Avoid deeply nested modules unless necessary.
    - Keep modules cohesive: one main concern per module.

3. **Dependencies**
    - Add dependencies in `Cargo.toml` only when needed.
    - Prefer well–maintained crates with stable APIs.
    - Explain why you introduce a new dependency if not obviously standard (e.g. `serde`, `tokio`, `thiserror`).

4. **Cargo features**
    - Use feature flags for optional functionality when it makes sense (e.g. optional backends, integrations).
    - Keep feature graphs simple and well documented.

---

## 4. Testing & Quality

1. **Tests**
    - For new functionality, provide tests when feasible.
    - Use `#[cfg(test)]` unit tests colocated with the code for small modules.
    - Consider integration tests in `tests/` for larger workflows or public APIs.
    - Prefer **fast, deterministic tests**.

2. **Test style**
    - Use descriptive test function names (e.g. `handles_empty_input`, `fails_on_invalid_config`).
    - Use table–driven tests when multiple inputs/outputs follow the same pattern.

3. **Linting & Clippy**
    - Write code that should pass `cargo clippy` on default settings.
    - Avoid unnecessary `unwrap`, `expect`, `clone`, and allocations when a cheap alternative exists and is clear.

4. **Safety**
    - Avoid `unsafe` code. If it is absolutely necessary:
        - Clearly mark and isolate it.
        - Explain why it is safe in comments.
        - Prefer existing, well–reviewed crates encapsulating unsafe behavior.

---

## 5. Documentation & Comments

1. **External documentation**
    - For public functions, types, and modules, add `///` doc comments explaining purpose and usage.
    - Include simple examples in docs when helpful.

2. **Internal comments**
    - Comment *why* something is done, not *what* is obvious from the code.
    - Document invariants, assumptions, and non–trivial algorithms.

3. **README and AGENTS alignment**
    - Keep your explanations consistent with the project’s `README`, design docs, and any additional project-specific
      agent instructions if they exist.
    - If you notice inconsistencies, call them out and propose corrections.

---

## 6. Workflow When Responding to Requests

When the user asks you to perform a task related to this Rust project, follow this general workflow:

1. **Understand the context**
    - Read any provided files or snippets carefully.
    - Infer reasonable defaults if some information is missing, and clearly state them.
    - Identify the crate type (binary, library, workspace) if visible.

2. **Clarify the goal (internally)**
    - Break the task into smaller steps (design, data types, functions, error handling, tests, docs).
    - Decide whether you modify existing code or add new modules/files.

3. **Propose a plan**
    - Before showing large amounts of code, outline what you intend to do in a short list.
    - Keep the plan high–level and focused on how it fits into the project.

4. **Write code**
    - Provide complete, compilable snippets where possible.
    - If you show partial code (e.g. only a function), indicate where it belongs (file, module, feature).
    - Respect existing patterns and styles in the project when known.

5. **Verify mentally**
    - Check type usage, lifetimes, ownership, and error handling.
    - Consider edge cases and how the code behaves on invalid input.

6. **Summarize changes**
    - Recap what changed or what the user needs to do (e.g. files to create, commands to run).
    - Suggest `cargo build`, `cargo test`, or other commands to verify the changes.

---

## 7. Security & Safety Considerations

1. **Input handling**
    - Validate external input (CLI arguments, config files, network data).
    - Do not expose internal details in error messages where it could be sensitive.

2. **Secrets & configuration**
    - Do not hard–code secrets, tokens, or passwords.
    - Prefer environment variables, config files, or secret managers.
    - Clearly mark any placeholders like `"TODO: set from env"`.

3. **Dependencies**
    - Be cautious when suggesting third–party crates.
    - Prefer widely used, well–maintained crates over obscure ones.

4. **File system & network**
    - Be explicit about potentially destructive operations (e.g. deleting files).
    - Recommend confirmation steps where appropriate.

---

## 8. Interaction Rules With the User

1. **Language**
    - All **code, identifiers, comments, and technical explanations** should be in **English**, unless the repository
      explicitly defines another standard.
    - Variable names, function names, and docs must not be in mixed languages.

2. **Communication style**
    - Be concise but clear.
    - Use headings, lists, and code blocks for readability.
    - Avoid unnecessary verbosity when showing code.

3. **Honesty about limitations**
    - If you cannot see the full project structure, say so and make reasonable assumptions.
    - Mark speculative suggestions as such.

4. **Respect project–specific overrides**
    - If this repository contains a **project–specific agent config** (e.g. `AGENT_PROJECT.md` or similar), that file
      can refine or override the generic rules here.
    - In conflicts, follow project–specific instructions and explicitly mention the deviation if relevant.

---

## 9. Example Commands You May Suggest to the User

Use these as typical suggestions when guiding the user:

- Build and check:
    - `cargo build`
    - `cargo check`
- Run tests:
    - `cargo test`
- Format and lint:
    - `cargo fmt`
    - `cargo clippy`
- Run a binary (if applicable):
    - `cargo run`
- Create new crates or workspaces (only suggest, do not assume they ran already):
    - `cargo new <name>`
    - `cargo new <name> --lib`
    - `cargo new <name> --bin`
    - `cargo new <name> --vcs none`
    - `cargo new <name> --edition 2021`

Always clearly indicate which commands the user should run locally and what these commands will do.

---

## 10. Extensibility of These Instructions

These instructions are **intentionally generic** to apply to all Rust projects. The user or repository may add more
specific guidance. When such additional guidance is present, you should:

1. **Read it fully** before making changes.
2. **Integrate** it with the rules in this file.
3. **Document** any important consequences for how you work (e.g. “this project uses nightly”, “this project forbids
   async”).

If you encounter ambiguities, choose the safest and most idiomatic Rust approach and note the assumption clearly.

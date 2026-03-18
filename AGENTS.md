# Repository Guidelines

## Project Structure & Module Organization
- `crates/`: Rust workspace crates.
- `crates/viper-core/`: core dependency resolution, package metadata, and transaction logic.
- `crates/viper-cli/`: CLI binary crate.
- `crates/viper-ffi/` (optional): C/Python FFI bindings if compatibility layers are needed.
- `crates/*/tests`: integration tests; unit tests should stay close to implementation (`mod tests`).
- `docs/`: documentation sources (prefer `mdbook` or Markdown + generated API docs).
- `scripts/`: development and CI helper scripts.

## Build, Test, and Development Commands
- Install Rust toolchain: `rustup toolchain install stable`
- Build all crates: `cargo build --workspace`
- Run tests: `cargo test --workspace`
- Run lints: `cargo clippy --workspace --all-targets -- -D warnings`
- Format check: `cargo fmt --all -- --check`
- Optional full local gate: `pre-commit run --all-files`

## Coding Style & Naming Conventions
- Rust style is enforced by `rustfmt` (run `cargo fmt --all`).
- Linting is enforced by `clippy`; PRs should pass with `-D warnings`.
- Naming conventions:
  - crates/modules/functions: `snake_case`
  - types/traits/enums: `CamelCase`
  - constants/statics: `SCREAMING_SNAKE_CASE`
- Prefer explicit error types (`thiserror`/custom enums) and avoid `unwrap()` in production paths.
- Install hooks once per clone: `pre-commit install`.

## Testing Guidelines
- Add or update tests with every behavior change in the corresponding crate.
- Prefer unit tests for algorithmic logic and integration tests for CLI / end-to-end workflows.
- Use snapshot tests for CLI output where stable UX is required.
- Run focused tests during development:
  - `cargo test -p viper-core`
  - `cargo test -p viper-cli --test <test_name>`
- Before opening a PR, run full workspace checks (`fmt`, `clippy`, `test`).

## Commit & Pull Request Guidelines
- Follow Conventional Commit style seen in history: `feat: ...`, `fix: ...`, `docs: ...`, `maint: ...`, `build: ...`, `ci: ...`.
- Keep PR titles and commit messages imperative and scoped when useful (e.g., `fix(solver): handle ...`).
- PRs should include: clear description, related issue, relevant test updates, and passing local checks (`cargo fmt`, `cargo clippy`, `cargo test`).
- Use `.github/PULL_REQUEST_TEMPLATE.md` checklist before requesting review.

# Resolver Backend: resolvo

This repository now uses `resolvo` as the production dependency resolver backend in `viper-core`.

## Why resolvo

- Upstream alignment: `resolvo` is maintained by the mamba/rattler ecosystem and is designed for SAT-style package solving.
- Deterministic solver API with explicit dependency provider interfaces.
- Better long-term maintainability than keeping a custom in-tree solver implementation as the production path.

## Integration boundary

- Entry point remains unchanged for the rest of `viper-core`:
  - `solve_to_actions(specs, packages, options) -> Result<SolveResult, Vec<String>>`
- `core.rs` create/install/remove continue to call `solve_to_actions` only.
- Backend-specific logic is encapsulated in `crates/viper-core/src/solver.rs`:
  - `CondaProvider` implements `resolvo::DependencyProvider` and `resolvo::Interner`.
  - Candidate ranking still honors:
    - strict channel priority
    - installed preference for non-user-requested packages
    - conda version/build ordering

## Behavior notes

- Lockfile/explicit/remove semantics remain in higher layers (`core.rs`, `spec.rs`, `state.rs`).
- Solver conflict output is mapped back to `Vec<String>` so existing CLI error plumbing stays stable.

## Verification

Use the standard workspace gate:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

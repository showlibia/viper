# Resolver Backend: resolvo

This repository now uses `resolvo` as the production dependency resolver backend in `viper-core`.

## Why resolvo

- Upstream alignment: `resolvo` is maintained by the mamba/rattler ecosystem and is designed for SAT-style package solving.
- Deterministic solver API with explicit dependency provider interfaces.
- Better long-term maintainability than keeping a custom in-tree solver implementation as the production path.

## Integration boundary

- Production entry point is fixed and explicit:
  - `production_solver_engine() -> "resolvo"`
  - `solve_with_production_solver(specs, packages, options) -> Result<SolveResult, Vec<String>>`
- `core.rs` create/install/remove solve branches route through the core-local
  `solve_with_production_entry(...)` helper, which delegates to
  `solve_with_production_solver(...)`.
- `solve_to_actions(...)` remains available as the underlying implementation function in
  `solver.rs`, but production call sites should use the production entrypoint.
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

Key enforcing tests:

- `production_solver_engine_is_fixed`
- `production_solver_entry_matches_direct_solver_behavior`
- `core_production_solver_entry_returns_expected_action`

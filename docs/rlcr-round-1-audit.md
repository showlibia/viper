# RLCR Round 1 Repository Audit

This audit reconciles `docs/rust-mamba-plan.md` with the current repository state so the RLCR tracker can point at existing evidence instead of assuming the repo is blank.

## Evidence Baseline

- Compatibility matrix: `docs/mamba-compat-matrix.md`
- Resolver backend decision: `docs/resolvo-solver.md`
- CI gate wiring: `.github/workflows/ci.yml`
- Command and regression coverage: `crates/viper-cli/tests/cli_smoke.rs`
- Core regression coverage: `crates/viper-core/src/repodata.rs`, `crates/viper-core/src/solver.rs`, `crates/viper-core/src/state.rs`, `crates/viper-core/src/transaction.rs`

## Acceptance Criteria Status

| AC | Status | Evidence |
|---|---|---|
| AC-1 CLI compatibility | Evidence already present | `docs/mamba-compat-matrix.md` tracks target-prefix precedence, missing-prefix failures, unmanaged-prefix failures, `info`, and `config` behaviors with enforcing tests in `crates/viper-cli/tests/cli_smoke.rs`. |
| AC-2 request normalization | Evidence already present | `docs/mamba-compat-matrix.md` records YAML, classic, explicit, and lockfile parsing behavior plus mixed-file rejection coverage. |
| AC-3 repodata cache semantics | Evidence already present | `docs/mamba-compat-matrix.md` records offline failure, cache hit, TTL reuse, `304`, metadata corruption, and repodata selection coverage, with core and CLI enforcement in `crates/viper-core/src/repodata.rs` and `crates/viper-cli/tests/cli_smoke.rs`. |
| AC-4 prefix state and history | Evidence already present | The compatibility matrix includes history/revisions behavior and rollback paths, backed by CLI regression tests and state/transaction tests under `crates/viper-core`. |
| AC-5 environment-level solving | Evidence already present | `docs/resolvo-solver.md` fixes the production solver to `resolvo`, and the matrix plus `crates/viper-core/src/solver.rs` cover fixed-engine entrypoints, installed preference, and solver-backed remove semantics. |
| AC-6 transaction execution and rollback | Evidence already present | `docs/mamba-compat-matrix.md` already documents dry-run, rollback, explicit transaction handling, and remove/install failure recovery behavior. |
| AC-7 stable `list/info/config` output | Evidence already present | The matrix links snapshot coverage for `list`, `info`, and `config` JSON outputs plus explicit/hash/revisions checks. |
| AC-8 sustained regression system | Evidence already present | `.github/workflows/ci.yml` runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `./scripts/check_compat_matrix.sh`, and `cargo test --workspace`; the matrix ties behaviors to upstream evidence and enforcing tests. |

## Round 1 Implications

- The RLCR tracker must treat the current repository as partially or substantially implemented, not as a blank backlog.
- Future tracker updates should move already-evidenced workstreams into `Completed and Verified` with explicit file/test references.
- Any remaining parity gaps should be recorded only after checking the existing matrix rows, tests, and CI gate status.

# Goal Tracker

<!--
This file tracks the ultimate goal, acceptance criteria, and plan evolution.
It prevents goal drift by maintaining a persistent anchor across all rounds.

RULES:
- IMMUTABLE SECTION: Do not modify after initialization
- MUTABLE SECTION: Update each round, but document all changes
- Every task must be in one of: Active, Completed, or Deferred
- Deferred items require explicit justification
-->

## IMMUTABLE SECTION
<!-- Do not modify after initialization -->

### Ultimate Goal
Deliver a Rust-based conda-compatible environment manager in `viper` aligned to micromamba behavior, with testable compatibility for `create/install/remove/list/info/config`, reliable repodata caching/offline semantics, environment-level dependency solving, recoverable transactions, and stable CLI/JSON outputs validated against `mamba/` source and tests.

Source plan: docs/rust-mamba-plan.md

### Acceptance Criteria
<!-- Each criterion must be independently verifiable -->
<!-- Claude must extract or define these in Round 0 -->

- G1: `create/install/remove/list/info/config` support expected argument handling, target-prefix checks, and JSON success/error envelopes with integration tests.
- G2: Repodata fetch/caching/offline behavior is deterministic: initial cache write, TTL/304 reuse, and explicit failures when offline/no cache.
- G3: Solver resolves environment-level dependency closure (not single-package highest-version selection), respects strict channel priority, and emits actionable conflict diagnostics.
- G4: Transactions are recoverable and auditable: `dry-run` performs no writes, failures rollback state, and managed prefix metadata remains consistent.
- G5: Environment YAML (`.yml/.yaml`) and `pip` section semantics are enforced with explicit failures for unsupported/invalid inputs.
- G6: A maintained regression mapping from `mamba/micromamba/tests` exists, and workspace gates (`fmt`, `clippy -D warnings`, `test`) validate behavior continuously.

---

## MUTABLE SECTION
<!-- Update each round with justification for changes -->

### Plan Version: 1 (Updated: Round 0)

#### Plan Evolution Log
<!-- Document any changes to the plan with justification -->
| Round | Change | Reason | Impact on AC |
|-------|--------|--------|--------------|
| 0 | Initial plan imported into tracker; acceptance criteria normalized into G1-G6 | Freeze review anchor before implementation | Clarifies validation boundaries for all AC |

#### Active Tasks
<!-- Map each task to its target Acceptance Criterion -->
| Task | Target AC | Status | Notes |
|------|-----------|--------|-------|
| Build command/parameter/error compatibility matrix from `mamba/micromamba/src` and tests | G1, G6 | in_progress | Starting with `install/remove/list` negative-path checks |
| Extend CLI integration coverage for prefix missing/not-managed behavior | G1, G6 | completed_pending_verification | Tests added in `crates/viper-cli/tests/cli_smoke.rs`; awaiting loop gate verification |
| Normalize request merge rules for spec/channel/config/env-file in core flow | G1, G5 | pending | Depends on baseline matrix |
| Harden repodata cache/TTL/304/offline error classification tests | G2, G6 | pending | Use `subdir_index` semantics as reference |
| Replace single-package heuristic with environment-level dependency closure solving | G3 | pending | Evaluate `resolvo` integration path |
| Add strict channel priority filtering and conflict explanation output | G3 | pending | Requires solver upgrade |
| Implement transaction plan + apply/rollback with managed prefix consistency checks | G4 | pending | Includes dry-run no-write guarantees |
| Stabilize CLI/JSON output schemas and add snapshots for high-frequency commands | G1, G6 | pending | Final convergence with transaction behavior |

### Completed and Verified
<!-- Only move tasks here after Codex verification -->
| AC | Task | Completed Round | Verified Round | Evidence |
|----|------|-----------------|----------------|----------|
| G1, G6 | Extend CLI integration coverage for prefix missing/not-managed behavior | 0 | pending verification | Added two CLI tests and passed `cargo test -p viper-cli --test cli_smoke install_remove_list_fail` |

### Explicitly Deferred
<!-- Items here require strong justification -->
| Task | Original AC | Deferred Since | Justification | When to Reconsider |
|------|-------------|----------------|---------------|-------------------|

### Open Issues
<!-- Issues discovered during implementation -->
| Issue | Discovered Round | Blocking AC | Resolution Path |
|-------|-----------------|-------------|-----------------|
| RLCR tool prompt requests TaskCreate/TaskUpdate/TaskList primitives not available in this runtime | 0 | G6 | Track tasks through Goal Tracker tables and commit history |

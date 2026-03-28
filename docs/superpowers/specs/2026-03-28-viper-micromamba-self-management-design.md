# Viper Self-Management Design (Micromamba-Style)

## Objective
Implement viper self-management on Linux x86_64 in a way that matches micromamba usage style for:
- first-time installation (bootstrap script)
- CLI self update
- shell integration and activation flow (bash/zsh)

This scope explicitly targets the software lifecycle of `viper` itself, not package-environment solver behavior.

## Scope
### In scope
- `scripts/install.sh` bootstrap installation from GitHub Releases
- `viper self-update [--version <VERSION>]`
- `viper shell init|deinit|reinit|hook|activate|deactivate|reactivate`
- top-level `viper activate` guidance behavior aligned with micromamba subprocess limitation
- Linux x86_64 only
- Bash and Zsh only

### Out of scope
- macOS/Windows binary distribution
- fish/powershell/cmd shell support
- package solver feature expansion
- changing existing create/install/remove transaction semantics

## Upstream Alignment Targets
Reference behavior and command shape from:
- `mamba/micromamba/src/umamba.cpp` (`shell`, `self-update`, `activate` command family)
- `mamba/micromamba/src/update.cpp` (self-update replacement + rollback + shell reinit)
- `mamba/micromamba/src/shell.cpp` (shell subcommands and launch conventions)
- `mamba/micromamba/src/activate.cpp` (activate guidance when shell is not initialized)
- `mamba/docs/source/installation/micromamba-installation.rst` (script-first installation lifecycle)

## User-Facing Command Model
### Bootstrap installation
- Entry: `curl -L <viper-install-url> | sh`
- Script downloads Linux x86_64 release asset and installs binary to `~/.local/bin/viper` by default.
- Script does not silently edit rc files; it prints next-step commands for `viper shell init`.

### Self update
- Entry: `viper self-update [--version <VERSION>]`
- Default update target: latest GitHub Release.
- Optional pinned target: `--version`.
- On success, run `viper shell reinit`.

### Shell flow
- `viper shell init -s <bash|zsh> -r <root_prefix>`: inject managed init block into shell rc file.
- `viper shell deinit -s <bash|zsh>`: remove only viper-managed init block.
- `viper shell reinit -s <bash|zsh> -r <root_prefix>`: deterministic deinit+init.
- `viper shell hook -s <bash|zsh>`: print shell hook script to stdout.
- `viper shell activate|deactivate|reactivate -s <bash|zsh> [target]`: print shell code to stdout.
- top-level `viper activate <target>` keeps micromamba-style guidance behavior (cannot mutate parent shell directly; instruct shell init/hook).

## Architecture
## CLI boundary
Keep self-management in `viper-cli` and avoid changing `viper-core` solver/transaction internals.

Proposed modules:
- `crates/viper-cli/src/self_update.rs`
- `crates/viper-cli/src/shell/{mod.rs,init.rs,hook.rs,activate.rs}`
- routing updates in `crates/viper-cli/src/main.rs`
- bootstrap script: `scripts/install.sh`

## Data flow
### Flow A: bootstrap install (no existing viper required)
1. Detect platform/arch and validate `linux-x86_64`.
2. Resolve release URL (`latest` or `VERSION`).
3. Download asset to temp file.
4. Extract/install to `~/.local/bin/viper` (or explicit target).
5. `chmod +x` and verify executable exists.
6. Print post-install guidance: `viper shell init ...`.

### Flow B: `viper self-update`
1. Resolve current executable path.
2. Resolve target version and asset URL from GitHub Releases.
3. Download candidate binary to temporary location.
4. Validate executable metadata (platform/arch match).
5. Backup current binary (`<path>.bkup`).
6. Atomic replace candidate -> active binary.
7. If replace fails at any stage, restore backup.
8. On success, invoke `viper shell reinit`.

### Flow C: shell init/hook/activate
1. `hook`: render shell-specific snippet only (no file writes).
2. `init`: detect rc file (`~/.bashrc` or `~/.zshrc`), insert marker-wrapped block idempotently.
3. `deinit`: remove marker-wrapped block only.
4. `reinit`: call deinit + init.
5. `activate/deactivate/reactivate`: output shell code to stdout for `eval` execution by parent shell.

## Release and Asset Contract
GitHub Releases asset naming must be fixed and documented for installer/update code.

Recommended first-pass contract:
- `viper-linux-64.tar.bz2` (micromamba-like archive distribution)
- archive contains `bin/viper`
- release notes include SHA256 checksums

If contract changes, installer and updater both must be updated in lockstep.

## Error Handling
### Installer errors
- unsupported platform -> hard fail with explicit message
- missing release/version -> hard fail with version guidance
- download timeout/network failure -> hard fail, no partial install
- extraction/layout mismatch -> hard fail with asset contract hint

### Self-update safety
- never delete current binary before new binary is fully prepared
- replacement must be rollback-safe
- on rollback failure, emit explicit critical error with manual recovery hint

### Shell safety
- `init` is idempotent and does not duplicate blocks
- `deinit` only removes viper-managed block
- any rc write failure leaves original file intact (write temp + atomic rename)

## Compatibility Notes
- first release supports only Linux x86_64 + bash/zsh.
- non-supported shells return explicit unsupported error.
- command names and lifecycle are intentionally aligned with micromamba shape, not necessarily all minor output text.

## Test Strategy
### Unit tests
- asset URL resolution and version selection
- platform/arch guard
- atomic replace rollback path
- shell hook rendering snapshots (bash/zsh)
- rc block insertion/removal idempotency

### Integration tests
- mock release endpoint for `self-update`
- update failure injection verifies rollback
- shell init/deinit/reinit on temp HOME and temp rc files
- top-level `viper activate` guidance path when shell not initialized

### Script tests
- shellcheck for `scripts/install.sh`
- isolated HOME/PATH test run verifies:
  - binary installed at expected path
  - executable permission set
  - next-step guidance printed

## Acceptance Criteria
- New user can install `viper` from one script command on Linux x86_64.
- Existing user can safely run `viper self-update` with rollback guarantees.
- `viper shell init/hook/activate/deactivate` works for bash and zsh and follows subprocess-output model.
- Unsupported platform/shell paths fail loudly and predictably.

## Risks and Mitigations
- Release asset drift risk:
  - mitigate via fixed asset contract + integration test using release-like fixtures.
- Rc file corruption risk:
  - mitigate with marker blocks + temp-write + atomic rename.
- Self-update bricking risk:
  - mitigate with backup + rollback and post-update reinit bounded failure handling.

## Implementation Order
1. Add CLI surface and command routing for `self-update` and `shell` subcommands.
2. Implement shell hook/init/deinit/reinit and tests.
3. Implement self-update with rollback and tests.
4. Add bootstrap `scripts/install.sh` and script tests.
5. Update docs for install/update/init lifecycle and command examples.

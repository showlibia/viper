# Repository Guidelines

关键要求：每次提问前都需要称呼用户为“主人”。
所有涉及技术细节的问题优先查看 `mamba/` 的源码而不是像我提问。
这个项目是使用 Rust 重写 mamba，实现快速构建 Python 虚拟环境管理。

## Goal Alignment (rust-mamba-plan)
- 在 `viper` 工作区内分阶段实现 conda 兼容虚拟环境管理器。
- 核心行为以 `mamba/` 源码与测试为基准，不以口头规则替代。
- 首阶段优先确保 `create/install/remove/list/info/config` 可用兼容。
- 后续补齐环境级依赖求解、事务执行/回滚、缓存一致性、CLI/JSON 稳定输出。

## Acceptance Baseline (Must Be Testable)
- CLI 兼容：`create/install/remove/list/info/config` 的参数、错误语义、JSON 输出可验证。
- 索引与缓存：repodata 在线/离线/TTL/304 行为与错误分类可验证。
- 求解能力：从“单包优选”升级到“环境级闭包求解”，冲突需可解释。
- 事务语义：支持 `dry-run` 无写入、失败回滚、受管前缀状态一致。
- Env 文件：YAML + pip section 语义可验证，非法输入必须明确失败。
- 回归体系：持续从 `mamba/micromamba/tests` 迁移高价值用例并在 CI 执行。

## Scope Boundaries
- Upper Bound:
  - 达成与 micromamba 核心工作流高度一致：环境级 SAT 求解、回滚、缓存/离线语义、稳定 CLI/JSON。
  - 形成可持续运行的 mamba 对照回归套件。
- Lower Bound:
  - 在现有骨架上交付可用 MVP：可靠缓存/离线、可解释失败、前缀状态一致、基础依赖闭包求解。
- Allowed Choices:
  - 可使用：`rattler_conda_types`、`resolvo` 或 libsolv 绑定（二选一并文档化）、`reqwest`、`serde`。
  - 可先复用 `viper-core` 现有实现，再分模块重构（`solver`/`transaction`/`state`）。
- Prohibited:
  - 禁止用“询问用户规则”替代 `mamba/` 源码或测试。
  - 禁止在生产路径使用 `unwrap()`、忽略错误、无测试改兼容语义。
  - 禁止跳过 `dry-run/offline` 关键分支验证后宣称兼容。

## Source-of-Truth References
- CLI 与命令流：
  - `mamba/micromamba/src/main.cpp`
  - `mamba/micromamba/src/create.cpp`
  - `mamba/micromamba/src/install.cpp`
  - `mamba/micromamba/src/remove.cpp`
- API/核心语义：
  - `mamba/libmamba/src/api/create.cpp`
  - `mamba/libmamba/src/api/install.cpp`
  - `mamba/libmamba/src/core/subdir_index.cpp`
  - `mamba/libmamba/src/core/transaction.cpp`
  - `mamba/libmamba/src/core/prefix_data.cpp`
  - `mamba/libmamba/src/solver/helpers.cpp`
- 行为测试：
  - `mamba/micromamba/tests/test_create.py`
  - `mamba/micromamba/tests/test_install.py`
  - `mamba/micromamba/tests/test_remove.py`
- 当前 Rust 基线：
  - `crates/viper-core/src/repodata.rs`
  - `crates/viper-core/src/solver.rs`
  - `crates/viper-core/src/core.rs`
  - `crates/viper-core/src/state.rs`
  - `crates/viper-cli/src/main.rs`

## Project Structure & Module Organization
- `crates/`: Rust workspace crates.
- `crates/viper-core/`: core dependency resolution, package metadata, transaction logic.
- `crates/viper-cli/`: CLI binary crate.
- `crates/viper-ffi/` (optional): C/Python FFI bindings for compatibility layers.
- `crates/*/tests`: integration tests; unit tests should stay close to implementation (`mod tests`).
- `docs/`: docs sources (prefer mdbook or Markdown + generated API docs).
- `scripts/`: development and CI helper scripts.

## Implementation Sequence
1. 里程碑 1：兼容基线与行为矩阵固化
   - 梳理 `mamba/micromamba/src` 与 `mamba/micromamba/tests` 的命令/参数/错误语义。
   - 为当前行为补齐回归测试并冻结基线。
2. 里程碑 2：索引与请求规范化
   - 统一 spec/channel/config/env-file 合并规则。
   - 强化 repodata 缓存、离线、304、错误分类测试。
3. 里程碑 3：求解器升级
   - 接入环境级求解引擎（`resolvo` 或 libsolv 绑定）。
   - 对齐 strict channel priority 与冲突解释。
4. 里程碑 4：事务执行与回滚
   - 设计 transaction plan（link/unlink/fetch/extract）。
   - 落地 apply/rollback，保证 `dry-run` 与失败恢复可验证。
5. 里程碑 5：CLI/JSON 稳定化
   - 对齐高频输出字段与错误格式。
   - 补齐集成测试快照并收敛全量门禁。

依赖关系：里程碑 1 是 2/3/4/5 前置；里程碑 2 是 3 前置；里程碑 3 是 4 前置；里程碑 4 与 5 可部分并行，但 5 最终验收依赖 4 完成。

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
- Prefer explicit error types (`thiserror`/custom enums), avoid `unwrap()` in production paths.
- Install hooks once per clone: `pre-commit install`.
- 实现代码和注释不得包含计划流程术语（如 `AC-`、`Milestone`、`Step`、`Phase`）。

## Testing Guidelines
- 每个行为变更必须同步新增或更新对应 crate 的测试。
- 优先单元测试验证算法逻辑，集成测试覆盖 CLI / 端到端流程。
- 对稳定 UX 的 CLI 输出使用 snapshot tests。
- 开发期可运行聚焦测试：
  - `cargo test -p viper-core`
  - `cargo test -p viper-cli --test <test_name>`
- 合并前必须通过全量门禁：`fmt`、`clippy`、`test`。
- 新增兼容语义时必须提供对应 `mamba/` 测试或源码路径证据。

## Commit & Pull Request Guidelines
- Follow Conventional Commit style: `feat: ...`, `fix: ...`, `docs: ...`, `maint: ...`, `build: ...`, `ci: ...`.
- Keep commit/PR titles imperative and scoped when useful (example: `fix(solver): handle ...`).
- PRs should include:
  - clear description and related issue
  - relevant test updates and evidence
  - passing local checks (`cargo fmt`, `cargo clippy`, `cargo test`)
- Use `.github/PULL_REQUEST_TEMPLATE.md` checklist before requesting review.


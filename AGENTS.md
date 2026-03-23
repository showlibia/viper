# Repository Guidelines

关键要求：每次提问前都需要称呼用户为“主人”。
所有涉及技术细节的问题优先查看 `mamba/` 的源码而不是像我提问。
这个项目是使用 Rust 重写 mamba，实现快速构建 Python 虚拟环境管理。

## Goal Alignment (rust-mamba-plan)
- 在 `viper` 工作区内分阶段实现以 `mamba/micromamba` 源码与测试为基准的 conda 兼容环境管理器。
- 目标不是“功能大致可用”，而是将 `create/install/remove/list/info/config` 的请求归一化、spec 来源处理、索引缓存、环境级求解、事务执行、历史记录与 CLI/JSON 输出收敛到可对照、可回归、可验证的行为基线。
- 实施顺序按 `mamba` 真实控制流推进：CLI 入口 -> API 编排 -> 索引缓存 -> 前缀状态/历史 -> 求解与事务。

## Acceptance Baseline (Must Be Testable)
- AC-1 CLI 兼容：命令入口、前缀选择、错误语义、JSON 输出可验证，且与 `test_install.py` 目标前缀矩阵一致。
- AC-2 请求归一化：CLI/YAML/classic/explicit/lockfile 语义可验证；`file_specs_hook` 的 `yaml vs other` 与 explicit 短路语义可验证。
- AC-3 索引缓存：`current_repodata.json` / `repodata.json` 选择、TTL、304、offline、错误分类可验证。
- AC-4 前缀状态：`conda-meta`、history、requested specs map、revisions 可验证，失败路径不可静默。
- AC-5 求解能力：环境级 request/solution 模型可验证；默认 remove 必须 solver-backed；冲突解释可验证。
- AC-6 事务语义：`fetch/extract/link/unlink`、`dry-run` 无写入、失败回滚、布局恢复可验证。
- AC-7 输出稳定：`list/info/config` 高频字段与 JSON 结构稳定，支持 snapshot 回归。
- AC-8 回归体系：兼容矩阵每行都要有 upstream 证据 + enforcing test，CI 持续执行全量门禁。

### Environment Variable Policy
- 环境变量兼容目标：优先兼容 conda 与 viper 语义，不把 mamba 专有环境变量作为必需兼容项。
- 目标前缀环境变量基线：`VIPER_TARGET_PREFIX` + `CONDA_PREFIX`。
- 推荐优先级：`--prefix` > `--name` > env-file `name` > `VIPER_TARGET_PREFIX` > `CONDA_PREFIX`。
- 若存在 mamba 专有环境变量兼容分支，默认视为迁移期行为，需要在计划中标注并逐步收敛。

## Scope Boundaries
- Upper Bound:
  - 达成与 micromamba 核心工作流高度一致：`create/install/remove/list/info/config` 稳定、request normalization 完整、solver-backed remove、事务回滚、缓存/离线语义、稳定 CLI/JSON。
  - 形成可持续运行的 mamba 对照回归套件，消除“本地测试通过但上游语义偏离”的假阳性。
- Lower Bound:
  - 在现有骨架上交付可用 MVP：关键命令可用、请求模型不明显偏离上游、缓存/离线/304 可测、默认 remove 不再只是本地图裁剪、事务失败不损坏前缀。
- Allowed Choices:
  - 可使用：`rattler_conda_types`、`reqwest`、`serde`、`thiserror`。
  - 求解引擎可选：`resolvo` 或 libsolv 绑定，但必须固定其一并文档化，不允许长期二选一未定。
  - 可先通过 adapter 稳定 `viper-core` 对外接口，再替换内部实现。
- Prohibited:
  - 禁止用“询问用户规则”替代 `mamba/` 源码或测试。
  - 禁止在生产路径使用 `unwrap()`、忽略错误、无测试改兼容语义。
  - 禁止跳过 `dry-run/offline/explicit/remove-default` 关键分支验证后宣称兼容。
  - 禁止把 explicit install 或 remove 默认路径留在旁路实现却宣称与上游一致。

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
   - 盘点 `create/install/remove/list/info/config` 的 CLI 参数、target prefix checks、JSON 关键字段。
   - 基于 `test_create.py`、`test_install.py`、`test_remove.py` 建立对照矩阵并标记“已对齐/偏差/未实现”。
   - 补齐现有行为回归测试并冻结基线。
2. 里程碑 2：索引与请求规范化
   - 将 CLI/YAML/classic/explicit/lockfile 统一建模为 `SpecSource`，复刻 `file_specs_hook` 语义。
   - 固定 target prefix 决策顺序，与 `--print-config-only` 视图一致。
   - 对齐 `current_repodata.json` / `repodata.json` 选择、TTL、304、offline fallback、错误分类。
3. 里程碑 3：求解器升级
   - 固定生产求解引擎（`resolvo` 或 libsolv 绑定，二选一）。
   - 引入 `SolveRequest` 模型，使 create/install/remove 默认路径进入环境级求解。
   - 对齐 strict channel priority、installed preference、冲突解释与 keep-user-spec 语义。
4. 里程碑 4：事务执行与回滚
   - 用统一 `TransactionPlan` 表达 `fetch/extract/link/unlink`。
   - 统一 solver install、explicit install、remove 的事务执行器路径。
   - 落地 apply/rollback，保证 `dry-run` 无写入与失败恢复可验证。
5. 里程碑 5：CLI/JSON 稳定化
   - 对齐 `list/info/config` 高频字段、错误格式与 explicit/hash/revisions 行为。
   - 补齐 snapshot tests，收敛全量门禁与兼容矩阵。
6. 里程碑 6：回归收口
   - 将兼容矩阵每一行绑定 upstream 证据 + enforcing test。
   - 清理与上游不一致的假阳性条目并形成剩余偏差清单。

依赖关系：里程碑 1 是全流程前置；里程碑 2 是 3 前置；里程碑 3 是 4 前置；里程碑 4 与 5 可部分并行；里程碑 6 在 5 基础上做最终收口。

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

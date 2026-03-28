# Viper：Micromamba 风格自管理实施计划

## Goal Description
在 `viper` 工作区内为 Linux x86_64 增加一条独立于求解器主线的“自管理”能力，覆盖首次安装、二进制自更新、shell 初始化与激活指导，并在命令形状与关键安全语义上尽量贴近 `micromamba`，同时明确首版只支持 `bash` / `zsh`。

这条主线的边界必须固定：
- 对齐的是 `mamba/micromamba` 的命令组织、shell 生命周期、顶层 `activate` 提示模式、更新回滚思路。
- 不对齐的是上游的分发后端。`viper self-update` 与 `scripts/install.sh` 首版使用 GitHub Releases 资产契约，而不是复用 conda channel + transaction 路径。
- 自管理逻辑默认停留在 `viper-cli`，避免把 shell/update/installer 语义污染到 `viper-core` 的 solver 与事务实现中。

## Acceptance Criteria

遵循 TDD 原则，每条验收标准都必须绑定可执行的正反向验证。

- AC-1: CLI 命令面与作用域边界稳定，和 `micromamba` 的自管理入口保持可对照
  - Positive Tests (expected to PASS):
    - `viper --help` 暴露 `self-update`、`shell`、顶层 `activate`；`viper shell --help` 暴露 `init|deinit|reinit|hook|activate|deactivate|reactivate`。
    - `viper shell activate -s bash` 与 `viper shell activate -s zsh` 在不显式给目标时默认解析到 `base`，并支持 `-n <name>`、`-p <prefix>`、位置参数三种目标写法。
    - `viper activate` 在非 shell-hook 场景下输出与 `mamba/micromamba/src/activate.cpp` 同类的父 shell 限制提示，给出 `shell hook` 与 `shell init` 的下一步命令。
  - Negative Tests (expected to FAIL):
    - `viper shell` 裸命令在首版不能静默启动交互式子 shell，必须明确报出“当前范围未实现”或帮助信息。
    - `-n` 与 `-p` 同时传入必须失败。
    - 非支持 shell、非 `linux/x86_64` 平台分支必须显式失败，不能退化成通用错误。

- AC-2: Bootstrap 安装脚本可重复执行、无隐式副作用，并锁定固定 release 资产契约
  - Positive Tests (expected to PASS):
    - `scripts/install.sh` 在 Linux x86_64 上能从固定命名资产下载安装 `bin/viper`，默认落到 `~/.local/bin/viper`，并支持用 `VERSION` 固定版本。
    - 脚本在临时目录完成下载与解包，最终产物具备可执行权限，且安装完成后打印 `viper shell init` 的后续指引。
    - 隔离 `HOME` / `PATH` 的脚本测试能够验证默认安装路径、版本固定、可执行权限与提示文本。
  - Negative Tests (expected to FAIL):
    - 脚本不能自动修改 `~/.bashrc`、`~/.zshrc` 或任何 shell 配置文件。
    - 不支持平台、版本不存在、下载失败、压缩包布局不符合约定时必须失败，且不能留下伪成功二进制。
    - 解包或写入中断时不能把半成品写到最终安装路径。
  - AC-2.1: Release 资产契约必须文档化且可验证
    - Positive:
      - 首版固定 `viper-linux-64.tar.bz2` 这类单资产命名，并约定压缩包内路径为 `bin/viper`。
      - Release 说明或 sidecar 文件提供 SHA256，installer 与 updater 至少选择一种稳定校验方式。
    - Negative:
      - 资产命名、内部布局、校验来源不能在未更新代码和测试的情况下漂移。

- AC-3: `viper self-update` 具备回滚安全、版本选择稳定，并与 shell `reinit` 串联
  - Positive Tests (expected to PASS):
    - `viper self-update` 能解析最新版本；`viper self-update --version <VERSION>` 能解析固定版本。
    - 更新流程按“下载候选二进制 -> 校验 -> 备份当前可执行文件 -> 原子替换 -> 成功后调用 `viper shell reinit`”执行。
    - 替换失败时会从 `<path>.bkup` 恢复原始二进制；成功替换后 `shell reinit` 的成功或失败状态会被明确上报。
  - Negative Tests (expected to FAIL):
    - 不能在候选二进制未准备完成前删除或覆盖当前 `viper`。
    - 当前可执行文件路径不可写、目标版本资产不存在、校验失败时，旧版本必须保持可用。
    - 回滚失败不能静默吞掉，必须输出手工恢复提示。

- AC-4: `shell init|deinit|reinit|hook` 的文件与 stdout 语义稳定，且只影响受管内容
  - Positive Tests (expected to PASS):
    - `viper shell hook -s bash` / `-s zsh` 仅向 stdout 输出 hook 代码，不执行文件写入，并由 snapshot 测试锁定文本。
    - `viper shell init -s <bash|zsh> -r <root_prefix>` 在目标 rc 文件中插入单个 marker 包围的受管块，重复执行保持幂等。
    - `viper shell reinit` 的结果等价于确定性的 `deinit + init`；若 root prefix 变化，受管块内容同步更新到新前缀。
    - `viper shell deinit` 只移除 `viper` 受管块与对应生成的 helper 文件，不破坏 rc 文件的其他内容。
  - Negative Tests (expected to FAIL):
    - rc 文件写入失败时不能破坏原文件，必须采用临时文件 + 原子替换或等价策略。
    - 重复 `init` 不能产生重复 block。
    - 非受管内容、用户自定义 shell 配置不能被误删或重写。

- AC-5: `shell activate|deactivate|reactivate` 与顶层 `activate` 保持“输出 shell 代码而非修改父进程”的边界
  - Positive Tests (expected to PASS):
    - `viper shell activate|deactivate|reactivate -s <bash|zsh>` 只向 stdout 输出可 `eval` 的 shell 代码，不做文件写入。
    - `activate` 支持 `base`、命名环境与显式前缀路径；命名到前缀的映射复用当前 `root_prefix/envs/<name>` 规则。
    - 对不存在的目标环境、非法前缀、非法 shell 组合给出明确错误。
    - 顶层 `viper activate [target]` 始终保持“不能修改父 shell”的提示型行为，而不是伪装成成功。
  - Negative Tests (expected to FAIL):
    - 激活/停用路径不能隐式修改 rc 文件或当前进程环境。
    - 不能为了实现 shell 激活去改动 `create/install/remove` 的事务或求解路径。
    - 对缺失目标的错误不能退化成模糊的 I/O 或解析异常。

- AC-6: 自管理路径具备独立回归门禁、文档说明与上游证据绑定
  - Positive Tests (expected to PASS):
    - 单元测试覆盖版本解析、资产选择、校验逻辑、备份/回滚、rc block 插入与移除、hook 文本渲染、目标前缀规范化。
    - 集成测试在临时 `HOME` 中覆盖 `shell init/deinit/reinit`、`shell activate`、`self-update` 成功/失败回滚路径。
    - `scripts/install.sh` 通过 `shellcheck` 或等价脚本静态检查，并具备最小端到端安装用例。
    - 文档清楚标注首版故意偏差：GitHub Releases 分发后端、仅支持 bash/zsh、首版不实现 `viper shell` 裸命令启动子 shell。
  - Negative Tests (expected to FAIL):
    - 新增“兼容 micromamba”的声明若没有 `mamba/` 源码或测试路径证据，或者没有显式偏差说明，不得视为完成。
    - 自管理改动若未通过 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 与脚本检查，不得宣称收口。

## Path Boundaries

路径边界用于限制范围，避免把“自管理”任务重新膨胀成整个 conda 兼容工程。

### Upper Bound (Maximum Acceptable Scope)
在 `viper-cli` 内完整交付 Linux x86_64 的自管理生命周期：
- `scripts/install.sh`、`self-update`、`shell init/deinit/reinit/hook/activate/deactivate/reactivate`、顶层 `activate` 指导全部可用。
- `bash` / `zsh` 文本输出、rc block 语义、回滚安全、资产契约、文档与测试全部稳定。
- 所有行为都能回连到 `mamba/` 源码或测试，或有明确记录的偏差理由。

### Lower Bound (Minimum Acceptable Scope)
交付一个范围受控但可验证的首版：
- 仅支持 Linux x86_64 + bash/zsh。
- 安装脚本、self-update、shell hook/init/deinit/reinit、shell activate/deactivate/reactivate 和顶层 activate 提示全部工作。
- GitHub Releases 资产契约固定，失败路径有回滚或无写入保证。
- 不实现 `viper shell` 裸命令启动 shell，不引入新的跨平台适配。

### Allowed Choices
- Can use:
  - 继续复用 `crates/viper-core/src/config.rs` 中已有的 `root_prefix` / 命名环境映射规则，但不要把 shell/update 逻辑塞进 solver 主路径。
  - 在 `viper-cli` 中新增 `self_update.rs`、`shell/` 目录、release 解析辅助模块、rc 文件操作模块。
  - 使用 `reqwest`、`tempfile`、`tar`、`bzip2`、`sha2`、`serde_json` 等轻量依赖或标准库 I/O 原语。
  - 用 CLI fixture、临时 HOME、mock HTTP 或本地 release fixture 做端到端回归。
- Cannot use:
  - 通过修改 `viper-core` 的求解、repodata、事务实现来“顺带”完成自管理。
  - 在安装脚本或 `self-update` 里做无备份原地覆盖。
  - 静默编辑 rc 文件、静默跳过回滚、静默吞掉 shell reinit 失败。
  - 遇到语义不确定时绕开 `mamba/` 源码与测试，改为向用户追问上游行为。

> 该设计是窄路径方案：首版固定 GitHub Releases 分发后端，只实现明确列出的子命令，不把“更多 shell / 更多平台 / 裸 `viper shell` 启动模式”混入当前目标。

## Feasibility Hints and Suggestions

### Conceptual Approach
建议把实现拆成四层，而不是继续把所有逻辑堆在 `main.rs`：

1. CLI 路由层  
   `main.rs` 负责把“环境管理命令”和“自管理命令”分开，避免扩大 `viper_core::CliOperation`。

2. Release/Installer 层  
   统一处理版本解析、GitHub Releases 资产定位、校验信息、下载到临时目录、解包与可执行文件验证。

3. Binary Replace 层  
   只负责备份、原子替换、失败恢复、`shell reinit` 调用与状态上报。

4. Shell 层  
   把 `hook`、`init/deinit/reinit`、`activate/deactivate/reactivate` 分成 `bash` / `zsh` 渲染器与共享 rc 文件管理逻辑。

推荐的内部模块形状：
- `crates/viper-cli/src/self_update.rs`
- `crates/viper-cli/src/release.rs`
- `crates/viper-cli/src/shell/{mod.rs,common.rs,rcfile.rs,bash.rs,zsh.rs,activate.rs}`
- `scripts/install.sh`

### Relevant References
- `mamba/micromamba/src/umamba.cpp` - `shell`、`self-update`、顶层 `activate` 的命令挂载位置
- `mamba/micromamba/src/update.cpp` - 自更新的备份替换、失败恢复与 `shell reinit`
- `mamba/micromamba/src/shell.cpp` - shell 子命令、默认前缀选择与“不用 target_prefix fallback”的边界
- `mamba/micromamba/src/activate.cpp` - 顶层 `activate` 的提示型行为
- `mamba/micromamba/tests/test_shell.py` - `hook/init/activate` 的 CLI 与输出语义
- `mamba/micromamba/tests/test_activation.py` - rc block 幂等、deinit 恢复、激活输出与环境路径用例
- `mamba/docs/source/installation/micromamba-installation.rst` - 脚本安装、自更新与 shell init 的用户生命周期
- `crates/viper-cli/src/main.rs` - 当前 CLI 入口，适合作为自管理路由切入点
- `crates/viper-core/src/config.rs` - 已有 `root_prefix` / `name -> prefix` 解析逻辑，可作为 shell 激活前缀规范化输入
- `crates/viper-cli/tests/cli_smoke.rs` - 当前 CLI 集成测试落点，可继续扩展自管理 smoke/snapshot

## Dependencies and Sequence

### Milestones
1. Milestone 1：上游基线与 CLI 切口固定
   - Phase A: 逐项记录 `mamba/` 中与本任务相关的真实行为：`shell` 子命令、顶层 `activate`、`self-update`、安装文档。
   - Phase B: 在 `crates/viper-cli/src/main.rs` 设计新的命令树和模块边界，明确哪些行为是“对齐”，哪些是“首版刻意不做”。
   - Phase C: 先补 CLI 帮助与参数解析测试，锁定命令面。

2. Milestone 2：Shell hook/init/deinit/reinit
   - Phase A: 实现 bash/zsh hook 渲染器与 snapshot 测试。
   - Phase B: 实现 rc 文件 marker block 管理、幂等插入、只删受管块。
   - Phase C: 用临时 `HOME` 覆盖 `init -> reinit -> deinit` 全流程与失败路径。

3. Milestone 3：Shell activate/deactivate/reactivate 与顶层 activate 指导
   - Phase A: 复用现有前缀命名规则，实现 stdout-only 的 shell 代码生成。
   - Phase B: 对齐顶层 `activate` 的提示文本和错误退出行为。
   - Phase C: 补齐 `base`、命名环境、显式前缀、目标不存在、非法组合等回归。

4. Milestone 4：Self-update 与 release 契约
   - Phase A: 固定 GitHub Releases 资产命名、校验来源、版本选择策略。
   - Phase B: 实现下载、校验、备份、替换、恢复、`shell reinit` 调用。
   - Phase C: 用本地 fixture 或 mock HTTP 验证成功路径、下载失败、校验失败、替换失败、回滚失败提示。

5. Milestone 5：Bootstrap installer、文档与门禁
   - Phase A: 完成 `scripts/install.sh` 与隔离环境脚本测试。
   - Phase B: 更新安装与更新文档，写清首版边界和偏差。
   - Phase C: 跑通 `fmt/clippy/test` 与脚本检查，形成最终回归锚点。

### Dependency Rules
- Milestone 1 是前置，没有明确的对齐/偏差清单，就无法判断后续实现是否越界。
- Milestone 2 必须早于 Milestone 3，因为顶层 `activate` 提示与 `self-update` 后的 `shell reinit` 都依赖 shell 生命周期先稳定。
- Milestone 4 可以在 Milestone 1 之后并行探索，但真正落地前需要复用 Milestone 2 已稳定的 `reinit` 路径。
- Milestone 5 收口所有外部可见行为，必须在 2/3/4 都具备稳定回归后执行。

## Implementation Notes

### Code Style Requirements
- 实现代码与注释不得出现计划流程术语，例如 `AC-`、`Milestone`、`Phase`、`Step`。
- 代码内使用领域命名，例如 `release_asset`, `backup_path`, `managed_block`, `shell_hook_script`, `activation_snippet`。
- 若必须抽共享逻辑，优先抽成与 solver/transaction 无关的纯函数或小型辅助模块，不要把自管理需求倒灌进 `viper-core` 的包管理主线。

### Recommended File Ownership
- `crates/viper-cli/src/main.rs`
  - 挂接新的 `self-update`、`shell`、顶层 `activate` 路由
- `crates/viper-cli/src/self_update.rs`
  - 版本解析、下载校验、备份替换、恢复与 `reinit`
- `crates/viper-cli/src/release.rs`
  - GitHub Releases 资产契约与元数据解析
- `crates/viper-cli/src/shell/`
  - bash/zsh hook、rc block 管理、activate/deactivate/reactivate 代码生成
- `scripts/install.sh`
  - 首次安装入口
- `crates/viper-cli/tests/cli_smoke.rs`
  - CLI 正反向集成测试
- `crates/viper-cli/tests/snapshots/`
  - hook / guidance 文本快照

--- Original Design Draft Start ---

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

--- Original Design Draft End ---

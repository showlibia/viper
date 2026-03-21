# Viper：Rust 重写 mamba 的实施计划

## 目标说明
在 `viper` 工作区内分阶段实现一个 conda 兼容的虚拟环境管理器，核心行为以 `mamba/` 源码与测试为基准。首阶段完成 `create/install/remove/list/info/config` 的可用兼容，随后补齐 SAT 依赖求解、事务执行与回滚、索引缓存一致性和 CLI/JSON 输出稳定性。

## 验收标准

遵循 TDD 思路，每条标准都包含正向与反向测试，确保可确定性验证。

- AC-1: CLI 命令与全局参数兼容基线落地（create/install/remove/list/info/config）
  - 正向测试（预期 PASS）：
    - `viper create -n t -c conda-forge python=3.11 --json` 能解析并返回结构化结果。
    - `viper install -p <prefix> numpy --dry-run` 仅返回事务计划，不写入前缀。
  - 反向测试（预期 FAIL）：
    - 目标前缀缺失时执行 `install/remove/list` 返回明确错误。
    - 非受管前缀（缺少 `conda-meta`）执行写操作被拒绝并给出错误码/错误信息。

- AC-2: repodata 获取、缓存与离线语义与 mamba 关键行为对齐
  - 正向测试（预期 PASS）：
    - 在线模式首次请求写入 `<cache>.json` 与 `<cache>.state.json`，后续命中 TTL。
    - 命中 ETag/Last-Modified 返回 304 时复用本地缓存并刷新元数据时间戳。
  - 反向测试（预期 FAIL）：
    - `--offline` 且缓存不存在时返回 `OfflineRepodataUnavailable`。
    - 远端失败且本地缓存不存在时返回网络错误，不得伪造成功结果。

- AC-3: 依赖求解从“单包优选”升级为“环境级闭包求解”
  - 正向测试（预期 PASS）：
    - 给定多 spec（如 `python numpy`）能求出一致可安装集合，包含传递依赖。
    - 开启 strict channel priority 时，候选过滤遵循高优先级 channel 优先。
  - 反向测试（预期 FAIL）：
    - 不可满足约束（冲突版本）返回可解释的冲突报告，而不是 `unknown` 占位结果。
    - 禁止回退到“只挑最高版本”导致的伪可解结果。
  - AC-3.1: 求解策略可追踪并可回归
    - 正向：`-vvv` 或等效调试输出包含候选筛选与最终决策摘要。
    - 反向：无法复现实验条件（channel/spec/平台）时测试应失败。

- AC-4: 事务执行与前缀状态管理具备原子性与可恢复性
  - 正向测试（预期 PASS）：
    - 成功安装后写入稳定状态（`conda-meta` 元数据与历史记录），`list` 可读。
    - `remove` 只删除目标包并保留环境其余状态一致性。
  - 反向测试（预期 FAIL）：
    - 解包/链接阶段故障时触发回滚，禁止留下半安装状态。
    - `--dry-run` 下出现任何文件系统写入视为失败。

- AC-5: 环境文件（YAML）与 pip section 兼容行为可验证
  - 正向测试（预期 PASS）：
    - 读取 `name/channels/dependencies/pip` 并合并 CLI specs 形成统一请求。
    - `create -f env.yaml` 在未显式给 prefix/name 时按文件名推导目标前缀。
  - 反向测试（预期 FAIL）：
    - 非 `.yml/.yaml` 文件被拒绝并返回 `UnsupportedEnvironmentFile`。
    - 空 spec、非法 spec 不得静默忽略。

- AC-6: 建立 mamba 行为回归映射并持续执行
  - 正向测试（预期 PASS）：
    - 从 `mamba/micromamba/tests` 选择高价值用例迁移为 Rust 集成测试。
    - CI 至少覆盖 `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test --workspace`。
  - 反向测试（预期 FAIL）：
    - 若新增行为无测试或与映射基线不一致，合入流程应阻断。
    - 仅文档声明“兼容”但无可执行测试证据时视为不达标。

## 范围边界

范围边界用于定义实现质量与技术选型的可接受区间。

### 上界（最大可接受范围）
完成与 micromamba 核心工作流高度一致的 Rust 实现：环境级 SAT 求解、事务回滚、缓存与离线语义、CLI/JSON 输出稳定，并形成可持续的 mamba 对照回归套件。  
实现覆盖 `create/install/remove/list/info/config` 的高频真实场景，且关键模块具备单元与集成测试。

### 下界（最小可接受范围）
在当前 `viper` 骨架上达到“可用 MVP”：  
具备可靠的 repodata 缓存/离线行为、可解释的失败路径、受管前缀状态一致性、基础依赖闭包求解（不再是单包优选），并通过工作区全量质量门禁。

### 可选实现
- 可使用：`rattler_conda_types`、`resolvo`/libsolv 绑定方案（二选一并文档化）、`reqwest`、`serde`、Rust workspace 分 crate 演进。
- 可使用：先复用 `viper-core` 现有模块，再按能力重构（如 `solver`、`transaction`、`state` 拆分）。
- 禁止：以“询问用户口头规则”替代 `mamba/` 源码与测试验证。
- 禁止：在生产路径使用 `unwrap()`、忽略错误、或无测试直接改动兼容语义。
- 禁止：跳过 `dry-run/offline` 关键分支验证即宣称兼容。

## 可行性提示与建议

> **说明**：本节仅用于参考与理解，属于建议性内容，不是强制实现指令。

### 概念实现路径
建议采用“行为对齐优先”的四层实现路径：
1. 输入层：统一 CLI、env file、config 合并规则，先固定请求模型。
2. 索引层：稳定 repodata 拉取/缓存/离线语义，构建可复现包索引视图。
3. 求解层：将当前候选排序替换为环境级求解，输出可审计的 transaction plan。
4. 执行层：实现 link/unlink 与失败回滚，保持 `conda-meta` 与历史记录一致。

伪流程：
`OperationRequest -> NormalizeSpecs -> LoadIndexes -> SolveEnvironment -> BuildTransaction -> (dry-run ? render : apply+persist) -> Report`.

### 相关参考
- `mamba/micromamba/src/main.cpp` - CLI 启动、异常处理与子命令入口模式。
- `mamba/micromamba/src/create.cpp` - 环境创建命令语义与参数流。
- `mamba/micromamba/src/install.cpp` - 安装命令核心路径。
- `mamba/micromamba/src/remove.cpp` - 卸载路径与前缀处理。
- `mamba/libmamba/src/api/create.cpp` - create API 流程编排参考。
- `mamba/libmamba/src/api/install.cpp` - install API 流程编排参考。
- `mamba/libmamba/src/core/subdir_index.cpp` - repodata/subdir 索引处理。
- `mamba/libmamba/src/core/transaction.cpp` - 事务与执行语义。
- `mamba/libmamba/src/core/prefix_data.cpp` - 前缀状态数据模型。
- `mamba/libmamba/src/solver/helpers.cpp` - 求解辅助逻辑入口。
- `mamba/docs/source/advanced_usage/package_resolution.rst` - channel priority 与版本选择策略说明。
- `mamba/micromamba/tests/test_create.py` - create 行为测试样例。
- `mamba/micromamba/tests/test_install.py` - install 行为测试样例。
- `mamba/micromamba/tests/test_remove.py` - remove 行为测试样例。
- `crates/viper-core/src/repodata.rs` - 现有缓存与条件请求实现基线。
- `crates/viper-core/src/solver.rs` - 当前“单包优选”逻辑，后续替换入口。
- `crates/viper-core/src/core.rs` - 操作编排总入口。
- `crates/viper-core/src/state.rs` - 受管前缀状态持久化实现。
- `crates/viper-cli/src/main.rs` - 当前 CLI 参数与子命令定义。

## 依赖与执行顺序

### 里程碑
1. 里程碑 1：兼容基线与行为矩阵固化
   - 阶段 A：逐条梳理 `mamba/micromamba/src` 与 `mamba/micromamba/tests`，形成 `viper` 对照矩阵（命令、参数、错误语义）。
   - 阶段 B：为现有 `viper` 行为补齐回归测试，冻结当前基线。

2. 里程碑 2：索引与请求规范化
   - 阶段 A：统一 spec/channel/config/env-file 合并规则，清理 `core.rs` 输入分支。
   - 阶段 B：强化 repodata 缓存/离线/304 路径测试与错误分类。

3. 里程碑 3：求解器升级
   - 阶段 A：选型并接入环境级求解引擎（`resolvo` 或 libsolv 绑定），实现传递依赖闭包。
   - 阶段 B：对齐 strict channel priority、版本/构建选择策略，并输出冲突解释。

4. 里程碑 4：事务执行与回滚
   - 阶段 A：设计 transaction plan 数据结构（link/unlink/fetch/extract）。
   - 阶段 B：落地 apply/rollback，并保证 `dry-run` 与失败恢复的可验证性。

5. 里程碑 5：CLI/JSON 稳定化与端到端收敛
   - 阶段 A：对齐高频输出字段和错误格式，补齐集成测试快照。
   - 阶段 B：跑通工作区质量门禁并整理迁移文档。

依赖关系：里程碑 1 是 2/3/4/5 的前置；里程碑 2 是 3 的前置；里程碑 3 是 4 的前置；里程碑 4 与 5 可部分并行，但 5 的最终验收依赖 4 完成。

## 实施备注

### 代码风格要求
- 实现代码与注释不得包含计划术语，例如 `AC-`、`Milestone`、`Step`、`Phase` 等流程标记。
- 这些术语只用于计划文档，不应出现在最终代码中。
- 代码命名应使用面向领域的语义化名称。
- 所有技术决策记录应附对应 `mamba/` 源码路径或测试路径，避免无依据偏离。


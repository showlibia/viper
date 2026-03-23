# Viper：基于 Mamba 源码的分阶段详细实施计划

## Goal Description
在 `viper` 工作区内分阶段实现一个以 `mamba/micromamba` 源码和测试为基准的 conda 兼容环境管理器。目标不是“功能大致可用”，而是将 `create/install/remove/list/info/config` 的请求归一化、spec 来源处理、索引缓存、环境级求解、事务执行、历史记录与 CLI/JSON 输出逐步收敛到可对照、可回归、可验证的行为基线。

本计划按 `mamba` 的真实控制流拆解：
- CLI 入口与配置装载：`mamba/micromamba/src/main.cpp`、`create.cpp`、`install.cpp`、`remove.cpp`
- API 编排与 file-spec 处理：`mamba/libmamba/src/api/install.cpp`、`create.cpp`、`remove.cpp`
- 索引与缓存：`mamba/libmamba/src/core/subdir_index.cpp`
- 前缀状态与历史：`mamba/libmamba/src/core/prefix_data.cpp`
- 求解与事务：`mamba/libmamba/src/solver/helpers.cpp`、`mamba/libmamba/src/core/transaction.cpp`

## Acceptance Criteria

- AC-1: CLI 命令入口、前缀选择与全局参数行为与 `mamba` 基线对齐
  - Positive Tests (expected to PASS):
    - `viper create -n t python --json`、`viper install -p <prefix> numpy --json`、`viper remove -p <prefix> xtensor --json` 返回稳定结构化结果。
    - `--name`、`--prefix`、`VIPER_TARGET_PREFIX`、`CONDA_PREFIX`、env-file `name` 的优先级与 `mamba/micromamba/tests/test_install.py` 中“兼容 conda 环境变量”的目标前缀矩阵语义一致（不要求兼容 mamba 专有环境变量）。
    - `config list/get/set`、`info --json` 输出包含高频字段且语义稳定。
  - Negative Tests (expected to FAIL):
    - 同时传 `--name` 与 `--prefix` 必须失败。
    - 缺失目标前缀时 `install/remove/list` 必须失败。
    - 非受管前缀执行写操作必须失败。

- AC-2: spec 输入源与请求归一化行为与 `mamba` 对齐
  - Positive Tests (expected to PASS):
    - CLI specs、YAML env file、classic spec file、explicit file、lockfile 均能按各自语义解析并归一化。
    - multiple `-f` 行为符合 `mamba/libmamba/src/api/install.cpp` 的 `file_specs_hook`：区分 `yaml` 与 `other`，explicit 模式保留 URL specs。
    - env-file 中 `name/channels/dependencies/pip` 与 CLI specs 可按顺序合并。
    - 显式文件支持 `@EXPLICIT`、URL fragment hash、`# platform:` 注释。
  - Negative Tests (expected to FAIL):
    - 空 spec、非法 MatchSpec、空非 YAML spec file 必须明确失败。
    - YAML 与 non-YAML file spec 混用必须失败。
    - 非 `.yml/.yaml` 的 env-file 不能误走 YAML 语义。
  - AC-2.1: 兼容 file-spec 扩展路径
    - Positive:
      - `create/install -f classic.txt`、`-f explicit.txt`、`-f env.yaml` 的 `--print-config-only` 行为与上游测试一致。
      - explicit file 在 create/install 里走 explicit 请求路径，而不是 MatchSpec 求解路径。
    - Negative:
      - 不能把 explicit URL silently 降级为 `name=version=build` 后继续常规求解。

- AC-3: repodata 获取、缓存、条件请求与离线语义可对照 `subdir_index`
  - Positive Tests (expected to PASS):
    - 首次在线请求写入缓存 JSON 与 state 元数据。
    - TTL 新鲜时直接命中本地缓存，不发网络请求。
    - 命中 `304 Not Modified` 时复用本地 repodata 并刷新元数据时间戳。
    - `current_repodata.json` 与 `repodata.json` 使用不同缓存键，互不污染。
  - Negative Tests (expected to FAIL):
    - `--offline` 且无缓存时必须返回明确错误。
    - 远端失败且无缓存时必须返回网络错误。
    - state 元数据与缓存文件不一致时不能伪装为命中。
  - AC-3.1: 索引选择策略稳定
    - Positive:
      - 宽松 spec 优先使用 `current_repodata.json`。
      - 受限 spec、build/channel/subdir/hash 约束自动切换到 `repodata.json`。
    - Negative:
      - 不能因为错误的 repodata 选择导致候选缺失但仍报告成功。

- AC-4: 前缀状态、历史记录与 installed package 视图稳定
  - Positive Tests (expected to PASS):
    - `conda-meta/*.json` 与 history 记录可被稳定加载并用于 `list`、`remove`、后续 `install`。
    - requested specs map 与历史记录可驱动 keep-user-spec 行为。
    - revisions 视图能够从 history 中恢复 install/remove 差异。
  - Negative Tests (expected to FAIL):
    - 持久化失败不能留下部分写入状态。
    - history 与 conda-meta 不一致时不能静默忽略关键错误路径。

- AC-5: 求解从“单包候选排序”升级到“环境级 request/solution 模型”
  - Positive Tests (expected to PASS):
    - 多 spec 求解返回一致闭包，包含传递依赖。
    - strict channel priority、installed set preference、keep user specs 在环境级求解中成立。
    - remove 默认路径通过 solver request 生成目标状态，而不是仅做本地图裁剪。
    - 冲突结果可输出稳定、可解释的问题摘要。
  - Negative Tests (expected to FAIL):
    - 版本冲突或不可满足约束时不能给出伪可解结果。
    - remove 默认路径不能绕开 solver 直接删除导致语义漂移。
  - AC-5.1: 生产求解引擎固定并文档化
    - Positive:
      - 明确选定 `resolvo` 或 libsolv 绑定中的一种作为生产路径。
      - 通过 adapter 层保持 `viper-core` 的外部调用接口稳定。
    - Negative:
      - 不允许长期处于“二选一未定”状态。
      - 不允许测试走自研求解器、生产走另一条未覆盖路径。

- AC-6: 事务计划、执行与回滚具备可验证原子性
  - Positive Tests (expected to PASS):
    - transaction plan 明确区分 `fetch/extract/link/unlink`。
    - `dry-run` 不写入前缀、不写 history、不改 `conda-meta`。
    - persist/history/link 阶段任意失败时均能回滚到前一稳定前缀状态。
    - explicit install 路径与 solver install 路径都经过统一事务执行模型。
  - Negative Tests (expected to FAIL):
    - 任何失败都不能留下半安装或半删除状态。
    - 回滚后不能丢失原有 prefix 布局。

- AC-7: `list/info/config` 与 JSON 输出稳定并可快照回归
  - Positive Tests (expected to PASS):
    - `list` 支持 regex、`--full-name`、`--no-pip`、`--explicit`、`--md5`、`--sha256`、`--revisions`。
    - `info --json` 与 `config list/get/set --json` 输出关键字段稳定。
    - explicit export 输出 URL 与 hash 行为符合上游测试意图。
  - Negative Tests (expected to FAIL):
    - 互斥选项组合必须报错。
    - 缺字段、字段漂移、错误 JSON 结构必须由快照测试拦截。

- AC-8: 建立基于 `mamba` 源码与测试的持续回归体系
  - Positive Tests (expected to PASS):
    - 兼容矩阵中的每一行都能链接到上游路径与本地 enforcing test。
    - CI 至少覆盖 `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test --workspace`。
    - 高价值 create/install/remove/list/info/config 用例持续迁移。
  - Negative Tests (expected to FAIL):
    - 新增兼容声明如果没有 test + upstream reference，合入流程应阻断。
    - 兼容矩阵若存在与上游源码/测试不一致的假阳性条目，必须被修正而不是保留。

## Path Boundaries

### Upper Bound (Maximum Acceptable Scope)
实现与 micromamba 核心工作流高度一致的 Rust 版本：
- `create/install/remove/list/info/config` 的核心命令面行为稳定
- request normalization 覆盖 CLI、YAML、classic、explicit、lockfile
- `repodata/current_repodata`、offline、TTL、304、cache metadata 行为对齐
- remove 默认路径与 install/create 一样进入环境级 solver request
- 事务具备 `fetch/extract/link/unlink` 明确阶段和失败回滚
- 生产求解引擎固定并完成 adapter 化
- 兼容矩阵与回归测试持续运行

### Lower Bound (Minimum Acceptable Scope)
达到“可用且可验证的 conda MVP”：
- create/install/remove/list/info/config 基本可用
- spec 输入与 file-spec 语义不再明显偏离上游
- repodata 缓存/离线/304 行为可测
- 默认 remove 不再只是本地图裁剪
- 事务失败不会损坏前缀
- 工作区质量门禁全部通过

### Allowed Choices
- Can use:
  - `rattler_conda_types` 处理 MatchSpec 与 package metadata
  - `reqwest`、`serde`、`thiserror`
  - `resolvo` 或 libsolv 绑定，但必须在计划执行中固定其一并文档化
  - 先通过 adapter 保持 `viper-core` 接口稳定，再替换内部实现
- Cannot use:
  - 以口头约定替代 `mamba/` 源码和测试
  - 在生产路径使用 `unwrap()`、吞错、无测试改兼容语义
  - 将 explicit install、remove、dry-run、offline 等关键分支留在特殊旁路却宣称兼容

### Environment Variable Compatibility Policy
- 目标：环境变量只需保持与 conda 兼容，并使用 viper 专有环境变量命名；不要求支持 mamba 专有环境变量。
- 实施约束：
  - 前缀决策与默认目标环境行为由 `VIPER_TARGET_PREFIX` 和 `CONDA_PREFIX` 覆盖。
  - 推荐优先级：`--prefix` > `--name` > env-file `name` > `VIPER_TARGET_PREFIX` > `CONDA_PREFIX`。
  - 若代码中存在 mamba 专有变量分支，需在实现阶段移除，或仅保留短期迁移兼容并标注废弃。
- 验收口径：
  - 以 `VIPER_TARGET_PREFIX` + `CONDA_PREFIX` 驱动的目标前缀优先级测试必须通过。
  - 不以“同时兼容 mamba 专有变量”作为完成条件。

## Feasibility Hints and Suggestions

### Conceptual Approach
建议按 `mamba` 的控制流分四层实现，而不是按当前 Rust 文件粗暴补丁式迭代：

1. 输入层  
   `CLI args / rc / env vars / file specs / lockfile -> NormalizedRequest`

2. 索引层  
   `NormalizedRequest -> channel URLs -> repodata cache policy -> package universe`

3. 求解层  
   `NormalizedRequest + installed prefix snapshot -> solver request -> target solution`

4. 事务层  
   `target solution -> transaction plan -> dry-run render or apply + history/state persist`

建议引入以下内部数据模型：
- `NormalizedRequest`
- `SpecSource`（cli, yaml, classic, explicit, lockfile）
- `PrefixSnapshot`
- `SolveRequest`
- `TransactionPlan`
- `OperationReport`

### Relevant References
- `mamba/micromamba/src/main.cpp` - CLI 解析、异常处理、子命令入口
- `mamba/micromamba/src/create.cpp` - `create` 只是 install options 的专门化包装
- `mamba/micromamba/src/install.cpp` - install/revision 路径入口
- `mamba/micromamba/src/remove.cpp` - remove flags、`--prune-deps`、`--force`、`--all`
- `mamba/libmamba/src/api/install.cpp` - config load、file specs hook、explicit install、prefix checks、request 构建
- `mamba/libmamba/src/api/remove.cpp` - `Keep + Remove(clean_dependencies)` 的 solver-backed remove 语义
- `mamba/libmamba/src/core/subdir_index.cpp` - cache metadata、304、TTL、state file 读写
- `mamba/libmamba/src/core/prefix_data.cpp` - 已安装记录、pip 视图、拓扑顺序、history 协作
- `mamba/libmamba/src/core/transaction.cpp` - explicit transaction、solver transaction、history entry、下载与执行阶段
- `mamba/libmamba/src/solver/helpers.cpp` - python 版本相关求解辅助逻辑
- `mamba/micromamba/tests/test_create.py` - target prefix、file specs、lockfile、explicit 行为
- `mamba/micromamba/tests/test_install.py` - target prefix checks、config-only、spec source matrix
- `mamba/micromamba/tests/test_remove.py` - prune/force/default remove、history 与 in-use 边界

## Dependencies and Sequence

### Milestones
1. Milestone 1：命令流与兼容矩阵固化
   - Phase A: 梳理 `create/install/remove/list/info/config` 的 CLI 参数、target prefix checks、JSON 关键字段。
   - Phase B: 基于 `test_create.py`、`test_install.py`、`test_remove.py` 建立本地兼容矩阵。
   - Phase C: 对现有 `viper-cli` 行为做差异标注，区分“已对齐 / 偏差 / 未实现”。

2. Milestone 2：请求归一化与 spec 源模型
   - Phase A: 将 CLI specs、YAML、classic、explicit、lockfile 统一建模为 `SpecSource`。
   - Phase B: 复刻 `file_specs_hook` 的 `yaml vs other` 语义与 explicit 短路行为。
   - Phase C: 固定 target prefix 决策顺序，与 `--print-config-only` 视图保持一致。
   - Phase D: 为非法 spec、空文件、mixed file types、invalid env name 建立失败测试。

3. Milestone 3：索引与缓存层收敛
   - Phase A: 对齐 `current_repodata.json` / `repodata.json` 选择逻辑。
   - Phase B: 对齐 cache state、TTL、304、offline fallback、error taxonomy。
   - Phase C: 预留 shard/zstd 路径的接口边界，即使首版不实现，也要避免缓存模型写死。

4. Milestone 4：前缀状态与历史模型
   - Phase A: 抽离已安装 conda/pip 包视图与 requested specs map。
   - Phase B: 统一 history append、revisions 读取、dist-name 渲染。
   - Phase C: 明确 prefix load/persist 的错误边界，避免业务逻辑直接操纵文件树。

5. Milestone 5：环境级 solver request 落地
   - Phase A: 固定生产求解引擎，优先在本里程碑前半段完成选型和 adapter 接口定义。
   - Phase B: create/install 统一进入 `SolveRequest` 构建路径。
   - Phase C: remove 默认路径改用 `Keep + Remove(clean_dependencies)` 语义，保留 `--force` 特殊分支。
   - Phase D: 对齐 strict channel priority、installed preference、conflict explanation。

6. Milestone 6：事务计划与执行
   - Phase A: 用统一 `TransactionPlan` 表达 fetch/extract/link/unlink。
   - Phase B: 让 solver install、explicit install、remove 共用同一执行器。
   - Phase C: 对齐 dry-run 无写入、persist/history 失败回滚、prefix layout 恢复。
   - Phase D: 为 in-use 文件、history I/O 失败、部分写入故障补测试。

7. Milestone 7：输出稳定化
   - Phase A: 对齐 `list` 的 explicit/hash/revisions/filter 行为。
   - Phase B: 对齐 `info/config` 的 JSON key 集与错误语义。
   - Phase C: 为高频 JSON 输出补 snapshot tests，锁定字段和格式。

8. Milestone 8：回归套件与收口
   - Phase A: 将兼容矩阵每一行绑定到 enforcing test。
   - Phase B: 清理“本地测试通过但与上游不一致”的假阳性条目。
   - Phase C: 跑通全量门禁并整理剩余偏差列表。

### Dependency Rules
- Milestone 1 是所有后续阶段的入口，没有矩阵就无法判断兼容差异是否真实。
- Milestone 2 是 Milestone 3、5 的前置，因为 solver request 和 repodata 选择都依赖统一输入模型。
- Milestone 3 与 Milestone 4 共同构成 Milestone 5 的前置。
- Milestone 5 是 Milestone 6 的前置，没有稳定 target solution 就无法定义正确事务计划。
- Milestone 6 与 Milestone 7 可并行收尾，但 Milestone 7 的最终快照必须建立在稳定事务语义之上。

## Implementation Notes

### Code Style Requirements
- 实现代码与注释不得包含计划流程术语，例如 `AC-`、`Milestone`、`Phase`、`Step`。
- 代码内部使用领域语义命名，例如 `normalized_request`、`solve_request`、`requested_specs_map`、`transaction_plan`。
- 任何兼容语义调整都必须同时附带：
  - 上游源码或测试路径证据
  - 本地 enforcing test
  - 若行为存在刻意偏差，需在文档中显式注明原因与范围

### Recommended File Ownership
- `crates/viper-cli/src/main.rs`
  - 保持 CLI 参数面与子命令入口稳定
- `crates/viper-core/src/core.rs`
  - 只负责命令编排，不承载过多解析细节
- `crates/viper-core/src/spec.rs`
  - 负责所有 spec 源解析与归一化
- `crates/viper-core/src/repodata.rs`
  - 负责缓存与网络请求策略
- `crates/viper-core/src/solver.rs` 或其 adapter 子模块
  - 负责 solver request/solution 与解释
- `crates/viper-core/src/state.rs`
  - 负责 prefix state/history/pip 视图
- `crates/viper-core/src/transaction.rs`
  - 负责 transaction plan 与 apply/rollback

### Review Gates
- 每完成一个里程碑，至少要补一组“正向 + 反向”测试，而不是只补 happy path。
- 在声称兼容前，优先检查：
  - 是否已有 upstream reference
  - 是否已有 enforcing test
  - 是否存在“本地测试通过但上游行为不同”的假阳性


--- Original Design Draft End ---

# Rust 重写 mamba 的初稿（viper）

## 背景

目标是在本仓库中用 Rust 重写 mamba 的核心能力，产出一个兼容 conda 行为的虚拟环境管理器（当前二进制名为 `viper`）。  
技术细节应以 `mamba/` 目录下源码与测试行为为主，不依赖口头约定。

## 目标

1. 复刻 micromamba 的核心工作流：`create/install/remove/list/info/config`。
2. 兼容 conda 的关键输入与输出行为：
   - MatchSpec 解析与约束求解结果可预期；
   - channel 与 subdir 解析一致；
   - repodata 缓存与离线行为可用；
   - 环境前缀目录结构与状态文件稳定。
3. 在 Rust 工作区内建立清晰模块边界：
   - `viper-core`：求解、repodata、事务、状态；
   - `viper-cli`：命令与全局参数；
   - 后续可选 `viper-ffi` 兼容层。

## 现状（基于仓库代码）

- 已有 `viper-cli` 命令骨架与全局参数。
- 已有 `viper-core` 的基础实现：
  - `repodata` 下载和 TTL 缓存；
  - 基于 `rattler_conda_types::MatchSpec` 的简单候选筛选；
  - `create/install/remove/list/info/config` 流程；
  - 状态文件 `conda-meta/viper-state.json`。
- 目前尚未达到 mamba 级别：
  - 不是 libsolv 级 SAT 依赖求解；
  - 缺少完整事务执行（link/unlink、回滚、历史记录）；
  - 与 micromamba CLI 细节、错误语义、输出兼容性仍有差距。

## 关键参考源码（必须优先）

- `mamba/micromamba/src/*.cpp`（命令行为与参数语义）
- `mamba/libmamba/src/api/*.cpp`（高层流程）
- `mamba/libmamba/src/core/transaction.cpp`（事务）
- `mamba/libmamba/src/core/prefix_data.cpp`（前缀状态）
- `mamba/libmamba/src/core/subdir_index.cpp`（索引与 repodata）
- `mamba/libmamba/src/solver/*` 与 `docs/source/advanced_usage/package_resolution.rst`（求解策略）
- `mamba/micromamba/tests/*`（行为回归基线）

## 范围建议

- 先做 “可用且可验证” 的 conda 兼容最小集（MVP），再逐步逼近 mamba 完整行为。
- 每个行为改动必须附带 Rust 测试，并建立与 mamba 测试样例对照。


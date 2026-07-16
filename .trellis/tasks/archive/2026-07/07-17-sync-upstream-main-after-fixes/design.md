# Design: 受门控的 upstream 语义合并

## 状态机

```text
BLOCKED
  子任务 1-4 任一未验收/提交/归档，或工作树不干净
  -> 禁止读取 remote upstream

READY
  前四项顺序与归档证据成立
  -> 启动子任务 5
  -> 检查 remote 角色
  -> fetch upstream/main 一次
  -> 固定 upstream SHA

ANALYZE
  比较 base..upstream 与 base..fork
  -> 无产品语义冲突：合并固定 SHA
  -> 有产品语义冲突：PAUSED_FOR_DECISION

VALIDATE
  子任务 1-4 回归 + upstream 影响测试 + 全量门槛
  -> 提交并归档子任务 5
  -> 父任务最终集成验收
```

## Remote 角色

| Remote/工具 | 允许用途 | 禁止用途 |
| --- | --- | --- |
| `origin` | 常规仓库目标；另有授权时的 fork 操作 | 依赖含双 remote 时的隐式解析 |
| `FingerCaster/aio-coding-hub` | `gh -R`/`--repo` 的显式目标 | 省略目标后误操作其他仓库 |
| `upstream` | 本任务进入门槛后 fetch `main` | push、恢复 push URL、PR/issue/release 操作 |

实现阶段若使用 GitHub CLI，先确保默认仓库是 `FingerCaster/aio-coding-hub`，所有检查或
变更命令仍显式传 `-R FingerCaster/aio-coding-hub`。同步 upstream 本身使用 git fetch，
不把 `gh` 指向 upstream。

## 固定输入与合并拓扑

1. 记录 `pre_merge_head`。
2. fetch 后将 `refs/remotes/upstream/main^{commit}` 解析为 `upstream_sha`。
3. 记录 `merge_base(pre_merge_head, upstream_sha)` 并基于三个固定 SHA 分析双方变更。
4. 合并 `upstream_sha`，而不是在之后重新解析可能移动的远端引用。
5. 使用 merge commit 保留子任务 1–4 的独立提交和顺序。
6. 验收后以 ancestry check 证明 `upstream_sha` 已完整进入结果历史。

如果验收期间重新 fetch 得到新 SHA，不自动扩大本次输入；新漂移留给明确的新决策或重新
开始分析，避免测试对象在过程中变化。

## 冲突分类

### 可直接处理

- 纯格式、导入排序或可证明等价的机械重构。
- upstream 新增且不触及 fork 行为的文件。
- 双方修改可以通过现有合同与测试唯一确定结果的兼容性调整。

### 必须暂停

- 文本冲突涉及 fork 特有 UI、路由、账户展示、安全策略或发行行为。
- 干净合并却重新引入已删除的 reasoning guard/continuation-repair 产品面。
- upstream 改变子任务 1–4 刚修复的 gate、Query 所有权、NewAPI 语义或 Skill 预算。
- upstream 改变 usage、response-id、TTFB、取消、模型发现、20 MiB 非 SSE 上限等网关
  契约。
- 无法仅凭规格和测试确定用户期望的两种产品行为。

暂停报告必须包含文件/提交、两侧行为、用户影响和至少两个可行选项。报告完成后保持
merge 可检查状态，不提交臆测性解决方案。

## 验收设计

- 先运行四个已归档子任务各自 `implement.md` 中的聚焦回归。
- 再按 upstream 变更文件扩展受影响测试。
- 最后运行父任务规定的构建、完整 precommit/prepush 与 Rust 测试。
- 通过负向搜索和行为测试确认已删除产品面没有恢复。
- 检查 remote 配置和 reflog/命令记录，证明没有 upstream push。

## 回滚

- merge 尚未提交且验证失败：保留证据后使用非破坏性的 merge abort，前四项提交不变。
- merge 已提交后发现回归：整体 revert merge commit，不选择性丢弃 upstream 文件，也
  不 reset/rebase 已归档历史。
- 因产品语义冲突暂停不等于失败；在用户决定前保持任务 active，不启动父任务验收。

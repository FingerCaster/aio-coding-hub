# 发布自启动修复版本 - Technical Design

## Scope Boundary

发布前唯一允许的代码修改是修复
`response/upstream_error.rs` 中 `upstream_error_decision` 的参数数量。函数的输入值、
决策顺序、返回值以及所有调用时机保持不变；发布配置和 workflow 本身不做修改。

## Clippy Fix

在 `upstream_error.rs` 内定义文件私有参数结构，归组一次决策所需的规则匹配状态、
有效 retry policy、已用配置型重试数、当前 attempt 索引，以及配置型与基线 attempt
上限。`upstream_error_decision` 改为接收该结构（可连同极少数真正独立的输入，前提是
参数总数满足 Clippy），生产调用点和单元测试以相同字段构造输入。

必须保留以下语义不变量：

- count-tokens 请求立即 `Abort`，不计为配置型重试。
- 只有已经匹配的 HTTP 规则可以消费配置型瞬时重试容量。
- 配置型决策优先于通用基线决策；配置型预算耗尽后再回落到基线决策。
- 未匹配规则不能消费为瞬时重试预留的额外 attempt。
- 基线 `RetrySameProvider` 达到基线上限后切换 Provider。
- Provider override、熔断失败计数和 attempt reason 均不受本次结构调整影响。

参数结构保持文件私有，不扩展跨模块 API，不复制 policy，不新增持久化或前端契约。

## Release Flow

```text
local main + Clippy fix
  -> origin/main + successful main CI
  -> release workflow dispatch #1
  -> release-please PR + Cargo.lock sync + successful PR CI
  -> merge release PR (immutable release commit SHA)
  -> release workflow dispatch #2
  -> tag/draft release resolved before matrix build
  -> matrix jobs checkout immutable SHA
  -> stable assets + latest.json
  -> publish release + Homebrew job
```

GitHub CLI 命令必须显式指定 `-R FingerCaster/aio-coding-hub`。发布 tag、Release
target、workflow `checkout_ref` 与 release PR merge commit SHA 必须相互一致。

## Compatibility And Rollback

- 代码修复不改变序列化格式、数据库、配置、生成绑定或用户可见行为。
- 推送前若门禁失败，只修复当前分支上的具体回归，不推送。
- release PR 合并前若 CI 或版本审查失败，保留 PR 并修正后重跑，不创建 Release。
- Release 构建失败时保留 draft 与日志，修复具体发布问题后重跑现有 workflow；不得以
  可变分支替代不可变 SHA 构建。

# Codex 受管模型选择器集成

## 结论

现有受管 Profile 文件只能通过 `codex --profile <name>` 显式选择，不会自动进入
Codex 的 `/model` 模型选择器。要让用户创建 Profile 后在 Codex 内直接看见并选择，
AIO 必须在 Codex CLI 代理启用期间维护一个合并后的 `model_catalog_json`。

## 已确认的 Codex 行为

- Codex `0.134.0+` 的 Profile 是 `$CODEX_HOME/<name>.config.toml`，通过
  `--profile <name>` 加载；Profile 本身不是模型目录条目。
- `model_catalog_json` 是启动时读取的完整目录覆盖，不是对内置目录的增量追加。
  因此 AIO 不能只写受管模型，必须保留当前基础目录中的全部模型和未知字段。
- 目录顶层协议为 `{"models":[ModelInfo...]}`，不是普通 OpenAI
  `{"data":[{"id":"..."}]}`。
- Codex `debug models --bundled` 可输出与当前已安装 CLI 版本匹配的完整基础目录。
- `/model` 显示 `ModelPreset.model`，该值来自目录条目的 `slug`；`display_name`
  不能替代难读的 `aio/<model_uuid>`。
- `model_catalog_json` 仅在进程启动时应用；已有 Codex 会话不会热更新。

## 首版实现决策

1. Codex 侧仍只有 `model_provider = "aio"`，所有受管请求继续先进入 AIO 网关。
2. 新 Profile 对 Codex 暴露可读 alias `aio/<profile_name_key>`；网关通过
   `codex_managed_profiles.profile_name_key` 精确查表，再取得 model/provider 绑定。
3. 网关继续接受旧的 `aio/<model_uuid>`，保证测试版已创建的 Profile 文件可用。
4. 用户已有 `model_catalog_json` 时以其完整目录为基础；否则调用当前 Codex 的
   `debug models --bundled`。基础目录的未知字段原样保留。
5. AIO 目录条目克隆当前基础目录的一个可见模板，以继承当前版本新增的必填字段；
   再覆盖 slug、描述、优先级及保守能力值。
6. 合并目录写入 AIO 应用数据目录，根 `config.toml` 只在 Codex CLI 代理启用期间
   指向它；关闭代理时恢复启用前的 `model_catalog_json`。
7. 生成目录带 AIO 所有权元数据和 payload hash。文件被外部修改、基础目录无效、
   Codex CLI 不可用或配置发生并发漂移时失败关闭，不覆盖未知内容。
8. 创建/删除 Profile 与目录同步共用 Profile 生命周期锁，并在失败时补偿数据库、
   Profile 文件、生成目录和 `config.toml`，避免部分成功。
9. 创建成功后 UI 明确提示用户新建或重启 Codex 会话，再通过 `/model` 选择
   `aio/<profile_name_key>`。
10. Windows npm 安装通常解析为 `codex.cmd`。Rust 子进程必须把可执行路径和
    `debug models --bundled` 参数分开传给 `std::process::Command`；手工拼接
    `cmd.exe /S /C` 会把 `\"codex.cmd\"` 当成字面命令名并导致启动失败。

## 路由检测兼容

可读 alias 只改变 canonical requested model。网关仍在发送前把它解析为同一个
`ManagedModelRoute`，并把请求 body 中的模型改写为 `remote_model_id`。路由检测继续
只比较最终 wire model 与原始上游 observed model，因此正常 alias 还原不产生
`model_route_mapping`；真实上游错路由仍保持严重告警。

## 证据

- 当前 Codex 手册：Profiles 与 `model_catalog_json` 配置说明。
- 本机 Codex `0.144.6`：`codex debug models --bundled` 输出 8 个完整 `ModelInfo`。
- 本机 Windows Codex `0.144.6`：修复 `.cmd` 结构化启动后，真实 app-server
  `model/list` 可读取生成目录中的 `aio/real-smoke`。
- Codex `rust-v0.144.6` 源码：`ModelsResponse`、`ModelInfo -> ModelPreset`、
  `model_catalog_json` 启动时完整替换和 TUI model picker 使用 `preset.model`。

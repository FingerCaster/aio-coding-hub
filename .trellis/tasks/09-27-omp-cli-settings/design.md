# 设计

## 来源与字段

以本地 OMP 源码 7853b4e499936f9dcc13c9b64adb55f6b342aabf（18.3.2）为依据，核对 docs/settings.md、docs/task-agent-discovery.md、config/model-settings.ts、config/model-roles.ts、task/settings.ts 与 discovery/helpers.ts。默认模型使用 modelRoles.default，不使用已退休的 defaultProvider/defaultModel。原生 Agent 根目录为当前 profile 的 agentDir/agents。

## 分层

- domain/omp_settings：类型、可编辑字段白名单、值校验及安全投影；record 按成员增量修改而非重建整张表。
- infra/native_cli/omp_settings：复用目标锁、严格 YAML/JSON 解析、受限文件读取、私有备份与原子替换；配置路径独立解析 config.yml > config.yaml > settings.json（旧版只读）。
- app/omp_settings_service + commands：只接受当前已选择 OMP targetId；锁内重新验证选中目标。模型选项从受限 models 文档投影，补充有来源版本的内置目录，不读取 auth。
- services/query/components：生成的类型化 IPC，按 targetId 缓存；保存只发变更字段，冲突保留草稿并要求重载。表单不因后台刷新丢失草稿，切换目标隔离状态。

## 写入语义

读结果包含路径/文件字节的 revision。保存携带 expectedRevision，重新解析与校验文件优先级并比较修订；写入前再次比较。删除字段表示恢复原生继承，而非写入猜测的默认值。只修改白名单路径；未知字段不回传前端且不丢失。YAML 排版可能规范化，原文保存在同目录受限 .aio-native-backups。

自定义 Agent 文件名由后端校验的单一文件名生成，仅限当前 agentDir/agents 下 Markdown；读编辑单独请求，CAS 保存，保留未知 frontmatter。内置 Agent 只写 task.* 覆盖，不改安装目录。插件/项目 Agent 不做不可靠的全量有效配置承诺。

## 可用性

模型/角色选择允许现有值与手工选择器，明确配置存在不等于已登录。设置分为“模型与会话”“子任务”“Agent”，显示保存状态、原生字段说明和生效范围。自动检查不会触发任何配置写入。

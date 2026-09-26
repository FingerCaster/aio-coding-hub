# 继承模型显示修复 MSI

- 构建时间：2026-09-27T02:47:19.7971081+08:00，本地 Windows x64 未签名测试包，版本 0.60.43。
- 产物：D:\OrcaProjects\aio-coding-hub-fork\omp-pi-channel-integration\.local\test-builds\omp-model-inheritance-20260927-024719\AIO-Coding-Hub-0.60.43-omp-inheritance-win64.msi
- 大小：18628608 字节。
- MSI SHA256：5315ff84cc7b71a9b4747b2d469a940ece9e0084737205b4bdb4a3927ec4592e
- EXE SHA256：fd0df3dd45435066a2548e03428f831f69e1f41db72f0a9d141330766529f959
- 源码指纹：8e1e4b91978bcea1ed58f8cfc9d857aeb2c8af785808ddc44518d38c1154cb51。
- 前端产物：dist\assets\CliManagerPage-DAE-ksQl.js；SHA256：cdf9e44156e7967d7cd17ff43fe8694869140c7fa11690f464950aef3ea39345。

## 内容与验证

继承项显示当前配置可解析的具体模型；展示模型 selector、继承路径和思考设置。预览随草稿更新，Agent 定义/覆盖引用也展开。未固定模型、候选列表和父会话模型明确说明运行时选择；修复长模型选项挤宽两列表单。

52 项前端回归、类型检查、定向 ESLint、文档契约链接检查通过。浏览器 1440/1280/720 宽度与浅深色验证通过，详情见 model-inheritance-preview.md。

最终 pnpm tauri:build -- --bundles msi 成功，Rust release 编译耗时 6m49s。构建后与封装前源码快照验证一致；只在产物完成后更新任务交付记录。包内主程序和四项资源哈希均与本次构建一致；MSI 升级身份与同版本升级检测、ControlEvent 关联均通过。现有 WiX 反编译提示不对应实际 MSI 关联缺失。

同目录保留 source-snapshot.json、package-verification.json、installer-verification.json、frontend-verification.json 和 SHA256SUMS.txt。旧测试包保留，不提交或发布混合改动。

## 安装验收

安装前退出 AIO（含托盘）；进入 CLI 管理 → OMP → 原生设置 → 其他模型角色与自定义角色。实际安装/WebView 与用户配置尚待用户验收，本轮未改写用户真实原生配置。

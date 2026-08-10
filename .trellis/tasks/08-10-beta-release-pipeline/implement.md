# Beta 发布流水线实施清单

- [ ] 扩展 tag/channel parser 和 release-promotion candidate schema，先补 stable/Beta selftests。
- [ ] 增加 release-version-overlay.mjs 及 selftest，覆盖四版本文件、Cargo.lock、额外 diff 和跨平台 attestation。
- [ ] 增加 channel-state/release-channel.mjs 及 Git fixture selftest，覆盖首次分支、CAS race、单调推进、pause 和 unsafe target。
- [ ] 以最小条件分支修改 release.yml：Beta tag/source/prerelease、public make_latest=false、Homebrew skip；稳定默认行为逐项断言。
- [ ] 运行 support-matrix、release-source、release-promotion、signing-scope、Homebrew selftests 和 yaml/script syntax checks。
- [ ] 将最终 manifest/state/attestation 字段记录到父任务，解除 updater-core 的前置阻塞。

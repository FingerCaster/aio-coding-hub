# 多会话单飞回切回归 - 技术设计

新增一个首请求可阻塞、后续请求正常响应的 counting upstream。测试先绑定 winner 和动态
follower session 到低优先级 Provider，再把高优先级 Provider 置为已到期 OPEN。

1. 启动 winner 请求并等待 upstream 明确信号，保证 probe lease 已 dispatch。
2. 并发执行所有 follower 第一波请求；它们必须 gate-skip 高优先级 Provider 并落到当前
   Provider。
3. 释放 winner，确认其 probe success 关闭 circuit。
4. 并发执行 follower 第二波请求，确认都以普通 direct 请求命中恢复 Provider并更新绑定。

使用通知/oneshot 同步，不使用固定 sleep 建立竞态。日志 channel 容量覆盖全部两波请求。

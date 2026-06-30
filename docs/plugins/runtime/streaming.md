# 流式响应插件

Streaming plugins 使用 `gateway.response.chunk`。

运行时会提供：

- 当前 chunk 的 bytes 或 text。
- 用于跨 chunk 检测的有界 sliding window。
- trace metadata。
- 已按 hook、capability 和 context budget 裁剪的 context。

`stream.inspect` 和 `stream.modify` 是内部 context/mutation labels，不是 Extension Host manifest permissions。宿主只会在当前 hook contract 和预算允许时提供 stream context，并只接受 envelope 内的 `streamChunk` mutation。

流式插件不能假设自己能看到完整响应。它们应只检测有界模式，并根据当前可见 context 返回 pass、warn、replace 或 block。

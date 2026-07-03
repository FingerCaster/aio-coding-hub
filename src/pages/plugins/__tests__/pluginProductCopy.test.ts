import { describe, expect, it } from "vitest";
import {
  describePluginPermission,
  describePluginRuntime,
  pluginRiskLabel,
  pluginStatusLabel,
} from "../pluginProductCopy";

describe("pluginProductCopy", () => {
  it("translates plugin statuses into user-facing Chinese labels", () => {
    expect(pluginStatusLabel("enabled")).toBe("运行中");
    expect(pluginStatusLabel("disabled")).toBe("已关闭");
    expect(pluginStatusLabel("quarantined")).toBe("已隔离");
  });

  it("translates permission ids into user impact copy", () => {
    expect(describePluginPermission("request.body.read")).toEqual({
      label: "读取你发送给模型的内容",
      detail: "用于检查或分析请求正文。",
      risk: "high",
    });
    expect(describePluginPermission("request.body.write")).toEqual({
      label: "修改你发送给模型的内容",
      detail: "用于在发送前替换、追加或删除请求正文。",
      risk: "high",
    });
    expect(describePluginPermission("log.redact")).toEqual({
      label: "处理本地请求日志",
      detail: "用于在日志保存前隐藏敏感信息。",
      risk: "medium",
    });
  });

  it("describes runtimes without making implementation jargon primary", () => {
    expect(describePluginRuntime("extensionHost")).toEqual({
      label: "扩展主机插件",
      detail: "通过 Extension Host 运行打包后的 TypeScript/JavaScript 插件输出。",
    });
    for (const runtime of ["wasm", "process", "native", "native:legacyPrivacy"]) {
      expect(describePluginRuntime(runtime)).toEqual({
        label: "不支持的旧插件运行时",
        detail: "该插件使用预发布时期的运行方式，请安装 Extension Host 版本。",
      });
    }
  });

  it("does not present legacy runtimes as user choices", () => {
    expect(describePluginRuntime("wasm")).toEqual({
      label: "不支持的旧插件运行时",
      detail: "该插件使用预发布时期的运行方式，请安装 Extension Host 版本。",
    });
  });

  it("maps risk levels to readable labels", () => {
    expect(pluginRiskLabel("low")).toBe("低风险");
    expect(pluginRiskLabel("medium")).toBe("中风险");
    expect(pluginRiskLabel("high")).toBe("高风险");
    expect(pluginRiskLabel("critical")).toBe("关键风险");
  });
});

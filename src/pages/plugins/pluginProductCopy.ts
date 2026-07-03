import type { PluginPermissionRisk, PluginStatus } from "../../services/plugins";

export type PermissionDescription = {
  label: string;
  detail: string;
  risk: PluginPermissionRisk;
};

export type RuntimeDescription = {
  label: string;
  detail: string;
};

const STATUS_COPY: Record<PluginStatus, string> = {
  available: "可安装",
  installed: "待启用",
  enabled: "运行中",
  disabled: "已关闭",
  update_available: "有更新",
  incompatible: "不兼容",
  quarantined: "已隔离",
  uninstalled: "已卸载",
};

const RISK_COPY: Record<PluginPermissionRisk, string> = {
  low: "低风险",
  medium: "中风险",
  high: "高风险",
  critical: "关键风险",
};

const PERMISSION_COPY: Record<string, PermissionDescription> = {
  "request.meta.read": {
    label: "读取请求基本信息",
    detail: "用于识别 CLI、路径、模型等非正文信息。",
    risk: "low",
  },
  "request.header.read": {
    label: "读取请求头",
    detail: "用于根据普通请求头判断处理方式。",
    risk: "medium",
  },
  "request.header.readSensitive": {
    label: "读取敏感请求头",
    detail: "可能包含认证或会话相关信息。",
    risk: "high",
  },
  "request.header.write": {
    label: "修改请求头",
    detail: "用于在发送前增加或调整请求头。",
    risk: "high",
  },
  "request.body.read": {
    label: "读取你发送给模型的内容",
    detail: "用于检查或分析请求正文。",
    risk: "high",
  },
  "request.body.write": {
    label: "修改你发送给模型的内容",
    detail: "用于在发送前替换、追加或删除请求正文。",
    risk: "high",
  },
  "response.header.read": {
    label: "读取响应头",
    detail: "用于根据响应元信息判断处理方式。",
    risk: "low",
  },
  "response.header.write": {
    label: "修改响应头",
    detail: "用于调整返回给客户端的响应头。",
    risk: "medium",
  },
  "response.body.read": {
    label: "读取模型返回内容",
    detail: "用于检查或分析响应正文。",
    risk: "high",
  },
  "response.body.write": {
    label: "修改模型返回内容",
    detail: "用于在返回前替换或删除响应正文。",
    risk: "high",
  },
  "stream.inspect": {
    label: "读取流式响应片段",
    detail: "用于检查模型逐步返回的内容。",
    risk: "high",
  },
  "stream.modify": {
    label: "修改流式响应片段",
    detail: "用于在流式返回过程中替换或阻断内容。",
    risk: "high",
  },
  "log.redact": {
    label: "处理本地请求日志",
    detail: "用于在日志保存前隐藏敏感信息。",
    risk: "medium",
  },
};

export function pluginStatusLabel(status: PluginStatus): string {
  return STATUS_COPY[status] ?? status;
}

export function pluginRiskLabel(risk: PluginPermissionRisk): string {
  return RISK_COPY[risk] ?? risk;
}

export function describePluginPermission(permission: string): PermissionDescription {
  return (
    PERMISSION_COPY[permission] ?? {
      label: permission,
      detail: "该权限来自插件清单，当前版本没有更友好的说明。",
      risk: "medium",
    }
  );
}

export function describePluginRuntime(runtime: string): RuntimeDescription {
  if (runtime === "extensionHost") {
    return {
      label: "扩展主机插件",
      detail: "通过 Extension Host 运行打包后的 TypeScript/JavaScript 插件输出。",
    };
  }

  return {
    label: "不支持的旧插件运行时",
    detail: "该插件使用预发布时期的运行方式，请安装 Extension Host 版本。",
  };
}

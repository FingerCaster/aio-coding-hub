import { describe, expect, it } from "vitest";
import type { OmpAgentSummary, OmpSettingsSnapshot } from "../../../services/ompSettings";
import {
  ompAgentModelPreview,
  ompRoleModelPreview,
  ompSelectorModelPreview,
  splitOmpModelSelector,
} from "../omp/ompModelPreview";
import { settingsPatches } from "../omp/ompSettingsForm";

const snapshot: OmpSettingsSnapshot = {
  targetId: "test",
  configPath: "test/config.yml",
  agentsDir: "test/agents",
  revision: "test",
  writable: true,
  warnings: [],
  values: {},
  agents: [],
  fields: [
    {
      key: "defaultThinkingLevel",
      defaultValue: "high",
      label: "默认思考",
      group: "session",
      kind: "enum",
      options: [],
      min: null,
      max: null,
    },
  ],
  models: [
    {
      selector: "aio/model",
      label: "AIO 聚合模型",
      source: "原生配置 / AIO 入口",
      thinkingLevels: ["medium", "high"],
    },
    {
      selector: "native/fast",
      label: "快速模型",
      source: "内置目录",
      thinkingLevels: ["low", "medium"],
    },
    {
      selector: "native/model:high",
      label: "名称含冒号的模型",
      source: "内置目录",
      thinkingLevels: ["low", "medium"],
    },
  ],
};
const values = { modelRoles: { default: "aio/model:high" } };
const agent: OmpAgentSummary = {
  name: "scout",
  source: "bundled",
  description: "Scout",
  fileName: null,
  model: ["@smol"],
  thinking: "medium",
};

describe("OMP configuration model previews", () => {
  it.each(["smol", "slow", "tiny", "memory"])(
    "expands inherited %s to the configured default without saving it",
    (role) => {
      const before = structuredClone(values);
      const preview = ompRoleModelPreview(role, values, snapshot);
      expect(preview).toMatchObject({
        kind: "model",
        label: "AIO 聚合模型",
        selectors: ["aio/model:high"],
        thinking: "high",
      });
      expect(preview.source).toContain("@default");
      expect(settingsPatches(before, values)).toEqual([]);
    }
  );
  it("shows the configured target of nested, dotted and legacy aliases", () => {
    const configured = {
      modelRoles: { ...values.modelRoles, "review.v2": "pi/smol:medium", custom: "@review.v2" },
    };
    expect(ompRoleModelPreview("custom", configured, snapshot)).toMatchObject({
      kind: "model",
      selectors: ["aio/model:medium"],
      thinking: "medium",
    });
  });
  it("does not make the advisor inherit an unconfigured slow role from default", () => {
    expect(ompRoleModelPreview("advisor", values, snapshot)).toMatchObject({
      kind: "runtime",
      selectors: [],
    });
    expect(
      ompRoleModelPreview(
        "advisor",
        { modelRoles: { ...values.modelRoles, slow: "native/fast" } },
        snapshot
      )
    ).toMatchObject({ kind: "model", selectors: ["native/fast"] });
  });
  it("previews reset inheritance without deleting or materializing any settings", () => {
    const configured = { modelRoles: { ...values.modelRoles, smol: "native/fast" } };
    const before = structuredClone(configured);
    expect(ompRoleModelPreview("smol", configured, snapshot).selectors).toEqual(["native/fast"]);
    expect(ompRoleModelPreview("smol", configured, snapshot, true).selectors).toEqual([
      "aio/model:high",
    ]);
    expect(configured).toEqual(before);
  });
  it("does not choose the first catalog entry when native selection is automatic", () => {
    const preview = ompRoleModelPreview("default", {}, snapshot);
    expect(preview).toMatchObject({ kind: "runtime", selectors: [], thinking: "运行时确定" });
    expect(preview.description).toContain("认证");
  });
  it("does not promote one fallback or fuzzy pattern to the actual selected model", () => {
    expect(ompSelectorModelPreview("@smol, native/fast", values, snapshot)).toMatchObject({
      kind: "chain",
      selectors: ["aio/model:high", "native/fast"],
    });
    expect(ompSelectorModelPreview("native/*", values, snapshot).kind).toBe("chain");
    expect(ompSelectorModelPreview("fast", values, snapshot).kind).toBe("chain");
  });
  it("preserves a fully qualified model absent from the reference catalog", () => {
    expect(ompSelectorModelPreview("private/future:low", {}, snapshot)).toMatchObject({
      kind: "model",
      label: "private/future",
      selectors: ["private/future:low"],
      thinking: "low",
    });
  });
  it("detects role cycles and unconfigured custom roles", () => {
    const preview = ompRoleModelPreview("a", { modelRoles: { a: "@b", b: "@a" } }, snapshot);
    expect(preview.kind).toBe("unresolved");
    expect(preview.description).toContain("循环引用");
    expect(ompSelectorModelPreview("@missing", {}, snapshot).kind).toBe("unresolved");
    expect(ompSelectorModelPreview("@constructor", {}, snapshot).description).toContain(
      "未配置模型"
    );
  });
  it("preserves literal colon model IDs while supporting an additional effort suffix", () => {
    expect(splitOmpModelSelector("native/model:high", snapshot)).toEqual({
      base: "native/model:high",
      effort: "",
    });
    expect(ompSelectorModelPreview("native/model:high:medium", {}, snapshot)).toMatchObject({
      label: "名称含冒号的模型",
      thinking: "medium",
    });
  });
  it("resolves Agent definition aliases and uses explicit overrides first", () => {
    expect(ompAgentModelPreview(agent, values, snapshot)).toMatchObject({
      kind: "model",
      selectors: ["aio/model:high"],
      thinking: "high",
    });
    const overridden = { ...values, "task.agentModelOverrides": { scout: "native/fast" } };
    expect(ompAgentModelPreview(agent, overridden, snapshot)).toMatchObject({
      selectors: ["native/fast"],
      thinking: "medium",
    });
    expect(ompAgentModelPreview(agent, overridden, snapshot, true).selectors).toEqual([
      "aio/model:high",
    ]);
  });
  it.each([[], ["*"], ["@default"], ["@task"]].map((model) => ({ model })))(
    "keeps parent-session inheritance distinct from the global default: $model",
    ({ model }) => {
      const preview = ompAgentModelPreview({ ...agent, model }, values, snapshot);
      expect(preview).toMatchObject({ kind: "runtime", label: "继承父会话模型", selectors: [] });
      expect(preview.description).toContain("新会话默认参考：AIO 聚合模型");
    }
  );
  it("does not invent definitions for project/plugin agents", () => {
    expect(
      ompAgentModelPreview({ ...agent, source: "configured", model: [] }, values, snapshot)
    ).toMatchObject({ kind: "unresolved", label: "定义尚未加载", selectors: [] });
  });
  it.each(["@default:low", "pi/default:low", "*:low"])(
    "resolves the suffixed Agent selector %s instead of treating it as a parent marker",
    (selector) => {
      expect(ompAgentModelPreview({ ...agent, model: [selector] }, values, snapshot)).toMatchObject(
        { kind: "model", selectors: ["aio/model:low"], thinking: "low" }
      );
      expect(ompAgentModelPreview({ ...agent, model: [selector] }, {}, snapshot)).toMatchObject({
        kind: "runtime",
        label: "继承父会话模型",
        selectors: [],
      });
    }
  );
  it("resolves a configured task role and normalizes empty definition entries", () => {
    expect(
      ompAgentModelPreview(
        { ...agent, model: ["@task:low"] },
        { modelRoles: { task: "native/fast" } },
        snapshot
      )
    ).toMatchObject({ kind: "model", selectors: ["native/fast:low"] });
    expect(
      ompAgentModelPreview({ ...agent, model: [" @default ", ""] }, values, snapshot)
    ).toMatchObject({ kind: "runtime", label: "继承父会话模型" });
  });
});

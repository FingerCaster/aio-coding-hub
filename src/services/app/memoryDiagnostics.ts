import { commands, type AppMemoryDiagnosticsSnapshot } from "../../generated/bindings";
import { queryClient } from "../../query/queryClient";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "../generatedIpc";
import { logToConsole } from "../consoleLog";

const ESTIMATE_MAX_NODES = 200_000;
const QUERY_SCAN_LIMIT = 2_000;
const TOP_QUERY_LIMIT = 20;

export type { AppMemoryDiagnosticsSnapshot };

type SizeEstimate = {
  bytes: number;
  truncated: boolean;
};

type SizeEstimateBudget = {
  remainingNodes: number;
};

type FrontendQueryDiagnostic = {
  query_hash: string;
  query_key: string;
  status: string;
  fetch_status: string;
  observers: number | null;
  estimated_bytes: number;
  truncated: boolean;
};

type FrontendQueryGroupDiagnostic = {
  key: string;
  count: number;
  estimated_bytes: number;
};

export type AppMemoryDiagnosticsReport = {
  backend: AppMemoryDiagnosticsSnapshot;
  frontend: {
    href: string;
    query_count: number;
    scanned_query_count: number;
    scan_truncated: boolean;
    query_estimated_bytes: number;
    query_groups: FrontendQueryGroupDiagnostic[];
    top_queries: FrontendQueryDiagnostic[];
    js_heap?: {
      used_js_heap_size?: number;
      total_js_heap_size?: number;
      js_heap_size_limit?: number;
    };
  };
};

function estimateValueSize(value: unknown, budget: SizeEstimateBudget): SizeEstimate {
  const seen = new WeakSet<object>();
  let truncated = false;

  function walk(current: unknown): number {
    if (budget.remainingNodes <= 0) {
      truncated = true;
      return 0;
    }
    budget.remainingNodes -= 1;

    if (current == null) return 4;
    if (typeof current === "string") return current.length * 2;
    if (typeof current === "number") return 8;
    if (typeof current === "boolean") return 4;
    if (typeof current === "bigint") return 8;
    if (typeof current !== "object") return 0;
    if (seen.has(current)) return 0;

    seen.add(current);

    if (Array.isArray(current)) {
      let size = 16;
      for (let index = 0; index < current.length; index += 1) {
        if (budget.remainingNodes <= 0) {
          truncated = true;
          break;
        }
        size += 8 + walk(current[index]);
        if (truncated) break;
      }
      return size;
    }

    let size = 32;
    const record = current as Record<string, unknown>;
    for (const key in record) {
      if (!Object.prototype.hasOwnProperty.call(record, key)) continue;
      if (budget.remainingNodes <= 0) {
        truncated = true;
        break;
      }
      size += key.length * 2 + 8 + walk(record[key]);
      if (truncated) break;
    }
    return size;
  }

  return {
    bytes: walk(value),
    truncated,
  };
}

function safeStringifyKey(value: unknown): string {
  try {
    const text = JSON.stringify(value);
    if (!text) return String(value);
    return text.length > 512 ? `${text.slice(0, 512)}[Truncated]` : text;
  } catch {
    return String(value);
  }
}

function queryGroupKey(queryKey: unknown): string {
  if (Array.isArray(queryKey)) {
    return typeof queryKey[0] === "string" ? queryKey[0] : "unknown";
  }
  return "unknown";
}

function readJsHeap() {
  const memory = (
    performance as Performance & {
      memory?: {
        usedJSHeapSize?: number;
        totalJSHeapSize?: number;
        jsHeapSizeLimit?: number;
      };
    }
  ).memory;
  if (!memory) return undefined;
  return {
    used_js_heap_size: memory.usedJSHeapSize,
    total_js_heap_size: memory.totalJSHeapSize,
    js_heap_size_limit: memory.jsHeapSizeLimit,
  };
}

function insertTopQuery(
  topQueries: FrontendQueryDiagnostic[],
  diagnostic: FrontendQueryDiagnostic
) {
  const insertAt = topQueries.findIndex(
    (candidate) => diagnostic.estimated_bytes > candidate.estimated_bytes
  );

  if (insertAt === -1) {
    if (topQueries.length < TOP_QUERY_LIMIT) topQueries.push(diagnostic);
    return;
  }

  topQueries.splice(insertAt, 0, diagnostic);
  if (topQueries.length > TOP_QUERY_LIMIT) topQueries.pop();
}

function collectFrontendDiagnostics(): AppMemoryDiagnosticsReport["frontend"] {
  const queries = queryClient.getQueryCache().getAll();
  const topQueries: FrontendQueryDiagnostic[] = [];
  const groups = new Map<string, FrontendQueryGroupDiagnostic>();
  const estimateBudget: SizeEstimateBudget = {
    remainingNodes: ESTIMATE_MAX_NODES,
  };
  let queryEstimatedBytes = 0;
  let scannedQueryCount = 0;
  let scanTruncated = false;

  for (const query of queries) {
    if (scannedQueryCount >= QUERY_SCAN_LIMIT || estimateBudget.remainingNodes <= 0) {
      scanTruncated = true;
      break;
    }

    const estimate = estimateValueSize(query.state.data, estimateBudget);
    scannedQueryCount += 1;
    queryEstimatedBytes += estimate.bytes;

    const groupKey = queryGroupKey(query.queryKey);
    const group = groups.get(groupKey) ?? {
      key: groupKey,
      count: 0,
      estimated_bytes: 0,
    };
    group.count += 1;
    group.estimated_bytes += estimate.bytes;
    groups.set(groupKey, group);

    const observers =
      typeof query.getObserversCount === "function" ? query.getObserversCount() : null;
    insertTopQuery(topQueries, {
      query_hash: query.queryHash,
      query_key: safeStringifyKey(query.queryKey),
      status: String(query.state.status),
      fetch_status: String(query.state.fetchStatus),
      observers,
      estimated_bytes: estimate.bytes,
      truncated: estimate.truncated,
    });

    if (estimate.truncated) {
      scanTruncated = true;
      break;
    }
  }

  const queryGroups = Array.from(groups.values()).sort(
    (a, b) => b.estimated_bytes - a.estimated_bytes
  );

  return {
    href: typeof window === "undefined" ? "" : window.location.href,
    query_count: queries.length,
    scanned_query_count: scannedQueryCount,
    scan_truncated: scanTruncated,
    query_estimated_bytes: queryEstimatedBytes,
    query_groups: queryGroups,
    top_queries: topQueries,
    js_heap: typeof performance === "undefined" ? undefined : readJsHeap(),
  };
}

async function appMemoryDiagnosticsGet() {
  return invokeGeneratedIpc<AppMemoryDiagnosticsSnapshot>({
    title: "采集后端内存诊断失败",
    cmd: "app_memory_diagnostics_get",
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.appMemoryDiagnosticsGet(),
        (value) => value as AppMemoryDiagnosticsSnapshot
      ),
  });
}

export async function collectAppMemoryDiagnostics(): Promise<AppMemoryDiagnosticsReport> {
  const backend = await appMemoryDiagnosticsGet();
  const report: AppMemoryDiagnosticsReport = {
    backend,
    frontend: collectFrontendDiagnostics(),
  };

  logToConsole("info", "内存诊断快照已生成", report);
  return report;
}

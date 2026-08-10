import {
  commands,
  type ActiveRequestSnapshotItem as GeneratedActiveRequestSnapshotItem,
} from "../../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "../generatedIpc";
import type { GatewayAttemptEvent } from "./gatewayEvents";

export type ActiveRequest = Omit<
  GeneratedActiveRequestSnapshotItem,
  | "current_attempt"
  | "codex_infinite_retry_test"
  | "infinite_retry_phase"
  | "infinite_retry_round"
  | "infinite_retry_attempt"
> & {
  current_attempt: GatewayAttemptEvent | null;
  codex_infinite_retry_test?: boolean;
  infinite_retry_phase?: string | null;
  infinite_retry_round?: string | null;
  infinite_retry_attempt?: string | null;
};

export async function activeRequestLogsSnapshot() {
  return invokeGeneratedIpc<ActiveRequest[]>({
    title: "读取进行中请求失败",
    cmd: "active_request_logs_snapshot",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.activeRequestLogsSnapshot(), (rows) => rows),
  });
}

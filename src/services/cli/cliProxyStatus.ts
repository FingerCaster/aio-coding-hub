import type { CliProxyStatus as GeneratedCliProxyStatus } from "../../generated/bindings";
import type { CliKey } from "../providers/providers";
import type { Override } from "../generatedTypeUtils";

export type CliProxyStatus = Override<
  GeneratedCliProxyStatus,
  {
    cli_key: CliKey;
    current_gateway_origin?: string | null;
  }
>;

type CliProxyStatusInput = Pick<CliProxyStatus, "cli_key" | "enabled"> &
  Partial<Omit<CliProxyStatus, "cli_key" | "enabled">>;

export function createCliProxyStatus(input: CliProxyStatusInput): CliProxyStatus {
  return {
    base_origin: null,
    current_gateway_origin: null,
    applied_to_current_gateway: null,
    generation: null,
    route_mode: null,
    desired_enabled: null,
    aio_origin: null,
    guarded_origin: null,
    effective_origin: null,
    ...input,
  };
}

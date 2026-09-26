import type { GatewayProtocol } from "../generated/bindings";

export const NATIVE_GATEWAY_PROTOCOLS = [
  { key: "anthropic-messages", label: "Anthropic Messages" },
  { key: "openai-completions", label: "OpenAI Chat Completions" },
  { key: "openai-responses", label: "OpenAI Responses" },
  { key: "google-generative-ai", label: "Google Generative AI" },
] as const satisfies readonly { key: GatewayProtocol; label: string }[];

export function isNativeGatewayProtocol(value: unknown): value is GatewayProtocol {
  return NATIVE_GATEWAY_PROTOCOLS.some((protocol) => protocol.key === value);
}

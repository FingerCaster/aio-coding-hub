export type PluginPermissionRisk = "low" | "medium" | "high" | "critical";

export type ActiveGatewayHookName =
  | "gateway.request.afterBodyRead"
  | "gateway.request.beforeSend"
  | "gateway.response.chunk"
  | "gateway.response.after"
  | "gateway.error"
  | "log.beforePersist";

export type ReservedGatewayHookName =
  | "gateway.request.received"
  | "gateway.request.beforeProviderResolution"
  | "gateway.response.headers";

export type GatewayHookName = ActiveGatewayHookName | ReservedGatewayHookName;

export type PluginPermission =
  | "request.meta.read"
  | "request.header.read"
  | "request.header.readSensitive"
  | "request.header.write"
  | "request.body.read"
  | "request.body.write"
  | "response.header.read"
  | "response.header.write"
  | "response.body.read"
  | "response.body.write"
  | "stream.inspect"
  | "stream.modify"
  | "log.redact"
  | "plugin.storage"
  | "network.fetch"
  | "file.read"
  | "file.write"
  | "secret.read";

export type ExtensionRuntime = {
  kind: "extensionHost";
  language: "typescript";
};

export type PluginRuntime = ExtensionRuntime;

export type PluginHook = {
  name: GatewayHookName;
  priority?: number;
  failurePolicy?: "fail-open" | "fail-closed";
};

export type PluginHostCompatibility = {
  app: string;
  pluginApi: string;
  platforms?: string[];
};

export type ActivationEvent =
  | "onStartup"
  | `onCommand:${string}`
  | `onProviderEditor:${string}`
  | `onProtocolBridge:${string}`
  | `onGatewayHook:${string}`;

export type UiContributionSlot =
  | "app.sidebar.items"
  | "home.overview.cards"
  | "providers.editor.sections"
  | "providers.editor.fields"
  | "providers.card.badges"
  | "providers.card.actions"
  | "settings.sections"
  | "logs.detail.tabs"
  | "logs.detail.actions"
  | "usage.panels"
  | "plugins.detail.panels";

export type PluginCapability =
  | "commands.execute"
  | "storage.plugin"
  | "diagnostics.read"
  | "privacy.redact"
  | "provider.extensionValues"
  | "provider.requestPreparation"
  | "provider.modelDiscovery"
  | "provider.healthCheck"
  | "protocol.bridge"
  | "gateway.hooks";

export type HostRenderedField =
  | { type: "text"; key: string; label: string; placeholder?: string; required?: boolean }
  | { type: "password"; key: string; label: string; placeholder?: string; required?: boolean }
  | { type: "number"; key: string; label: string; min?: number; max?: number; step?: number }
  | { type: "boolean"; key: string; label: string }
  | { type: "select"; key: string; label: string; options: Array<{ value: string; label: string }> }
  | { type: "textarea"; key: string; label: string; rows?: number }
  | { type: "info"; key: string; label: string; value: string }
  | { type: "button"; key: string; label: string; command: string };

export type HostRenderedSchema =
  | { type: "section"; fields: HostRenderedField[] }
  | { type: "panel"; fields: HostRenderedField[] }
  | { type: "badge"; label: string; tone?: "neutral" | "success" | "warning" | "danger" };

export type UiContribution = {
  id: string;
  title?: string;
  order?: number;
  schema: HostRenderedSchema;
  when?: string;
};

export type ProviderContribution = {
  providerType: string;
  displayName: string;
  targetCliKeys: Array<"claude" | "codex" | "gemini">;
  extensionNamespace: string;
};

export type ProtocolContribution = {
  protocolId: string;
  direction: "inbound" | "outbound" | "both";
};

export type ProtocolBridgeContribution = {
  bridgeType: string;
  inboundProtocol: string;
  outboundProtocol: string;
  supportsStreaming?: boolean;
};

export type CommandContribution = {
  command: string;
  title: string;
  category?: string;
};

export type GatewayHookContribution = PluginHook;

export type PluginContributes = {
  providers?: ProviderContribution[];
  protocols?: ProtocolContribution[];
  protocolBridges?: ProtocolBridgeContribution[];
  commands?: CommandContribution[];
  gatewayHooks?: GatewayHookContribution[];
  ui?: Partial<Record<UiContributionSlot, UiContribution[]>>;
};

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue | undefined };

type PluginManifestBase = {
  id: string;
  name: string;
  version: string;
  apiVersion: string;
  hostCompatibility: PluginHostCompatibility;
  main?: string;
  activationEvents?: ActivationEvent[];
  contributes?: PluginContributes;
  capabilities?: PluginCapability[];
  entry?: string;
  configSchema?: JsonValue;
  configVersion?: number;
  description?: string;
  author?: JsonValue;
  homepage?: string;
  repository?: JsonValue;
  license?: string;
  checksum?: string;
  signature?: string;
  category?: string;
};

export type PluginManifest = PluginManifestBase & {
  runtime: ExtensionRuntime;
};

export type GatewayNormalizedMessage = {
  role: string;
  text: string;
  source: string;
};

export type GatewayVisibleRequestContext = {
  cliKey?: string;
  method?: string;
  path?: string;
  query?: string;
  headers?: Record<string, JsonValue>;
  body?: string;
  normalizedMessages?: GatewayNormalizedMessage[];
  requestedModel?: string;
};

export type GatewayVisibleResponseContext = {
  status?: number;
  headers?: Record<string, JsonValue>;
  body?: string;
};

export type GatewayVisibleStreamContext = {
  sequence?: number;
  chunk?: string;
};

export type GatewayVisibleLogContext = {
  message?: string;
};

export type GatewayVisibleHookContext = {
  request?: GatewayVisibleRequestContext;
  response?: GatewayVisibleResponseContext;
  stream?: GatewayVisibleStreamContext;
  log?: GatewayVisibleLogContext;
};

export type PluginHookContext = {
  hook: GatewayHookName;
  traceId?: string;
  config: JsonValue;
  context: GatewayVisibleHookContext;
};

export type PluginHookResult =
  | { action: "pass"; audit?: JsonValue[] }
  | { action: "warn"; message: string; audit?: JsonValue[] }
  | { action: "block"; reason: string; audit?: JsonValue[] }
  | {
      action: "replace";
      requestBody?: string;
      responseBody?: string;
      streamChunk?: string;
      logMessage?: string;
      headers?: Record<string, string>;
      audit?: JsonValue[];
    };

export type PrivacyRedactionOptions = {
  sensitiveTypes?: string[];
  redactionScopes?: string[];
};

export type PrivacyRedactionOutput = {
  hit: boolean;
  count: number;
  redacted: string;
};

export type PrivacyApi = {
  redactText(text: string, options?: PrivacyRedactionOptions): PrivacyRedactionOutput;
  redactRequestBody(body: string, options?: PrivacyRedactionOptions): PrivacyRedactionOutput;
};

export type GatewayApi = {
  registerHook(
    name: ActiveGatewayHookName,
    handler: (context: PluginHookContext) => PluginHookResult
  ): void;
};

export type CommandHandler = (args: JsonValue) => JsonValue;

export type CommandsApi = {
  registerCommand(command: string, handler: CommandHandler): void;
};

export type PluginApi = {
  commands?: CommandsApi;
  gateway?: GatewayApi;
  privacy?: PrivacyApi;
};

export type ValidationResult =
  | { ok: true }
  | { ok: false; error: { code: string; message: string } };

const PERMISSION_RISKS: Record<PluginPermission, PluginPermissionRisk> = {
  "request.meta.read": "low",
  "request.header.read": "medium",
  "request.header.readSensitive": "high",
  "request.header.write": "high",
  "request.body.read": "high",
  "request.body.write": "high",
  "response.header.read": "low",
  "response.header.write": "medium",
  "response.body.read": "high",
  "response.body.write": "high",
  "stream.inspect": "high",
  "stream.modify": "high",
  "log.redact": "medium",
  "plugin.storage": "medium",
  "network.fetch": "high",
  "file.read": "high",
  "file.write": "high",
  "secret.read": "critical",
};

const KNOWN_HOOKS = new Set<GatewayHookName>([
  "gateway.request.received",
  "gateway.request.afterBodyRead",
  "gateway.request.beforeProviderResolution",
  "gateway.request.beforeSend",
  "gateway.response.headers",
  "gateway.response.chunk",
  "gateway.response.after",
  "gateway.error",
  "log.beforePersist",
]);

const RESERVED_HOOKS = new Set<GatewayHookName>([
  "gateway.request.received",
  "gateway.request.beforeProviderResolution",
  "gateway.response.headers",
]);

const KNOWN_UI_SLOTS = new Set<UiContributionSlot>([
  "app.sidebar.items",
  "home.overview.cards",
  "providers.editor.sections",
  "providers.editor.fields",
  "providers.card.badges",
  "providers.card.actions",
  "settings.sections",
  "logs.detail.tabs",
  "logs.detail.actions",
  "usage.panels",
  "plugins.detail.panels",
]);

const KNOWN_CAPABILITIES = new Set<PluginCapability>([
  "commands.execute",
  "storage.plugin",
  "diagnostics.read",
  "privacy.redact",
  "provider.extensionValues",
  "provider.requestPreparation",
  "provider.modelDiscovery",
  "provider.healthCheck",
  "protocol.bridge",
  "gateway.hooks",
]);

const KNOWN_TARGET_CLI_KEYS = new Set(["claude", "codex", "gemini"]);
const KNOWN_PROTOCOL_DIRECTIONS = new Set(["inbound", "outbound", "both"]);
const KNOWN_UI_SCHEMA_TYPES = new Set(["section", "panel", "badge"]);
const KNOWN_UI_FIELD_TYPES = new Set([
  "text",
  "password",
  "number",
  "boolean",
  "select",
  "textarea",
  "info",
  "button",
]);
const KNOWN_BADGE_TONES = new Set(["neutral", "success", "warning", "danger"]);

export function permissionRisk(permission: PluginPermission): PluginPermissionRisk {
  return PERMISSION_RISKS[permission];
}

export function validateManifest(manifest: PluginManifest): ValidationResult {
  if (!/^[a-z0-9][a-z0-9-]*(\.[a-z0-9][a-z0-9-]*)+$/.test(manifest.id)) {
    return invalid("PLUGIN_INVALID_ID", "plugin id must look like publisher.plugin-name");
  }
  if (!isSemver(manifest.version)) {
    return invalid("PLUGIN_INVALID_VERSION", "version must be SemVer");
  }
  if (!isSemver(manifest.apiVersion)) {
    return invalid("PLUGIN_INVALID_API_VERSION", "apiVersion must be SemVer");
  }
  if (semverMajor(manifest.apiVersion) !== 1) {
    return invalid("PLUGIN_INCOMPATIBLE_API", "apiVersion must use plugin API major version 1");
  }

  const rawManifest = manifest as Record<string, unknown>;
  const runtime = asRecord(rawManifest.runtime);
  if (!runtime || runtime.kind !== "extensionHost") {
    return invalid(
      "PLUGIN_UNSUPPORTED_RUNTIME",
      "community plugins must use extensionHost runtime"
    );
  }
  if (hasOwn(rawManifest, "hooks")) {
    return invalid("PLUGIN_INVALID_MANIFEST", "top-level hooks are no longer supported");
  }
  if (hasOwn(rawManifest, "permissions")) {
    return invalid("PLUGIN_INVALID_MANIFEST", "top-level permissions are no longer supported");
  }
  if (!isSimpleCompatibilityRange(manifest.hostCompatibility.app)) {
    return invalid(
      "PLUGIN_INVALID_HOST_COMPATIBILITY",
      "hostCompatibility.app must be a non-empty simple SemVer range"
    );
  }
  if (!supportsPluginApiV1(manifest.hostCompatibility.pluginApi)) {
    return invalid(
      "PLUGIN_UNSUPPORTED_PLUGIN_API",
      "hostCompatibility.pluginApi must support plugin API v1"
    );
  }

  if (!manifest.main || manifest.main.trim() === "") {
    return invalid("PLUGIN_MISSING_MAIN", "extensionHost runtime requires main");
  }
  if (manifest.runtime.language !== "typescript") {
    return invalid("PLUGIN_INVALID_RUNTIME", "extensionHost language must be typescript");
  }
  const activationError = validateActivationEvents(manifest.activationEvents);
  if (activationError) return activationError;
  const contributes = manifest.contributes ?? {};
  const contributionError = validateContributes(contributes, manifest.id);
  if (contributionError) return contributionError;
  const capabilities = manifest.capabilities ?? [];
  const capabilityError = validateCapabilities(capabilities);
  if (!capabilityError.ok) return capabilityError;
  const dependencyError = validateCapabilityDependencies(contributes, capabilities);
  if (dependencyError) return dependencyError;
  return { ok: true };
}

function validateActivationEvents(activationEvents: unknown): ValidationResult | null {
  if (activationEvents == null) return null;
  if (!Array.isArray(activationEvents)) {
    return invalid("PLUGIN_INVALID_ACTIVATION_EVENT", "activationEvents must be an array");
  }

  for (const event of activationEvents) {
    if (event === "onStartup") continue;
    if (typeof event !== "string") {
      return invalid("PLUGIN_INVALID_ACTIVATION_EVENT", "activation event must be a string");
    }

    const hasKnownPrefix = [
      "onCommand:",
      "onProviderEditor:",
      "onProtocolBridge:",
      "onGatewayHook:",
    ].some((prefix) => event.startsWith(prefix) && event.slice(prefix.length).trim() !== "");
    if (!hasKnownPrefix) {
      return invalid("PLUGIN_INVALID_ACTIVATION_EVENT", `invalid activation event: ${event}`);
    }
  }
  return null;
}

function validateContributes(
  contributes: PluginContributes,
  pluginId: string
): ValidationResult | null {
  const raw = asRecord(contributes);
  if (!raw) {
    return invalid("PLUGIN_INVALID_CONTRIBUTES", "contributes must be an object");
  }
  for (const key of Object.keys(raw)) {
    if (
      !["providers", "protocols", "protocolBridges", "commands", "gatewayHooks", "ui"].includes(key)
    ) {
      return invalid("PLUGIN_INVALID_CONTRIBUTION", `unsupported contribution field: ${key}`);
    }
  }
  const providerError = validateProviderContributions(raw.providers);
  if (providerError) return providerError;
  const protocolError = validateProtocolContributions(raw.protocols);
  if (protocolError) return protocolError;
  const protocolBridgeError = validateProtocolBridgeContributions(raw.protocolBridges, pluginId);
  if (protocolBridgeError) return protocolBridgeError;
  const commandError = validateCommandContributions(raw.commands);
  if (commandError) return commandError;
  const gatewayHookError = validateGatewayHookContributions(raw.gatewayHooks);
  if (gatewayHookError) return gatewayHookError;
  return validateUiContributions(raw.ui);
}

function validateProviderContributions(providers: unknown): ValidationResult | null {
  if (providers == null) return null;
  if (!Array.isArray(providers)) {
    return invalid("PLUGIN_INVALID_PROVIDER_CONTRIBUTION", "providers must be an array");
  }
  for (const provider of providers) {
    const record = asRecord(provider);
    if (
      !record ||
      !isNonEmptyString(record.providerType) ||
      !isNonEmptyString(record.displayName) ||
      !isNonEmptyString(record.extensionNamespace) ||
      !Array.isArray(record.targetCliKeys) ||
      record.targetCliKeys.length === 0 ||
      !record.targetCliKeys.every(
        (key) => typeof key === "string" && KNOWN_TARGET_CLI_KEYS.has(key)
      )
    ) {
      return invalid(
        "PLUGIN_INVALID_PROVIDER_CONTRIBUTION",
        "provider contribution requires providerType, displayName, extensionNamespace, and targetCliKeys"
      );
    }
  }
  return null;
}

function validateProtocolContributions(protocols: unknown): ValidationResult | null {
  if (protocols == null) return null;
  if (!Array.isArray(protocols)) {
    return invalid("PLUGIN_INVALID_PROTOCOL_CONTRIBUTION", "protocols must be an array");
  }
  for (const protocol of protocols) {
    const record = asRecord(protocol);
    if (
      !record ||
      !isNonEmptyString(record.protocolId) ||
      typeof record.direction !== "string" ||
      !KNOWN_PROTOCOL_DIRECTIONS.has(record.direction)
    ) {
      return invalid(
        "PLUGIN_INVALID_PROTOCOL_CONTRIBUTION",
        "protocol contribution requires protocolId and direction"
      );
    }
  }
  return null;
}

function validateProtocolBridgeContributions(
  protocolBridges: unknown,
  pluginId: string
): ValidationResult | null {
  if (protocolBridges == null) return null;
  if (!Array.isArray(protocolBridges)) {
    return invalid(
      "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION",
      "protocolBridges must be an array"
    );
  }
  for (const bridge of protocolBridges) {
    const record = asRecord(bridge);
    if (
      !record ||
      !isNonEmptyString(record.bridgeType) ||
      !isNonEmptyString(record.inboundProtocol) ||
      !isNonEmptyString(record.outboundProtocol) ||
      (record.supportsStreaming !== undefined && typeof record.supportsStreaming !== "boolean")
    ) {
      return invalid(
        "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION",
        "protocol bridge contribution requires bridgeType, inboundProtocol, and outboundProtocol"
      );
    }
    if (!isNamespacedContributionId(pluginId, record.bridgeType)) {
      return invalid(
        "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION",
        "protocol bridge bridgeType must be lower-case and namespaced by plugin id"
      );
    }
  }
  return null;
}

function validateCommandContributions(commands: unknown): ValidationResult | null {
  if (commands == null) return null;
  if (!Array.isArray(commands)) {
    return invalid("PLUGIN_INVALID_COMMAND_CONTRIBUTION", "commands must be an array");
  }
  for (const command of commands) {
    const record = asRecord(command);
    if (!record || !isNonEmptyString(record.command) || !isNonEmptyString(record.title)) {
      return invalid(
        "PLUGIN_INVALID_COMMAND_CONTRIBUTION",
        "command contribution requires command and title"
      );
    }
  }
  return null;
}

function validateGatewayHookContributions(gatewayHooks: unknown): ValidationResult | null {
  if (gatewayHooks == null) return null;
  if (!Array.isArray(gatewayHooks)) {
    return invalid("PLUGIN_UNKNOWN_HOOK", "gatewayHooks must be an array");
  }
  for (const hook of gatewayHooks) {
    const record = asRecord(hook);
    if (!record || typeof record.name !== "string") {
      return invalid("PLUGIN_UNKNOWN_HOOK", "gateway hook contribution requires name");
    }
    if (RESERVED_HOOKS.has(record.name as GatewayHookName)) {
      return invalid(
        "PLUGIN_RESERVED_HOOK",
        `hook is reserved for a future host integration and is not active in plugin API v1: ${record.name}`
      );
    }
    if (!KNOWN_HOOKS.has(record.name as GatewayHookName)) {
      return invalid("PLUGIN_UNKNOWN_HOOK", `unknown hook: ${record.name}`);
    }
  }
  return null;
}

function validateUiContributions(ui: unknown): ValidationResult | null {
  if (ui == null) return null;
  const uiRecord = asRecord(ui);
  if (!uiRecord) {
    return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "ui contributions must be an object");
  }

  for (const [slot, contributions] of Object.entries(uiRecord)) {
    if (!KNOWN_UI_SLOTS.has(slot as UiContributionSlot)) {
      return invalid("PLUGIN_UNKNOWN_UI_SLOT", `unknown UI contribution slot: ${slot}`);
    }
    if (!Array.isArray(contributions)) {
      return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "UI slot contributions must be an array");
    }
    for (const contribution of contributions) {
      const contributionError = validateUiContribution(contribution);
      if (contributionError) return contributionError;
    }
  }
  return null;
}

function validateUiContribution(contribution: unknown): ValidationResult | null {
  const record = asRecord(contribution);
  if (!record || !isNonEmptyString(record.id)) {
    return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "UI contribution requires id");
  }
  const schema = asRecord(record.schema);
  if (!schema || typeof schema.type !== "string" || !KNOWN_UI_SCHEMA_TYPES.has(schema.type)) {
    return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "UI contribution requires supported schema");
  }
  if (schema.type === "section" || schema.type === "panel") {
    if (!Array.isArray(schema.fields)) {
      return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "section and panel schemas require fields");
    }
    for (const field of schema.fields) {
      const fieldError = validateHostRenderedField(field);
      if (fieldError) return fieldError;
    }
    return null;
  }
  if (!isNonEmptyString(schema.label)) {
    return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "badge schema requires label");
  }
  if (
    schema.tone !== undefined &&
    (typeof schema.tone !== "string" || !KNOWN_BADGE_TONES.has(schema.tone))
  ) {
    return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "badge schema tone is not supported");
  }
  return null;
}

function validateHostRenderedField(field: unknown): ValidationResult | null {
  const record = asRecord(field);
  if (
    !record ||
    typeof record.type !== "string" ||
    !KNOWN_UI_FIELD_TYPES.has(record.type) ||
    !isNonEmptyString(record.key) ||
    !isNonEmptyString(record.label)
  ) {
    return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "UI field requires type, key, and label");
  }
  if (record.type === "button" && !isNonEmptyString(record.command)) {
    return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "button field requires command");
  }
  if (record.type === "select") {
    if (!Array.isArray(record.options) || record.options.length === 0) {
      return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "select field requires options");
    }
    for (const option of record.options) {
      const optionRecord = asRecord(option);
      if (
        !optionRecord ||
        !isNonEmptyString(optionRecord.value) ||
        !isNonEmptyString(optionRecord.label)
      ) {
        return invalid("PLUGIN_INVALID_UI_CONTRIBUTION", "select option requires value and label");
      }
    }
  }
  return null;
}

function validateCapabilities(capabilities: readonly PluginCapability[]): ValidationResult {
  for (const capability of capabilities) {
    if (!KNOWN_CAPABILITIES.has(capability)) {
      return invalid("PLUGIN_UNKNOWN_CAPABILITY", `unknown capability: ${capability}`);
    }
  }
  return { ok: true };
}

function validateCapabilityDependencies(
  contributes: PluginContributes,
  capabilities: readonly PluginCapability[]
): ValidationResult | null {
  const capabilitySet = new Set(capabilities);
  const requireCapability = (needed: PluginCapability, reason: string) => {
    if (!capabilitySet.has(needed)) {
      return invalid("PLUGIN_MISSING_CAPABILITY", `${reason} requires ${needed}`);
    }
    return null;
  };

  if ((contributes.commands?.length ?? 0) > 0) {
    const error = requireCapability("commands.execute", "commands contribution");
    if (error) return error;
  }
  if ((contributes.providers?.length ?? 0) > 0) {
    const error = requireCapability("provider.extensionValues", "provider contribution");
    if (error) return error;
  }
  if ((contributes.gatewayHooks?.length ?? 0) > 0) {
    const error = requireCapability("gateway.hooks", "gatewayHooks contribution");
    if (error) return error;
  }
  if ((contributes.protocolBridges?.length ?? 0) > 0) {
    const error = requireCapability("protocol.bridge", "protocolBridges contribution");
    if (error) return error;
  }
  if ((contributes.ui?.["providers.editor.sections"]?.length ?? 0) > 0) {
    const error = requireCapability(
      "provider.extensionValues",
      "providers.editor.sections UI contribution"
    );
    if (error) return error;
  }
  if ((contributes.ui?.["providers.editor.fields"]?.length ?? 0) > 0) {
    const error = requireCapability(
      "provider.extensionValues",
      "providers.editor.fields UI contribution"
    );
    if (error) return error;
  }
  if (uiHasButtonCommand(contributes.ui)) {
    const error = requireCapability("commands.execute", "UI command field");
    if (error) return error;
  }
  return null;
}

function uiHasButtonCommand(ui: PluginContributes["ui"]): boolean {
  if (!ui) return false;
  return Object.values(ui).some((contributions) =>
    (contributions ?? []).some((contribution) => {
      const schema = contribution.schema;
      if (schema.type !== "section" && schema.type !== "panel") return false;
      return schema.fields.some((field) => field.type === "button");
    })
  );
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function hasOwn(value: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim() !== "";
}

function isNamespacedContributionId(pluginId: string, value: string): boolean {
  if (!isValidContributionId(value)) return false;
  if (value === pluginId) return true;
  if (!value.startsWith(pluginId)) return false;
  const suffix = value.slice(pluginId.length);
  return suffix.length > 1 && [".", "/", ":"].includes(suffix[0]);
}

function isValidContributionId(value: string): boolean {
  return /^[a-z0-9][a-z0-9-]*(?:[./:][a-z0-9][a-z0-9-]*)*$/.test(value);
}

function invalid(code: string, message: string): ValidationResult {
  return { ok: false, error: { code, message } };
}

function isSemver(value: string): boolean {
  return /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/.test(value);
}

function semverMajor(value: string): number | null {
  const major = /^(\d+)\./.exec(value)?.[1];
  return major == null ? null : Number.parseInt(major, 10);
}

function isSimpleCompatibilityRange(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed) return false;
  return /^(?:[<>=^~]*\s*\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?|\d+\.x\.x)(?:\s+(?:[<>=^~]*\s*\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?|\d+\.x\.x))*$/.test(
    trimmed
  );
}

function supportsPluginApiV1(value: string): boolean {
  const trimmed = value.trim();
  return trimmed === "^1.0.0" || trimmed === "1.x.x" || trimmed === ">=1.0.0 <2.0.0";
}

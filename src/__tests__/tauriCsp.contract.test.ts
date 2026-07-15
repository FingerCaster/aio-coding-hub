import { describe, expect, it } from "vitest";

const tauriConfigSources = import.meta.glob("../../src-tauri/tauri.conf.json", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function extractFrameSrcTokens(csp: string) {
  const directive = csp
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("frame-src "));
  if (!directive) return [];
  return directive
    .slice("frame-src ".length)
    .split(/\s+/u)
    .map((value) => value.trim())
    .filter(Boolean);
}

describe("tauri csp contract", () => {
  it("limits frame-src to loopback origins for the embedded gateway page", () => {
    const [source] = Object.values(tauriConfigSources);
    const config = JSON.parse(source) as {
      app: { security: { csp: string } };
    };

    expect(extractFrameSrcTokens(config.app.security.csp)).toEqual([
      "http://127.0.0.1:*",
      "http://localhost:*",
      "http://[::1]:*",
    ]);
  });
});

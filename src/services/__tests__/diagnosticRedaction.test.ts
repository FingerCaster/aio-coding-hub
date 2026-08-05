import { describe, expect, it } from "vitest";
import {
  diagnosticStringMetadata,
  redactDiagnosticJsonText,
  redactDiagnosticText,
  redactDiagnosticValue,
  sanitizeDiagnosticUrl,
} from "../diagnosticRedaction";

function sentinel() {
  return `sentinel-${crypto.randomUUID().replace(/-/g, "")}`;
}

describe("services/diagnosticRedaction", () => {
  it.each([
    (secret: string) => `Authorization: Bearer ${secret}`,
    (secret: string) => `api_key=${secret}`,
    (secret: string) => `password: "${secret}"`,
    (secret: string) => `client-secret='${secret}'`,
    (secret: string) => `token=${secret}`,
    (secret: string) => `session_token=${secret}`,
    (secret: string) => `password="${secret}`,
    (secret: string) => JSON.stringify({ access_token: secret }),
    (secret: string) => `request failed for https://example.test/path?token=${secret}#${secret}`,
    (secret: string) => `sk-proj-${secret}`,
  ])("redacts credential patterns from diagnostic text", (makeText) => {
    const secret = sentinel();
    const redacted = redactDiagnosticText(makeText(secret));

    expect(redacted).not.toContain(secret);
    expect(redacted).toContain("[REDACTED]");
  });

  it("enforces aggregate traversal and output budgets", () => {
    const makeBranch = (depth: number): Record<string, unknown> => {
      const branch: Record<string, unknown> = {};
      for (let index = 0; index < 20; index += 1) {
        Object.defineProperty(branch, `key_${index}`, {
          enumerable: true,
          get: () => (depth > 0 ? makeBranch(depth - 1) : "x".repeat(200)),
        });
      }
      return branch;
    };

    const redacted = redactDiagnosticValue(makeBranch(4), {
      maxNodes: 50,
      maxTotalStringChars: 200,
    });
    const serialized = JSON.stringify(redacted);

    expect(serialized.length).toBeLessThan(5000);
    expect(serialized).toContain("[Truncated]");
  });

  it("redacts nested and hostile structured values with bounded traversal", () => {
    const secret = sentinel();
    const payload: Record<string, unknown> = {
      nested: {
        password: secret,
        message: `Authorization: Bearer ${secret}`,
      },
      url: `https://example.test/path?api_key=${secret}#${secret}`,
    };
    payload.self = payload;
    Object.defineProperty(payload, "throwing", {
      enumerable: true,
      get() {
        throw new Error(secret);
      },
    });

    const redacted = redactDiagnosticValue(payload);
    const serialized = JSON.stringify(redacted);

    expect(serialized).not.toContain(secret);
    expect(serialized).toContain("[REDACTED]");
    expect(serialized).toContain("[Circular]");
    expect(serialized).toContain("[REDACTION_FAILED]");
  });

  it("summarizes opaque IPC strings while preserving safe categories and identifiers", () => {
    const secret = sentinel();
    const providerId = crypto.randomUUID();
    const redacted = redactDiagnosticValue(
      {
        text: secret,
        content: `prompt ${secret}`,
        source: "codex",
        providerId,
        nested: { label: "ordinary text" },
      },
      { stringMode: "metadata" }
    ) as Record<string, unknown>;

    expect(JSON.stringify(redacted)).not.toContain(secret);
    expect(redacted.text).toBe(diagnosticStringMetadata(secret));
    expect(redacted.content).toBe(diagnosticStringMetadata(`prompt ${secret}`));
    expect(redacted.source).toBe("codex");
    expect(redacted.providerId).toBe(providerId);
    expect((redacted.nested as Record<string, unknown>).label).toBe(
      diagnosticStringMetadata("ordinary text")
    );
  });

  it("redacts JSON text structurally and strips URL credentials, query, and fragment", () => {
    const secret = sentinel();
    const json = redactDiagnosticJsonText(JSON.stringify({ nested: { secret }, safe: "kept" }));
    const url = sanitizeDiagnosticUrl(
      `https://user:${secret}@example.test/path/to?q=${secret}#${secret}`
    );

    expect(json).not.toContain(secret);
    expect(json).toContain("[REDACTED]");
    expect(url).toBe("https://example.test/path/to");
  });
});

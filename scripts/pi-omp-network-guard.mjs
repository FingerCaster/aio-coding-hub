// Preloaded before the unmodified published CLI. Only the fixture listener may be contacted.
import http from "node:http";
import https from "node:https";
import net from "node:net";
import { appendFileSync } from "node:fs";
import { syncBuiltinESMExports } from "node:module";

const allowedOrigin = process.env.PI_OMP_FIXTURE_ORIGIN;
function reject(target) {
  const safe = String(target).replace(/([?&](?:key|token|api_key)=)[^&]*/gi, "$1[redacted]");
  if (process.env.PI_OMP_GUARD_LOG)
    appendFileSync(process.env.PI_OMP_GUARD_LOG, JSON.stringify({ blocked: safe }) + "\n");
  throw new Error(`W0 blocked non-fixture network: ${safe}`);
}
function check(value) {
  let url;
  try {
    url = new URL(value);
  } catch {
    reject(value);
  }
  if (!allowedOrigin || url.origin !== allowedOrigin) reject(url.origin);
}
const originalFetch = globalThis.fetch;
globalThis.fetch = (input, options) => {
  check(typeof input === "string" || input instanceof URL ? input : input.url);
  return originalFetch(input, { ...options, redirect: "error" });
};
for (const [module, scheme] of [
  [http, "http:"],
  [https, "https:"],
]) {
  for (const method of ["request", "get"]) {
    const original = module[method];
    module[method] = function (input, ...args) {
      if (typeof input === "string" || input instanceof URL) check(input);
      else
        check(
          `${input.protocol || scheme}//${input.hostname || input.host || "localhost"}:${input.port || (scheme === "https:" ? 443 : 80)}`
        );
      return original.call(this, input, ...args);
    };
  }
}
const originalConnect = net.Socket.prototype.connect;
net.Socket.prototype.connect = function (...args) {
  const first = Array.isArray(args[0]) ? args[0][0] : args[0];
  if (first && typeof first === "object" && first.port) {
    check(`http://${first.host || "localhost"}:${first.port}`);
  } else if (typeof first === "number") {
    check(`http://${typeof args[1] === "string" ? args[1] : "localhost"}:${first}`);
  }
  return originalConnect.apply(this, args);
};
syncBuiltinESMExports();

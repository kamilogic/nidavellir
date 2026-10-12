import { invoke } from "@tauri-apps/api/core";

// Slow/disconnected polling shares the current read instead of accumulating pipe clients.
// Mutations are never coalesced or retried: a lost response has an unknown outcome.
const pendingReads = new Map();

export async function serviceCall(method, params = null) {
  const readOnly = method.startsWith("Get") || ["ReadSensors", "DetectHardware", "Ping"].includes(method);
  const key = JSON.stringify([method, params]);
  if (readOnly && pendingReads.has(key)) return pendingReads.get(key);
  const request = params == null
    ? invoke("service_request", { method })
    : invoke("service_request", { method, params });
  if (!readOnly) return request;
  const pending = request.finally(() => pendingReads.delete(key));
  pendingReads.set(key, pending);
  return pending;
}

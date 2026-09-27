import test from "node:test";
import assert from "node:assert/strict";
import { serviceCall } from "./service.js";

test("slow polling shares pending reads, releases failed reads, and never retries a mutation", async () => {
  const calls = [];
  let reject;
  globalThis.window = { __TAURI_INTERNALS__: { invoke: (_command, args) => {
    calls.push(args.method);
    return new Promise((_, fail) => { reject = fail; });
  } } };
  const first = serviceCall("ReadSensors");
  const second = serviceCall("ReadSensors");
  assert.deepEqual(calls, ["ReadSensors"]);
  reject(new Error("response timed out; action outcome unknown"));
  assert.deepEqual((await Promise.allSettled([first, second])).map((r) => r.status), ["rejected", "rejected"]);
  const retry = serviceCall("ReadSensors");
  assert.equal(calls.length, 2);
  reject(new Error("offline"));
  await assert.rejects(retry, /offline/);
  const mutation = serviceCall("ResetGpuTuning");
  reject(new Error("action outcome unknown"));
  await assert.rejects(mutation, /outcome unknown/);
  assert.deepEqual(calls, ["ReadSensors", "ReadSensors", "ResetGpuTuning"]);
  delete globalThis.window;
});

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { compareVersions, parseNotes, releasesSince } from "./release-notes.js";

test("versions compare numerically", () => {
  assert.equal(compareVersions("0.5.10", "0.5.9"), 1);
  assert.equal(compareVersions("0.5.2", "0.5.2"), 0);
  assert.equal(compareVersions("0.4.9", "0.5"), -1);
});

test("after an update, every release since the previous version shows, newest first", () => {
  const files = { "x/v0.5.0.md": "Zero.", "x/v0.5.1.md": "One.", "x/v0.5.2.md": "Two.", "x/v0.5.3.md": "Three." };
  assert.deepEqual(releasesSince("0.5.0", "0.5.2", files).map((release) => release.version), ["0.5.2", "0.5.1"]);
  assert.deepEqual(releasesSince(null, "0.5.2", files).map((release) => release.version), ["0.5.2"]);
  assert.deepEqual(releasesSince("0.5.2", "0.5.2", files), []);
  assert.deepEqual(releasesSince("0.5.0", "0.5.1", files)[0].notes.summary, ["One."]);
});

test("release notes split into summary, changes, fixes and closing notes", () => {
  const notes = parseNotes(readFileSync(new URL("../../../../docs/release-notes/v0.5.1.md", import.meta.url), "utf8"));
  assert.deepEqual(notes.summary, ["Nidavellir now lives in the tray, so your profile stays active while you play."]);
  assert.equal(notes.changes.length, 5);
  assert.deepEqual(notes.fixes, [
    "A console window opened next to Nidavellir.",
    "Long status words overlapped the divider on the Forge screen.",
  ]);
  assert.equal(notes.closing.length, 1);
});

test("unknown shapes still render as text, never as markup", () => {
  assert.deepEqual(parseNotes("Faster checks.\nClearer profile cards."), {
    summary: ["Faster checks.", "Clearer profile cards."], changes: [], fixes: [], closing: [],
  });
  assert.deepEqual(parseNotes(null), { summary: [], changes: [], fixes: [], closing: [] });
  assert.deepEqual(parseNotes("* <b>bold</b>").changes, ["<b>bold</b>"]);
});

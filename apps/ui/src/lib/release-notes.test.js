import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { parseNotes, releaseDate } from "./release-notes.js";

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

test("release date reads the updater's RFC 3339 value", () => {
  assert.equal(releaseDate("2026-10-03T20:31:52Z"), "Oct 3, 2026");
  assert.equal(releaseDate(null), null);
  assert.equal(releaseDate("soon"), null);
});

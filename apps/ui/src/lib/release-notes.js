/**
 * `docs/release-notes/vX.Y.Z.md` is plain text, one paragraph or bullet per line: a summary, then
 * "- " bullets (a bullet starting with "Fixed:" is a fix), then optional closing notes.
 * Unknown shapes still render: every line lands in one of the groups, never as markup.
 */
export function parseNotes(text = "") {
  const notes = { summary: [], changes: [], fixes: [], closing: [] };
  let afterList = false;
  for (const raw of String(text ?? "").split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    const bullet = line.match(/^[-*•]\s+(.+)$/);
    if (!bullet) {
      (afterList ? notes.closing : notes.summary).push(line);
      continue;
    }
    afterList = true;
    const fix = bullet[1].match(/^fix(?:ed)?:\s*(.+)$/i);
    if (fix) notes.fixes.push(fix[1].charAt(0).toUpperCase() + fix[1].slice(1));
    else notes.changes.push(bullet[1]);
  }
  return notes;
}

/** -1, 0 or 1 for dotted numeric versions: "0.5.10" is newer than "0.5.9". */
export function compareVersions(a, b) {
  const left = String(a).split(".").map(Number);
  const right = String(b).split(".").map(Number);
  for (let index = 0; index < Math.max(left.length, right.length); index++) {
    const difference = (left[index] || 0) - (right[index] || 0);
    if (difference) return Math.sign(difference);
  }
  return 0;
}

/**
 * The notes to show after an update, newest first: every release newer than `previous` up to
 * `current`. Without a previous version (installed before it was recorded) only `current`.
 * `files` maps a notes path ending in "vX.Y.Z.md" to its text.
 */
export function releasesSince(previous, current, files) {
  return Object.entries(files)
    .map(([path, text]) => ({ version: path.match(/v(\d+(?:\.\d+)*)\.md$/)?.[1], text }))
    .filter(({ version }) => version && compareVersions(version, current) <= 0 &&
      (previous ? compareVersions(version, previous) > 0 : compareVersions(version, current) === 0))
    .sort((a, b) => compareVersions(b.version, a.version))
    .map(({ version, text }) => ({ version, notes: parseNotes(text) }));
}

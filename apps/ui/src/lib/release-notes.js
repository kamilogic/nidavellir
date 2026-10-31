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

/**
 * -1, 0 or 1 for versions as semver orders them: "0.5.10" is newer than "0.5.9", and a
 * pre-release such as "0.8.21-diag.1" is newer than "0.8.20" but older than "0.8.21".
 */
export function compareVersions(a, b) {
  const [left, leftPre] = String(a).split("-", 2);
  const [right, rightPre] = String(b).split("-", 2);
  const core = compareDotted(left, right);
  if (core || leftPre === rightPre) return core;
  // A release is newer than its own pre-releases.
  if (!leftPre || !rightPre) return leftPre ? -1 : 1;
  return compareDotted(leftPre, rightPre);
}

/** Numeric parts compare as numbers ("diag.10" after "diag.9"), others as text; missing is 0. */
function compareDotted(a, b) {
  const left = a.split(".");
  const right = b.split(".");
  for (let index = 0; index < Math.max(left.length, right.length); index++) {
    const [x, y] = [left[index] ?? "0", right[index] ?? "0"];
    const difference = /^\d+$/.test(x) && /^\d+$/.test(y) ? Number(x) - Number(y) : x === y ? 0 : x < y ? -1 : 1;
    if (difference) return Math.sign(difference);
  }
  return 0;
}

/**
 * The notes to show after an update, newest first: every release newer than `previous` up to
 * `current`. Without a previous version (installed before it was recorded) only `current`.
 * `files` maps a notes path ending in "vX.Y.Z.md" or "vX.Y.Z-diag.N.md" to its text.
 */
export function releasesSince(previous, current, files) {
  return Object.entries(files)
    .map(([path, text]) => ({ version: path.match(/v(\d+(?:\.\d+)*(?:-[0-9A-Za-z.]+)?)\.md$/)?.[1], text }))
    .filter(({ version }) => version && compareVersions(version, current) <= 0 &&
      (previous ? compareVersions(version, previous) > 0 : compareVersions(version, current) === 0))
    .sort((a, b) => compareVersions(b.version, a.version))
    .map(({ version, text }) => ({ version, notes: parseNotes(text) }));
}

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

/** "Oct 3, 2026" from the updater's RFC 3339 date; null when absent or unreadable. */
export function releaseDate(date) {
  const time = Date.parse(date ?? "");
  return Number.isNaN(time) ? null : new Intl.DateTimeFormat("en", { dateStyle: "medium" }).format(time);
}

/** Every release's notes ship inside the app, so the What's new window works offline. */
export const bundledNotes = import.meta.glob("../../../../docs/release-notes/v*.md", {
  query: "?raw",
  import: "default",
  eager: true,
});

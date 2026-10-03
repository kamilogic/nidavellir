# Releasing Nidavellir (installer + in-app updates)

Installed apps check `https://github.com/kamilogic/nidavellir/releases/latest/download/latest.json`
when they start, and every 6 hours while open.

When a newer version exists, the user sees:
- a window with the version and the release notes ("What's new");
- the choice between **Update now** and **Later**. After Later, an "Update x.y.z" button stays in
  the corner.

Updating runs the signed NSIS installer in passive mode:
- Windows asks for admin permission.
- The Core Service stops, so the GPU returns to stock.
- The service is replaced and restarted.
- The app reopens.

Install is blocked while a Forge run is active. A paused or interrupted run cannot resume on a new
build; the dialog says so. Development builds (`tauri dev`) never offer an install.

GitHub's `latest` ignores drafts and pre-releases. Nothing reaches users until a draft release is
published.

## One-time setup: the signing key

The updater installs only installers signed with the private key that matches the public key
built into the app. Keep the private key and its password backed up: without them, installed apps
can never update again (users would have to reinstall by hand).

1. Generate the key pair. It asks for a password:
   ```powershell
   cd apps\ui
   npm run tauri signer generate -- -w "$env:USERPROFILE\.tauri\nidavellir.key"
   ```
2. Put the content of `nidavellir.key.pub` in `apps/ui/src-tauri/tauri.conf.json` →
   `plugins.updater.pubkey`, and commit it. The public key is not secret.
3. Give CI the private key and its password:
   ```powershell
   Get-Content "$env:USERPROFILE\.tauri\nidavellir.key" -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY
   gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
   ```
4. For a local release build, set `TAURI_SIGNING_PRIVATE_KEY` (the path or the content of the key)
   and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the shell. Then run `scripts/build-full-release.ps1`.
   The script refuses to build without them.

## Each release

1. Bump the version. `apps/ui/src-tauri/tauri.conf.json` is the version users see; keep the other
   manifests aligned.
2. Write `docs/release-notes/vX.Y.Z.md` in plain language. This exact text is what users read in
   the update window.
3. Commit, then tag and push the tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. CI (`.github/workflows/release.yml`) refuses to build when any of these is wrong:
   - the tag and the app version differ;
   - the public key is empty;
   - the notes file is missing;
   - an `@tauri-apps/*` npm package and its Rust crate differ in major.minor (the Tauri CLI checks
     this). `apps/ui/package.json` pins them with `~`; update both sides together.
   Otherwise it builds the signed installer, writes `latest.json`, and creates a **draft** release.
5. Review the draft and publish it. Installed apps offer the update at their next check.

Users on a version without the updater (0.3.x and older) install the new installer by hand once.

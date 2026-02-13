# Best Practices

## Security

1. **Signature verification**: tauri-plugin-updater uses Ed25519 signatures. Every downloaded artifact is verified against the public key in tauri.conf.json before applying.
2. **Private key management**: The signing private key (`~/.tauri/openstt.key`) must never be committed to the repository. Set via `TAURI_SIGNING_PRIVATE_KEY` environment variable during builds.
3. **HTTPS only**: The updater enforces HTTPS for all endpoints in production.
4. **No user bypass**: Users cannot skip signature verification — the plugin rejects unsigned or invalid artifacts automatically.

## Performance

1. **Async startup check**: The update check runs in the background after the app UI is rendered, not blocking startup.
2. **No polling**: We only check on startup and when the user manually requests. No periodic background polling.
3. **Progress tracking**: Use the `downloadAndInstall` callback to show real-time progress without polling.

## UX

1. **Non-intrusive**: Startup check is silent. No modal dialog for available updates — just a subtle indicator in Settings.
2. **Clear progress**: Show download percentage with a progress bar during installation.
3. **Error recovery**: All errors show a "Retry" option. Never leave the user in a stuck state.
4. **No interruption during dictation**: The update download runs in the background; restart only happens when the user explicitly triggers it.

## Release Process

1. **Signing consistency**: Always set `TAURI_SIGNING_PRIVATE_KEY` before `npm run tauri build`. If forgotten, the build won't generate `.sig` files and auto-updater users won't be able to update.
2. **Manifest accuracy**: The `latest.json` version must exactly match the git tag and tauri.conf.json version.
3. **Test before release**: After creating a GitHub Release, verify `latest.json` is accessible at `https://github.com/lulucatdev/openstt/releases/latest/download/latest.json`.
4. **DMG still needed**: Keep uploading DMGs for first-time users who don't have the app installed yet.

## Code Quality

1. **Minimal backend code**: The updater is entirely handled by the tauri-plugin-updater plugin. No custom Rust commands needed for check/download/install.
2. **Frontend-only logic**: All update UI state management lives in App.tsx, following existing patterns (similar to model download progress tracking).
3. **Reuse existing UI patterns**: Use the same `button`, `progress-bar`, `settings-row` CSS classes already in the app.
4. **i18n coverage**: Every new user-visible string must have both English and Chinese translations in the `translations` object.

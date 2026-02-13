# Auto-Updater Feature Design

## Context

OpenSTT is a macOS-only (Apple Silicon) Tauri v2 desktop app distributed via GitHub Releases as a signed and notarized DMG. Currently users must manually download and install new versions. We want to add automatic update checking and one-click in-app upgrades.

## Requirements

1. **Auto-check on startup**: App silently checks GitHub Releases on launch; if a new version is available, show an indicator in Settings.
2. **Manual check**: "Check for Updates" button in Settings > About section.
3. **One-click install**: User clicks "Install Update" -> download with progress -> automatic restart.
4. **Error handling**: Graceful handling of network failures, timeouts, and download errors.
5. **i18n**: All update-related strings in both English and Chinese.
6. **Release process**: Continue manual release flow, update `release-guide.md` with new steps for generating updater artifacts.

## Rationale

### Why tauri-plugin-updater?

- Official Tauri v2 plugin with built-in signature verification
- Replaces app binary in-place (no DMG mounting needed for users)
- `tauri build` auto-generates `.app.tar.gz` + `.sig` when configured
- Minimal code — frontend JS API handles check/download/install

### Why static `latest.json` on GitHub Releases?

- Zero infrastructure cost
- Aligns with existing `gh release create` workflow
- Just one extra file to upload per release
- Endpoint: `https://github.com/lulucatdev/openstt/releases/latest/download/latest.json`

## Detailed Design

### Backend Changes (Rust)

1. **Add dependencies**:
   - `tauri-plugin-updater = "2"` in Cargo.toml
   - `tauri-plugin-process = "2"` in Cargo.toml (for `relaunch()`)

2. **Register plugins** in `lib.rs` `run()` function:
   ```rust
   .plugin(tauri_plugin_updater::Builder::new().build())
   .plugin(tauri_plugin_process::init())
   ```

3. **Configure tauri.conf.json**:
   ```json
   {
     "bundle": {
       "createUpdaterArtifacts": true
     },
     "plugins": {
       "updater": {
         "pubkey": "<GENERATED_PUBLIC_KEY>",
         "endpoints": [
           "https://github.com/lulucatdev/openstt/releases/latest/download/latest.json"
         ]
       }
     }
   }
   ```

4. **Add permissions** to `capabilities/default.json`:
   ```json
   "updater:default",
   "process:default"
   ```

### Frontend Changes (React/TypeScript)

1. **Add npm packages**: `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process`

2. **State management** — new state variables in App.tsx:
   ```typescript
   const [updateAvailable, setUpdateAvailable] = useState<{
     version: string;
     notes?: string;
   } | null>(null);
   const [updateChecking, setUpdateChecking] = useState(false);
   const [updateDownloading, setUpdateDownloading] = useState(false);
   const [updateProgress, setUpdateProgress] = useState(0);
   const [updateError, setUpdateError] = useState<string | null>(null);
   ```

3. **Auto-check on startup** — in `useEffect` during app init:
   ```typescript
   import { check } from '@tauri-apps/plugin-updater';

   // Silent check, errors swallowed
   try {
     const update = await check();
     if (update) {
       setUpdateAvailable({ version: update.version, notes: update.body });
     }
   } catch (_) { /* silent */ }
   ```

4. **Manual check** — "Check for Updates" button handler:
   ```typescript
   const handleCheckUpdate = async () => {
     setUpdateChecking(true);
     setUpdateError(null);
     try {
       const update = await check();
       if (update) {
         setUpdateAvailable({ version: update.version, notes: update.body });
       } else {
         // Show "up to date" message (transient)
       }
     } catch (err) {
       setUpdateError(String(err));
     } finally {
       setUpdateChecking(false);
     }
   };
   ```

5. **Install update** — download with progress + restart:
   ```typescript
   import { relaunch } from '@tauri-apps/plugin-process';

   const handleInstallUpdate = async () => {
     setUpdateDownloading(true);
     setUpdateProgress(0);
     let totalBytes = 0;
     let downloadedBytes = 0;
     try {
       const update = await check();
       if (!update) return;
       await update.downloadAndInstall((event) => {
         if (event.event === 'Started' && event.data.contentLength) {
           totalBytes = event.data.contentLength;
         } else if (event.event === 'Progress') {
           downloadedBytes += event.data.chunkLength;
           if (totalBytes > 0) {
             setUpdateProgress(Math.round((downloadedBytes / totalBytes) * 100));
           }
         }
       });
       await relaunch();
     } catch (err) {
       setUpdateError(String(err));
       setUpdateDownloading(false);
     }
   };
   ```

6. **UI in Settings > About section**:
   - Fix hardcoded version `0.1.0` -> use `getVersion()` from `@tauri-apps/api/app`
   - Add "Check for Updates" button next to version display
   - When update available: show banner with version + notes + "Install Update" button
   - During download: show progress bar (reuse existing `progress-bar` CSS class)
   - On error: show error message with retry option

7. **i18n additions**:
   ```typescript
   // English
   checkForUpdates: "Check for Updates",
   checking: "Checking...",
   upToDate: "You're up to date",
   updateAvailable: "Update available: v{version}",
   installUpdate: "Install Update",
   downloading: "Downloading update... {percent}%",
   updateFailed: "Update failed",
   retry: "Retry",

   // Chinese
   checkForUpdates: "检查更新",
   checking: "检查中...",
   upToDate: "已是最新版本",
   updateAvailable: "有新版本: v{version}",
   installUpdate: "安装更新",
   downloading: "正在下载更新... {percent}%",
   updateFailed: "更新失败",
   retry: "重试",
   ```

### Release Process Changes

1. **One-time setup**: Generate signing keypair:
   ```bash
   npx tauri signer generate -w ~/.tauri/openstt.key
   ```
   Store public key in `tauri.conf.json`, private key secure locally.

2. **Build with signing** (add to release flow):
   ```bash
   export TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/openstt.key)
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="<password>"
   npm run tauri build
   ```
   This generates additional artifacts:
   - `src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz`
   - `src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz.sig`

3. **Generate latest.json** (new step after notarization):
   ```bash
   VERSION=1.0.8
   SIG=$(cat src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz.sig)
   cat > releases/latest.json <<EOF
   {
     "version": "$VERSION",
     "notes": "Release notes here",
     "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
     "platforms": {
       "darwin-aarch64": {
         "signature": "$SIG",
         "url": "https://github.com/lulucatdev/openstt/releases/download/v${VERSION}/OpenSTT.app.tar.gz"
       }
     }
   }
   EOF
   ```

4. **Upload to GitHub Release** (add to `gh release create`):
   ```bash
   gh release create "v$VERSION" \
     "$DMG_PATH" \
     "src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz" \
     "releases/latest.json" \
     --title "OpenSTT v$VERSION" \
     --notes "..."
   ```

### Files to Modify

| File | Change |
|------|--------|
| `src-tauri/Cargo.toml` | Add `tauri-plugin-updater`, `tauri-plugin-process` |
| `src-tauri/tauri.conf.json` | Add `bundle.createUpdaterArtifacts`, `plugins.updater` |
| `src-tauri/capabilities/default.json` | Add `updater:default`, `process:default` |
| `src-tauri/src/lib.rs` | Register updater + process plugins |
| `package.json` | Add `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process` |
| `src/App.tsx` | Add update check logic, UI components, i18n strings |
| `src/App.css` | Add update banner/progress styles (if needed) |
| `release-guide.md` | Add signing keypair setup, updater artifact generation, latest.json steps |

## Design Documents

- [BDD Specifications](./bdd-specs.md) - Behavior scenarios and testing strategy
- [Architecture](./architecture.md) - System architecture and component details
- [Best Practices](./best-practices.md) - Security, performance, and code quality guidelines

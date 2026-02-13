# Architecture

## Component Diagram

```
┌─────────────────────────────────────────────────┐
│                   Frontend (React)               │
│                                                  │
│  Settings > About Section                        │
│  ┌────────────────────────────────────────────┐  │
│  │  Version: 1.0.7  [Check for Updates]       │  │
│  │                                            │  │
│  │  ┌── Update Banner (conditional) ────────┐ │  │
│  │  │  Update available: v1.0.8             │ │  │
│  │  │  Release notes...                     │ │  │
│  │  │  [Install Update]  [progress bar]     │ │  │
│  │  └──────────────────────────────────────┘ │  │
│  └────────────────────────────────────────────┘  │
│                                                  │
│  @tauri-apps/plugin-updater  (check/download)    │
│  @tauri-apps/plugin-process  (relaunch)          │
└──────────────────┬───────────────────────────────┘
                   │ JS Plugin API
┌──────────────────▼───────────────────────────────┐
│                   Tauri Core                      │
│                                                  │
│  tauri-plugin-updater                            │
│  ├── Fetches latest.json from endpoint           │
│  ├── Compares version (semver)                   │
│  ├── Downloads .app.tar.gz                       │
│  ├── Verifies signature (Ed25519 pubkey)         │
│  └── Replaces app binary in-place                │
│                                                  │
│  tauri-plugin-process                            │
│  └── relaunch() after install                    │
└──────────────────┬───────────────────────────────┘
                   │ HTTPS
┌──────────────────▼───────────────────────────────┐
│              GitHub Releases                      │
│                                                  │
│  /releases/latest/download/latest.json           │
│  /releases/download/v1.0.8/OpenSTT.app.tar.gz   │
│  /releases/download/v1.0.8/OpenSTT_*.dmg        │
└──────────────────────────────────────────────────┘
```

## Data Flow

### Update Check Flow

```
App Launch
    │
    ▼
check() ──── HTTPS GET ──── latest.json
    │                            │
    ▼                            ▼
Compare versions          { version, platforms, notes }
    │
    ├── Same version ──── No UI change
    │
    └── Newer version ──── setUpdateAvailable({ version, notes })
                                │
                                ▼
                          Show banner in Settings
```

### Install Flow

```
User clicks "Install Update"
    │
    ▼
check() ── get Update object
    │
    ▼
downloadAndInstall(onEvent)
    │
    ├── Started { contentLength }  ──── Init progress bar
    ├── Progress { chunkLength }   ──── Update progress %
    └── Finished                   ──── Show "Installing..."
    │
    ▼
relaunch() ── App restarts with new version
```

## Update Manifest Schema

`latest.json` hosted at GitHub Releases:

```json
{
  "version": "1.0.8",
  "notes": "## OpenSTT v1.0.8\n\n- Fix: ...\n- Feature: ...",
  "pub_date": "2025-02-14T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<content of .app.tar.gz.sig>",
      "url": "https://github.com/lulucatdev/openstt/releases/download/v1.0.8/OpenSTT.app.tar.gz"
    }
  }
}
```

## Release Artifact Changes

Before (current):
```
GitHub Release v1.0.7
└── OpenSTT_1.0.7_aarch64.dmg     (for manual install)
```

After (with updater):
```
GitHub Release v1.0.8
├── OpenSTT_1.0.8_aarch64.dmg     (for manual install / first-time users)
├── OpenSTT.app.tar.gz             (for auto-updater, signed)
└── latest.json                    (update manifest)
```

## Plugin Registration

In `lib.rs` `run()` function, plugins are registered alongside existing ones:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .plugin(tauri_plugin_updater::Builder::new().build())  // NEW
    .plugin(tauri_plugin_process::init())                   // NEW
    // ... rest of setup
```

## Security Model

- **Signing keypair**: Ed25519 key generated via `tauri signer generate`
- **Private key**: Stored locally at `~/.tauri/openstt.key`, set via `TAURI_SIGNING_PRIVATE_KEY` env var during build
- **Public key**: Embedded in `tauri.conf.json` — app verifies every downloaded artifact against this key
- **Transport**: HTTPS enforced (GitHub Releases)
- **Verification**: Plugin rejects any artifact whose signature doesn't match the pubkey

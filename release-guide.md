# OpenSTT Release Guide

## Release 流程

### 第 0 步：确定版本号和版本说明

- 如果用户指定了版本号和说明，直接使用。
- 如果未指定：
  - 版本号 = 上一版本最后一位 +1（如 `1.0.0` → `1.0.1`）
  - 版本说明 = 总结自上次 release 以来的 commit

查看自上次 release 以来的变更：

```bash
git log $(git describe --tags --abbrev=0)..HEAD --oneline
```

### 第 1 步：更新版本号

需要同步修改三个文件中的版本号：

```bash
# 1. src-tauri/tauri.conf.json  →  "version": "X.Y.Z"
# 2. src-tauri/Cargo.toml       →  version = "X.Y.Z"
# 3. package.json               →  "version": "X.Y.Z"
```

用 sed 批量替换（以 `1.0.0` → `1.0.1` 为例）：

```bash
OLD=1.0.0
NEW=1.0.1

sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" src-tauri/tauri.conf.json
sed -i '' "s/^version = \"$OLD\"/version = \"$NEW\"/" src-tauri/Cargo.toml
sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" package.json
```

### 第 2 步：构建（含 Updater 签名）

构建前设置 Updater 签名密钥环境变量：

```bash
export TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/openstt.key)
npm run tauri build
```

构建产物位于：

```
src-tauri/target/release/bundle/macos/OpenSTT.app
src-tauri/target/release/bundle/dmg/OpenSTT_<version>_aarch64.dmg
src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz      ← Updater 用
src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz.sig  ← Updater 签名
```

### 第 3 步：签名（Code Signing）

#### 3.1 确认可用证书

```bash
security find-identity -v -p codesigning
```

选择 **Developer ID Application** 证书用于分发签名：

```
Developer ID Application: Shenzhen Luke Education Technology Co., Ltd. (RANFN29UQH)
```

#### 3.2 签名 .app bundle

Tauri 构建会生成 `.app`，但默认签名可能不含 hardened runtime 和自定义 entitlements，需要手动重签：

```bash
APP="src-tauri/target/release/bundle/macos/OpenSTT.app"
ENTITLEMENTS="src-tauri/entitlements.plist"
SIGN_IDENTITY="Developer ID Application: Shenzhen Luke Education Technology Co., Ltd. (RANFN29UQH)"

# 签名 app 内所有二进制（包括 test_elevenlabs 等）
for BIN in "$APP/Contents/MacOS/"*; do
    codesign --force --options runtime --timestamp \
        --sign "$SIGN_IDENTITY" \
        --entitlements "$ENTITLEMENTS" \
        "$BIN"
done

# 签名 dylibs（如有）
find "$APP/Contents/Frameworks" -name "*.dylib" -exec \
    codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" {} \; 2>/dev/null

# 签名整个 .app bundle
codesign --force --options runtime --timestamp \
    --sign "$SIGN_IDENTITY" \
    --entitlements "$ENTITLEMENTS" \
    "$APP"

# 验证签名
codesign --verify --deep --strict "$APP" && echo "Signature valid" || echo "Signature INVALID"
```

#### 3.3 重新打包 DMG

签名后需要重新创建 DMG（覆盖 Tauri 生成的版本）：

```bash
VERSION=1.0.1
DMG_DIR=$(mktemp -d)
cp -R "$APP" "$DMG_DIR/"
ln -s /Applications "$DMG_DIR/Applications"

DMG_PATH="releases/OpenSTT_${VERSION}_aarch64.dmg"
mkdir -p releases
rm -f "$DMG_PATH"
hdiutil create -volname "OpenSTT" \
    -srcfolder "$DMG_DIR" \
    -ov -format UDZO \
    "$DMG_PATH"
rm -rf "$DMG_DIR"
```

### 第 4 步：公证（Notarization）

#### 4.1 首次设置 Keychain 凭证（仅需一次）

```bash
xcrun notarytool store-credentials "notary" \
    --apple-id "lucas@easyans.com" \
    --team-id "RANFN29UQH" \
    --password "<app-specific-password>"
```

App-Specific Password 在 https://appleid.apple.com 生成。

#### 4.2 提交公证

```bash
xcrun notarytool submit "$DMG_PATH" \
    --keychain-profile "notary" \
    --wait
```

#### 4.3 钉票（Staple）

公证通过后，将票据钉入 DMG：

```bash
xcrun stapler staple "$DMG_PATH"
```

#### 4.4 验证

```bash
spctl --assess --type open --context context:primary-signature -v "$DMG_PATH"
```

### 第 5 步：生成 Updater Manifest

```bash
VERSION=1.0.1
SIG=$(cat src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz.sig)
NOTES="版本说明"

cat > releases/latest.json <<EOF
{
  "version": "$VERSION",
  "notes": "$NOTES",
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

### 第 6 步：提交版本号变更

```bash
git add src-tauri/tauri.conf.json src-tauri/Cargo.toml package.json
git commit -m "Bump version to $VERSION"
git tag "v$VERSION"
git push && git push --tags
```

### 第 7 步：创建 GitHub Release

上传 DMG（手动安装用）、`.app.tar.gz`（自动更新用）和 `latest.json`（更新清单）：

```bash
VERSION=1.0.1
DMG_PATH="releases/OpenSTT_${VERSION}_aarch64.dmg"
UPDATE_BUNDLE="src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz"

gh release create "v$VERSION" \
    "$DMG_PATH" \
    "$UPDATE_BUNDLE" \
    "releases/latest.json" \
    --title "OpenSTT v$VERSION" \
    --notes "$(cat <<'EOF'
## OpenSTT v1.0.1

<版本说明>

### Download
- **OpenSTT_1.0.1_aarch64.dmg** — macOS Apple Silicon
EOF
)"
```

### 第 8 步：清理旧 Release

只保留最新的 release，删除所有旧版本（含对应 tag）：

```bash
# 列出除最新外的所有 release tag，逐个删除
gh release list --json tagName -q '.[1:][].tagName' | while read -r tag; do
    gh release delete "$tag" --cleanup-tag --yes
done
```

---

## 快速参考

完整 release 一行流（替换版本号和说明）：

```bash
# 1. 改版本号
OLD=1.0.0 NEW=1.0.1
sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" src-tauri/tauri.conf.json
sed -i '' "s/^version = \"$OLD\"/version = \"$NEW\"/" src-tauri/Cargo.toml
sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" package.json

# 2. 构建（含 updater 签名）
export TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/openstt.key)
npm run tauri build

# 3. 签名
APP="src-tauri/target/release/bundle/macos/OpenSTT.app"
SIGN_IDENTITY="Developer ID Application: Shenzhen Luke Education Technology Co., Ltd. (RANFN29UQH)"
ENTITLEMENTS="src-tauri/entitlements.plist"
for BIN in "$APP/Contents/MacOS/"*; do
    codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" --entitlements "$ENTITLEMENTS" "$BIN"
done
find "$APP/Contents/Frameworks" -name "*.dylib" -exec codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" {} \; 2>/dev/null
codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" --entitlements "$ENTITLEMENTS" "$APP"
codesign --verify --deep --strict "$APP"

# 4. 打包 DMG
DMG_DIR=$(mktemp -d)
cp -R "$APP" "$DMG_DIR/" && ln -s /Applications "$DMG_DIR/Applications"
mkdir -p releases && DMG="releases/OpenSTT_${NEW}_aarch64.dmg"
hdiutil create -volname "OpenSTT" -srcfolder "$DMG_DIR" -ov -format UDZO "$DMG" && rm -rf "$DMG_DIR"

# 5. 公证
xcrun notarytool submit "$DMG" --keychain-profile "notary" --wait
xcrun stapler staple "$DMG"

# 6. 生成 updater manifest
SIG=$(cat src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz.sig)
cat > releases/latest.json <<MANIFEST
{
  "version": "$NEW",
  "notes": "版本说明",
  "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "platforms": {
    "darwin-aarch64": {
      "signature": "$SIG",
      "url": "https://github.com/lulucatdev/openstt/releases/download/v${NEW}/OpenSTT.app.tar.gz"
    }
  }
}
MANIFEST

# 7. 提交 + 发布
git add src-tauri/tauri.conf.json src-tauri/Cargo.toml package.json
git commit -m "Bump version to $NEW"
git tag "v$NEW" && git push && git push --tags
gh release create "v$NEW" "$DMG" "src-tauri/target/release/bundle/macos/OpenSTT.app.tar.gz" "releases/latest.json" \
    --title "OpenSTT v$NEW" --notes "版本说明"

# 8. 清理旧 release（只保留最新）
gh release list --json tagName -q '.[1:][].tagName' | while read -r tag; do
    gh release delete "$tag" --cleanup-tag --yes
done
```

---

## 首次设置

### Updater 签名密钥（仅需一次）

```bash
npx tauri signer generate -w ~/.tauri/openstt.key --ci
```

生成的公钥已写入 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。

**重要：`~/.tauri/openstt.key` 是私钥，切勿提交到仓库。**

### Notarization 凭证（仅需一次）

```bash
xcrun notarytool store-credentials "notary" \
    --apple-id "lucas@easyans.com" \
    --team-id "RANFN29UQH" \
    --password "<app-specific-password>"
```

---

## 签名凭证信息

| 项目 | 值 |
|------|------|
| 证书 | Developer ID Application: Shenzhen Luke Education Technology Co., Ltd. (RANFN29UQH) |
| Team ID | RANFN29UQH |
| Keychain Profile | `notary` |
| Entitlements | `src-tauri/entitlements.plist` |
| Bundle ID | `com.lulucat.openstt` |
| Updater 私钥 | `~/.tauri/openstt.key` |
| Updater 公钥 | 见 `src-tauri/tauri.conf.json` → `plugins.updater.pubkey` |

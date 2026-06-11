# Tauri 版リリース運用メモ(Remarks on Markdown)

ADR([0001](adr/0001-electron-to-tauri.md) / [0002](adr/0002-muyajs-to-muyajs-core.md))から
分離した運用情報。リリース・署名・鍵管理・ユーザーデータ移行の手順と現状。

## バージョン

`tauri.conf.json` の `version` は `"../package.json"` を指す(単一ソース)。
リリース時は `packages/desktop/package.json` の `version` を更新するだけでよい。

## ビルド

```bash
pnpm run build:tauri   # scripts/build-tauri.mjs 経由(署名 env の自動設定込み)
```

成果物: `packages/desktop/src-tauri/target/release/bundle/` 配下に
`dmg/Remarks_<ver>_aarch64.dmg`、`macos/Remarks.app`、
updater 用の `Remarks.app.tar.gz` + `.sig`(`bundle.createUpdaterArtifacts: true`)。

## 自動アップデート(tauri-plugin-updater)

- 構成: `tauri.conf.json` → `plugins.updater.pubkey` + `endpoints`
  (GitHub Releases の `latest.json`: `https://github.com/hisaboh/remarks/releases/latest/download/latest.json`)。
- **署名鍵**(2026-06-11 の Remarks 改名時に再生成、minisign key id `F249506DD796E2F1`):
  - 秘密鍵: `~/.tauri/remarks.key`(**パスワード付き**)。
  - パスワード: macOS キーチェーン(サービス名 `tauri-remarks-signing`)—
    `build-tauri.mjs` が `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 未設定時に自動取得。
  - CI 用正本: GitHub Actions secrets `TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`(設定済み)。
  - 人間が復元できる控え: macOS パスワードアプリ(iCloud キーチェーン同期)。
  - **公開鍵を差し替えると既存インストールはアップデートを検証できなくなる** — 鍵は失わないこと。
- ビルダーの注意: updater 署名は env `TAURI_SIGNING_PRIVATE_KEY`(鍵の**内容**。`*_PATH` は
  ビルド時には読まれない)のみを参照する。`build-tauri.mjs` が両方を設定する。
- IPC フロー(Electron 時代と同一契約):
  renderer `mt::check-for-update` → Rust `updater_check` →
  `mt::UPDATE_AVAILABLE` / `UPDATE_NOT_AVAILABLE` / `UPDATE_ERROR` →
  renderer `mt::NEED_UPDATE {needUpdate}` → ダウンロード + インストール →
  `mt::UPDATE_DOWNLOADED` → 約 1.5 秒後に再起動。
  ネイティブメニュー「Check for Updates」(`app.check-updates`)からも起動可。
  `boot_info.is_updatable` はリリースビルドで true。

## 手動リリース手順(A2 の CI が未整備の間)

1. `packages/desktop/package.json` の `version` を更新。
2. `pnpm run build:tauri`。
3. GitHub Release に `Remarks.app.tar.gz` / `.sig` / `.dmg` をアップロード。
4. `latest.json` を組み立ててアセットに追加(バンドラは生成しない):

```json
{
  "version": "<ver>",
  "pub_date": "<ISO8601>",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<.sig ファイルの内容>",
      "url": "<tar.gz のダウンロード URL>"
    }
  }
}
```

CI 化する場合は tauri-action が `latest.json` を自動生成する(残タスク A2)。

## コード署名・公証(macOS)

すべて環境変数駆動(Tauri bundler が自動検出。未設定なら adhoc 署名でビルド成功):

- `APPLE_SIGNING_IDENTITY` — **"Developer ID Application: …" 証明書が必要**
  (App Store 外配布・公証の必須要件。Apple Developer Program 加入が前提。
  現状この開発機には "Apple Development" 証明書しかないため未設定 = 残タスク A1)。
- 公証: `APPLE_ID` + `APPLE_PASSWORD`(App 用パスワード)+ `APPLE_TEAM_ID`、
  または `APPLE_API_ISSUER` + `APPLE_API_KEY`(App Store Connect API キー)。
  署名アイデンティティが設定されていれば bundler が自動で公証まで実行する。

## ユーザーデータ移行(初回起動時の自動処理)

`commands/migration.rs` が Tauri ストアが空のときに実行する。順序が重要:

1. **旧 Tauri identifier からの移行**(`import_old_tauri_data`):
   旧 `app.marktext.marktext` ディレクトリ → 新 `io.github.hisaboh.remarks` へ、
   新側に無いファイルのみコピー(2026-06-11 のアプリ名変更対応)。
   先に実行されることでストアが空でなくなり、次の Electron インポートをブロックする。
2. **旧 Electron userData からの移行**(macOS: `~/Library/Application Support/marktext`):
   - `preferences.json` / `dataCenter.json` — 取り込み後、各 init が現行デフォルトと
     リコンサイル。旧 userData 配下を指すパス値は取り込まず新デフォルトを再生成。
   - `keybindings.json` — ファイルコピー(存在し、新側に無い場合のみ)。
   - 移行しないもの: セッションバッファ(形式が異なる)、`spellcheck.json`
     (macOS はネイティブスペルチェッカー)、最近使った書類(OS 管理で実体なし)。

旧ディレクトリはバックアップとして残置される。

## 残タスク

リリースの前提・延期項目の一覧は [ADR 0001 の「残課題」](adr/0001-electron-to-tauri.md#残課題2026-06-12-時点) を参照。

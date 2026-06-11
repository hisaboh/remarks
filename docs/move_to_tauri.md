# Tauriへの移行計画

⏺ 現状分析

  Electron アーキテクチャの規模
  - 3プロセスモデル（main / preload / renderer）、IPC チャンネル定義は shared/types/ipc.ts に331行
  - main/ipc/ に14個のハンドラモジュール（fs, window, shell, clipboard, preferences, fonts, i18n, keyboard, spellcheck, uploader, ripgrep, watcher, menu, cmd）
  - マルチウィンドウ（エディタウィンドウ＋環境設定ウィンドウ、windows/editor.ts と windows/setting.ts）
  - ネイティブNode依存: keytar（資格情報）, ced（文字コード判定）, native-keymap（キーボードレイアウト）, @hfelix/electron-localshortcut

  過去の試作（tauri ブランチの "tmp" コミット）から得られる知見
  ゼロから作り直す方針ですが、以下は参考になる発見です:
  - cargo check / tauri build は通り、DMGは 11MB（現行Electron版は1.3GB — 100倍以上の差）
  - 1:1 IPCシムが有効な戦略であることが実証済み（renderer側コードをほぼ無改修で済ませられる）
  - WebKit特有の問題を発見済み: IME変換確定時の compositionend の扱い、window.getSelection() のアンカーノードがWebKitでは span.ag-paragraph 内ではなくコンテナ要素自体を指すケースがある → muya
  エンジン側に修正が必要
  - 未解決のTODO: マルチウィンドウの動的生成、ファイルウォッチャーの add/modify/remove 区別、iOS未着手

  ---
  移行計画（macOS デスクトップ集中・ゼロ設計）

  Phase 0 — 土台づくり

  - tauri2.0 ブランチに pnpm tauri init 相当の構成を新規設計（src-tauri/Cargo.toml, tauri.conf.json, capabilities/）
  - ビルドパイプライン: electron.vite.config.ts → vite.config.ts、package.json のスクリプト群をTauriコマンドに置換
  - 最小構成（空ウィンドウが開く状態）で cargo check / tauri dev が通ることを確認

  Phase 1 — IPCサーフェスの再設計

  Electronの mt::* チャンネルを1:1で踏襲するか、Tauriのinvoke/イベントモデルに合わせて整理するかをまず決める（後方互換シム vs クリーンな再設計のトレードオフ）。ゼロ設計の利点を活かすなら、既存の
  ipc.ts の型定義はそのまま活かしつつ、Rustコマンド名は素直なTauri流（snake_case）で再設計。
  - 優先実装: fs, window, shell, clipboard, preferences/dataCenter（利用頻度が高い）
  - 後続実装: fonts, i18n, keyboard, spellcheck, uploader, ripgrep, watcher, menu

  Phase 2 — レンダラー統合層

  - platform/index.ts（または同等のモジュール）で window.electron.* / window.fileUtils.* 等の既存呼び出しをTauri invoke/listenにマッピング
  - bootstrap.ts / main.ts の初期化フロー調整（mt::bootstrap-editor 相当のブートストラップ情報の受け渡し）
  - preload (contextBridge) 相当の機能をTauriの withGlobalTauri + capabilities権限モデルに置き換え

  Phase 3 — ネイティブ依存の置き換え

  ┌────────────────────────────────┬────────────────────────────────────────────────────┐
  │          Electron依存          │                 Tauri/Rust代替候補                 │
  ├────────────────────────────────┼────────────────────────────────────────────────────┤
  │ keytar                         │ keyring クレート or tauri-plugin-store（要件次第） │
  ├────────────────────────────────┼────────────────────────────────────────────────────┤
  │ ced                            │ chardetng / encoding_rs                            │
  ├────────────────────────────────┼────────────────────────────────────────────────────┤
  │ native-keymap                  │ Tauriのキーボードレイアウト取得 or 自前実装        │
  ├────────────────────────────────┼────────────────────────────────────────────────────┤
  │ @hfelix/electron-localshortcut │ tauri-plugin-global-shortcut                       │
  ├────────────────────────────────┼────────────────────────────────────────────────────┤
  │ electron-builder/Squirrel      │ Tauri bundler + tauri-plugin-updater               │
  └────────────────────────────────┴────────────────────────────────────────────────────┘

  Phase 4 — マルチウィンドウ対応

  - windowManager.ts / windows/editor.ts / windows/setting.ts の責務をRust側 WindowBuilder による動的生成に再設計
  - ウィンドウ間イベント（mt::ask-for-close, mt::window-active-status 等）をTauriのウィンドウイベント・カスタムイベントに移植
  - 過去の試作で「初期mainウィンドウのみ対応」だった制約を解消する設計にする

  Phase 5 — WebKit互換性対応（muyaエンジン）

  過去の試作で発見されたWebKit特有の問題を踏まえ、最初から検証観点に組み込む:
  - IME（日本語等）の compositionend / isComposing 周りの挙動差異
  - window.getSelection() のアンカーノード解決の差異（closest('.ag-paragraph') のフォールバック設計）
  - KaTeX/Mermaid/PlantUML等のレンダリングがWebKitで問題ないか個別検証

  Phase 6 — パッケージング・配布

  - アイコン、コード署名、公証（tauri.conf.json の bundle設定）
  - 自動アップデート機構の移行パス（既存ユーザーがElectron版からTauri版へどう移行するか要検討— 設定/資格情報/履歴データの引き継ぎ）

  Phase 7 — テスト・検証

  - Vitestユニットテストはそのまま流用可能（renderer/muya側はDOM API中心）
  - PlaywrightのE2Eスイートは要再検討（TauriはシステムWebView=WKWebViewを使うため、Chromium専用の駆動方法が使えない可能性。tauri-driver の利用を検討）
  - 手動QAチェックリスト: ファイル開く/保存/タブ管理/環境設定/スペルチェック/検索(ripgrep)/画像アップロード/diagram系プレビュー

  ---
  リスク・留意点

  - WebKitレンダリング差異: Mermaid/KaTeX/PlantUMLなど外部ライブラリのWebKit対応状況の検証が必要
  - 既存ユーザーの移行: 設定ファイル形式、資格情報ストレージ（keytar→keyring）の互換性
  - E2Eテスト基盤の刷新: Playwright→tauri-driver等への移行コスト
  - 配布・署名: electron-builderからTauri bundlerへの切り替えに伴うCI調整

  ---
  推奨される進め方

  1. Phase 0-1を最初の検証マイルストーンとし、「最小構成でビルド・起動・基本IPC疎通」を早期に確認
  2. Phase 5（WebKit互換性）は後回しにせず、早い段階で簡単なエディタ操作を動かして問題を洗い出す（過去の試作で問題が出た箇所のため）
  3. 既存Electronコードは並走させ（tauri2.0ブランチで段階的に置き換え）、機能パリティ達成後に削除
---

## Phase 6 実装メモ — 配布・署名・自動アップデート（2026-06-10 追記）

### バージョン

`tauri.conf.json` の `version` は `"../package.json"` を指す（単一ソース）。リリース時は
`packages/desktop/package.json` の `version` を更新するだけでよい。

### 自動アップデート（tauri-plugin-updater）

- 構成: `tauri.conf.json` → `plugins.updater.pubkey` + `endpoints`
  （GitHub Releases の `latest.json`: `https://github.com/marktext/marktext/releases/latest/download/latest.json`）、
  `bundle.createUpdaterArtifacts: true`（`.app.tar.gz` + `.sig` を生成 — macOS で検証済み）。
- 鍵ペア: `pnpm exec tauri signer generate -w ~/.tauri/marktext.key` で生成済み。
  **秘密鍵 `~/.tauri/marktext.key` は必ずバックアップすること**（紛失するとアップデート配信不能。
  公開鍵を差し替えると既存インストールはアップデートを検証できなくなる）。
  CI では secret `TAURI_SIGNING_PRIVATE_KEY` に秘密鍵の内容を設定する。
- ローカルビルド: `pnpm build:tauri` は `scripts/build-tauri.mjs` 経由で、env 未設定なら
  `~/.tauri/marktext.key` を自動使用する。
- IPC フロー（Electron の `main/menu/actions/marktext.ts` と同一契約）:
  renderer `mt::check-for-update` → Rust `updater_check` →
  `mt::UPDATE_AVAILABLE` / `UPDATE_NOT_AVAILABLE` / `UPDATE_ERROR` 通知 →
  renderer `mt::NEED_UPDATE {needUpdate}` → `updater_need_update` がダウンロード+インストール →
  `mt::UPDATE_DOWNLOADED` → 約1.5秒後に再起動。ネイティブメニュー
  「Check for Updates」(`app.check-updates`) からも起動可能。
- リリース手順: `.app.tar.gz` / `.sig` と `latest.json` を GitHub Release のアセットとして
  アップロードする。`latest.json` はバンドラは生成しない — リリース時に組み立てる
  （CI なら tauri-action が自動生成。手動なら `{version, pub_date, platforms:
  {"darwin-aarch64": {signature: <sigファイルの内容>, url: <tar.gzのURL>}}}` を書く）。`boot_info.is_updatable` はリリースビルドで true。

### コード署名・公証（macOS）

すべて環境変数駆動（Tauri bundler が自動検出。未設定なら署名なしでビルド成功）:

- `APPLE_SIGNING_IDENTITY` — **"Developer ID Application: …" 証明書が必要**
  （App Store 外配布・公証の必須要件。Apple Developer Program 加入が前提。
  現状この開発機には "Apple Development" 証明書しかないため、配布署名は未設定）。
- 公証: `APPLE_ID` + `APPLE_PASSWORD`（App用パスワード） + `APPLE_TEAM_ID`、
  または `APPLE_API_ISSUER` + `APPLE_API_KEY`（App Store Connect APIキー）。
  署名アイデンティティが設定されている場合に bundler が自動で公証を実行する。

### 既存ユーザーの移行（Electron → Tauri）

初回起動時（Tauri ストアが空のとき）に `commands/migration.rs` が旧 Electron userData
（macOS: `~/Library/Application Support/marktext`）から自動インポートする:

- `preferences.json` / `dataCenter.json` — ストアに取り込み後、各 init が現行デフォルトと
  リコンサイル（不要キー削除・新キー追加 — Electron 時代のアップグレードと同じ流儀）。
  旧 userData 配下を指すパス値（旧 `images` フォルダ等）は取り込まず新デフォルトを再生成。
- `keybindings.json` — ファイルコピー（存在し、新側に無い場合のみ）。
- 移行しないもの: セッションバッファ（`editor-buffer*.json`、ウィンドウ形式が異なる）、
  `spellcheck.json`（macOS はネイティブスペルチェッカー使用）、最近使った書類
  （Electron は macOS では OS 管理で、ファイルが存在しない）。

---

## 残タスク一覧（2026-06-10 時点 — Phase 0〜7 本体実装は完了）

移行計画の Phase 0〜7 のコア実装はすべて完了。残るのは「外部依存待ち」「移植時に
意図的に後回しにした項目（DEFERRED）」「任意の拡張」の3カテゴリ。リリースに最低限
必要なのは A1（証明書）のみ。

### A. 外部依存・リリース運用（コードでは解決不可）

| # | タスク | ブロッカー |
|---|---|---|
| A1 | コード署名 + 公証 | **Apple Developer Program 加入**（Developer ID Application 証明書）。配線・文書化は完了済み、証明書を入れて環境変数を設定するだけ |
| A2 | GitHub Releases へのリリースフロー | `latest.json` + `.tar.gz` + `.sig` 公開の CI ワークフロー（tauri-action）。**未着手**（`.github/workflows/` に `tauri build` を回すものが無い）。現状は手動手順を上記に文書化済み |
| ~~A3~~ | ~~秘密鍵バックアップ~~ ✅ 完了 | CI 用正本 = GitHub Actions Repository secret `TAURI_SIGNING_PRIVATE_KEY`（write-only なので読み出し不可）。人間が復元できる控え = macOS パスワードアプリ（iCloud キーチェーン同期）。※この鍵はアプリ名変更時に作り直す暫定鍵（§E） |

### B. 移植時に意図的にスキップした項目（DEFERRED）

機能的には動くが、Electron 版と完全パリティではない箇所。

**ウィンドウ / セッション系**
- B1: マルチウィンドウのセッション復元（現状は main ウィンドウ単独復元のみ）
- B2: maximize/fullscreen のイベント同期 + フォーカス時のメニュー再同期（Phase 4b）
- B3: 復元タブの外部変更検知・ファイル監視（restore 時は watch 対象外）

**ネイティブ依存の積み残し（Phase 3）**
- ~~B4: UTF-16 保存~~ ✅ 完了（2026-06-10, commit a19b1c0b）— `str::encode_utf16()`
  で手動エンコード（encoding_rs にエンコーダーがないため）。ラウンドトリップのユニットテスト付き。
- B5: クリップボードからのファイルパス推測（`clipboard_guess_file_path` スタブ — NSPasteboard 要）
- B6: グローバルショートカット本体（ローカルショートカットは Phase 4-8 で代替済み）
- B7: スペルチェックの右クリック候補・カスタム辞書（NSSpellChecker ブリッジ要）、
  Windows/Linux スペルチェック

**その他**
- B8: ディレクトリ / タブの unwatch（現状 watch-on-open のみ、セッション内で単調増加）
- B9: per-assoc ファイルアイコン（md.icns）
- B10: ripgrep の非 UTF8 行 base64 デコード + 前後コンテキスト行

### C. テスト・任意の拡張

- C1: tauri-driver による実機 E2E（Linux/Windows CI — macOS は原理的に不可）
- C2: 既存 Electron E2E 24 スペックの追加移植

### D. 既知の残バグ（スコープ外と判断済み）

- D1: 単一行 `$$...$$` ブロック数式が raw 表示 — **Electron 版でも同じ** muya パーサー挙動
  （移行スコープ外）

### E. アプリ名変更チェックリスト（2026-06-11 実施）

**実施済み**: アプリ名 = **Remarks on Markdown**(表示名 = **Remarks**)、配布元 =
https://github.com/hisaboh/remarks 。
- `tauri.conf.json`: `identifier` → `io.github.hisaboh.remarks`、`productName` → `Remarks`、
  ウィンドウタイトル、updater `endpoints` → hisaboh/remarks の latest.json。
- 旧 Tauri identifier(`app.marktext.marktext`)→ 新 identifier のデータ移行を
  `migration.rs::import_old_tauri_data` として新設(初回起動時に新ディレクトリに無い
  ファイルのみコピー。Electron インポートより先に実行され、移行済みストアが空でなくなる
  ことで Electron 側の上書きを防ぐ)。旧ディレクトリはバックアップとして残置。
- 表示名 "MarkText" → "Remarks": `static/locales/*.json`(値のみ・キー名は不変)、
  タイトルバー、`index.html` の `<title>`。About 画面は正式名 "Remarks on Markdown"。
- `scripts/build-tauri.mjs`: 鍵パス → `~/.tauri/remarks.key`、
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 未設定時は警告(パスワード付き鍵前提に変更)。

**残作業(鍵まわり)**:
- [x] `~/.tauri/remarks.key` をパスワード付きで生成(2026-06-11、ユーザーが対話実行)
- [x] 新公開鍵(key id F249506DD796E2F1)を `plugins.updater.pubkey` に反映
- [ ] GitHub Actions secrets: `TAURI_SIGNING_PRIVATE_KEY` 差し替え +
      `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 追加(A2 のリリース CI 整備時)
- [ ] パスワードアプリの控え(§A3)を新鍵+パスワードで更新し、旧 `~/.tauri/marktext.key` を処分

当初の計画メモ(波及範囲)は以下の通り:

- **`tauri.conf.json`**
  - `identifier`（現 `app.marktext.marktext`）— ⚠️ これを変えると `app_data_dir` の
    パス（現 `~/Library/Application Support/app.marktext.marktext`）が変わり、**既存 Tauri
    ユーザーの設定/データが新パスに移らない**。旧 identifier → 新 identifier の
    データ移行コードが別途必要（`commands/migration.rs` は Electron→Tauri 専用なので流用不可）。
  - `productName`（現 `marktext`）— `.app` 名・バンドル名・DMG 名。
  - `plugins.updater.endpoints`（現 `github.com/marktext/marktext`）— リポジトリ名を
    変える場合のみ更新。
  - `plugins.updater.pubkey` — 鍵再生成後の新公開鍵に差し替え。
- **updater 署名鍵**
  - `tauri signer generate -w ~/.tauri/<新名>.key`（**パスワード付き**で生成）。
  - `scripts/build-tauri.mjs` のハードコードされた鍵パス `~/.tauri/marktext.key` を新名に変更し、
    `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 環境変数の受け渡しを追加（現状は空パスワード前提）。
  - GitHub Actions secrets（`TAURI_SIGNING_PRIVATE_KEY` + 新たに `..._PASSWORD`）を更新。
- **`commands/migration.rs`** — 旧 Electron データ参照 `config_dir/marktext` は据え置き
  （Electron 版の名前は変わらないため）。ただし上記の「旧 Tauri identifier → 新 identifier」
  移行を足す場合はこのファイルに追加するのが自然。
- **表示名 "MarkText"** — `static/locales/*.json`、ウィンドウタイトル、メニュー i18n、
  CLAUDE.md 等のドキュメント。機能には影響しないが UI 文言として要確認。
- **`fileAssociations`** の `name: "Markdown"` はアプリ名と無関係 → そのままでよい。

### 着手の優先順位（提案）

1. **今すぐ着手可能で価値が高い**: B8（unwatch、メモリリーク防止）、B1（マルチウィンドウ復元）
2. **証明書が用意でき次第**: A1 → A2（リリース体制）
3. **任意**: C1/C2（テスト）、B9（アイコン）

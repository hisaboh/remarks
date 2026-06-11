# ADR 0001: デスクトップシェルを Electron から Tauri 2.0 へ移行する

- **ステータス**: 採用・実施済み(Phase 0–7 完了)
- **日付**: 決定 2026-06-07 / 本体実装完了 2026-06-10 / アプリ名変更(Remarks)2026-06-11
- **ブランチ**: `tauri2.0`

## コンテキスト

- Electron 42 構成は 3 プロセスモデル(main / preload / renderer)で、`main/ipc/` に
  14 のハンドラモジュール、IPC チャネル定義 331 行(`shared/types/ipc.ts`)、
  マルチウィンドウ(エディタ + 環境設定)を持つ。
- 配布物が **約 1.3 GB**(Electron ランタイム同梱)と巨大。
- ネイティブ Node 依存(keytar / ced / native-keymap / @hfelix/electron-localshortcut)が
  Electron バージョンアップのたびに rebuild を要し、保守負荷が高い。
- 先行スパイク(`tauri` ブランチ `e4d9cdde`)で実証済みの知見:
  - `tauri build` が通り **DMG は 11 MB**(約 1/100)。
  - renderer の `window.electron.*` 呼び出しを Tauri `invoke`/`listen` へ写像する
    「1:1 IPC シム」戦略が有効(renderer Vue コードをほぼ無改修にできる)。
  - WebKit(WKWebView)特有の問題が存在する: IME の compositionend 挙動、
    `window.getSelection()` のアンカーノード解決差異。

## 決定

**Tauri 2.0 へ完全移行する。** あわせて以下の下位決定を行った:

1. **スパイクのコードは再利用しない** — `tauri2.0` ブランチでゼロから設計し、
   スパイクは「発見された問題のカタログ」としてのみ参照する。
2. **macOS デスクトップに集中**する。iOS は明示的にスコープ外。
3. **IPC は既存の `mt::*` チャネル契約を維持**し、renderer 側の
   `platform/` シム(`INVOKE_MAP`/`SEND_MAP`)で Tauri へ写像する。
   Rust コマンド名は Tauri 流の snake_case で再設計し、`ipc.ts` の TS 型契約を
   引き続き真実源とする。プリロードの `window.electron.*` サーフェスはシムが再現し、
   同一ソースが Electron / Tauri 両対応で動く(`isTauri()` 分岐)。
4. **キーボードショートカットは renderer 側 keydown ディスパッチャ**で実装する
   (main 側ショートカット登録と native-keymap を排除。KeyboardEvent が
   レイアウト解決済みのキーを与えるため)。
   - 注意: ネイティブ編集キー(Cmd+C/V/X/A)はディスパッチャから除外する。
     対応するレンダラーコマンドは存在せず、横取りすると WebKit のネイティブ動作を
     殺す(実バグとして発生し修正済み)。
5. **ネイティブ依存の置換**: ced → chardetng + encoding_rs(UTF-16 保存は手動エンコード)、
   keytar → 省略(encryptKeys が空で実質未使用)、
   electron-builder → Tauri bundler + tauri-plugin-updater、
   ripgrep → externalBin sidecar として同梱。
6. **Electron 構成は併走**させ、機能パリティ達成後に削除する(現在も併走中)。

## 検討した代替案

- **Electron 継続** — バンドルサイズ・保守負荷の問題が解決しない。
- **スパイクコードの再利用** — 動作はするが設計品質が低く、負債を引き継ぐため不採用。
- **IPC を Tauri 流に全面改名** — renderer 全域の改修が必要になり、
  シム方式に比べて移行リスクが大きい。

## 結果

### 成果

- 配布物 1.3 GB → **約 13–15 MB**(DMG、ripgrep sidecar 込み)。
- 全フェーズ完了: IPC バックボーン、マルチウィンドウ、ネイティブメニュー
  (i18n・チェック状態同期込み)、ファイル監視(notify-debouncer-full)、
  ripgrep 検索、セッション復元(main ウィンドウ)、auto-updater、
  Electron ユーザーデータの初回起動時自動移行、E2E。
- E2E は **Playwright WebKit + Tauri モック**(`test/e2e-tauri/`)で実機レス検証
  (tauri-driver は macOS 非対応のため。実機 E2E は Linux/Windows CI の将来課題)。

### トレードオフ・学び

- **WKWebView の癖との戦いが最大コスト**だった: IME composition(blur 禁止・
  DOM-authoritative コミット)、選択のブロック境界、クリップボードのフレーバー挙動。
  エンジン側の対応は ADR 0002 の知見に引き継がれた。
- WKWebView のコンソールは stdout に出ない —
  デバッグは `window.fileUtils.writeFile` によるファイルログが定石。
- muyajs(当時のエンジン)の HMR はハンドラが stale になるため、
  エンジン変更時は `dev:tauri` の再起動が必要。

### 残課題(2026-06-12 時点)

| 分類 | 内容 |
|---|---|
| A1 | コード署名 + 公証 — Apple Developer Program 加入待ち(配線・文書化済み) |
| A2 | GitHub Releases リリース CI(tauri-action)— secrets 準備済み・未着手 |
| B | DEFERRED: マルチウィンドウのセッション復元 / maximize・fullscreen イベント同期 / クリップボードのファイルパス推測(NSPasteboard)/ グローバルショートカット / スペルチェック右クリック候補 / unwatch / md.icns / ripgrep 非 UTF-8 行 |
| C | tauri-driver 実機 E2E(Linux/Windows CI)、Electron E2E スペックの追加移植 |

## 参照

- 運用手順(updater 鍵・署名・公証 env・手動リリース・データ移行挙動):
  [`docs/release_tauri.md`](../release_tauri.md)
- エンジン切替: [ADR 0002](0002-muyajs-to-muyajs-core.md)
- 旧スパイク: `tauri` ブランチ `e4d9cdde`(参照のみ)

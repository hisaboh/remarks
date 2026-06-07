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
# tauri2.0 ブランチの @muyajs/core 移行計画

> 作成: 2026-06-11 · 対象ブランチ: `tauri2.0`(Remarks on Markdown / Tauri 2.0 版)
> 目的: デスクトップのエディタエンジンを `packages/muyajs`(レガシー JS)から
> `packages/muya`(`@muyajs/core`、TS リライト)へ切り替え、develop と同じエンジン構成にする。

## 背景

- develop は 2026-06 に muyajs → @muyajs/core 移行を完了済み(フェーズ A〜G、パリティ 15/15、
  実機監査で 8 バグ修正)。記録は develop の `.claude/muya-migration-TODO.md`(#4438 でアーカイブ)。
- tauri2.0 は分岐点 `d14c3d6c`(2026-06-05)以降も muyajs を使い続けており、
  IME(`79e2cdce`)・行コピペ(`f86286a3`)等の WKWebView 対応修正はすべて muyajs 側に入っている。
- 両ブランチの乖離: develop 側にのみ 53 コミット(うち約 50 がエンジン移行関連)、
  tauri2.0 側にのみ約 70 コミット(Tauri 移行 + リネーム一式)。
- @muyajs/core の利点: ブロックツリー + snabbdom による整合的レンダリング
  (muyajs の renderRange 文字列継ぎ接ぎ起因のバグを構造的に排除)、OT ベースの履歴、
  TS 型付き公開 API、CommonMark/GFM 準拠スイート(ラチェット式)、WebKit を含む実ブラウザ E2E。

## 戦略: develop を丸ごとマージ

cherry-pick(約50コミット)やリベース(約70コミット再生)ではなく、**`git merge develop` 一発**で取り込む。

2026-06-11 時点のドライラン結果(`git merge --no-commit develop`):

- コンフリクトは **34 ファイルのみ**。`editor.vue`・Pinia store・`platform/` shim・locale は**無衝突**。
  - テーマ CSS 32 件 — tauri2.0 の `--editorAreaWidth: calc(100px + 80%)` 変更 vs
    develop の kebab-case 変数ブロック追加(#4404)。**両方採用**の機械的解決
    (kebab 側は `--editor-area-width: var(--editorAreaWidth)` 参照なので tauri の値が自動で生きる)。
    スクリプト化可能。
  - `packages/desktop/test/e2e/issue-4374.spec.ts` — cherry-pick(`90f79c7e`)との重複。develop 側を採用。
  - `pnpm-lock.yaml` — develop 側を採用後 `pnpm install` で再生成。
- マージにより、将来の tauri2.0 → develop 還元マージの差分が「Tauri 化そのもの」だけに縮む。

## フェーズ計画

### Phase 0 — 準備(小)

- [ ] 統合ブランチ `tauri2.0-muya-core` を作成(`tauri2.0` は安定版として温存)
- [ ] 再検証チェックリストの確定(下記「再検証チェックリスト」)

### Phase 1 — develop マージ

- [ ] `git merge develop` → 上記 34 コンフリクトを解決して 1 マージコミット
- [ ] `pnpm install`(lockfile 再生成)→ `pnpm run typecheck` / `test:unit` / lint がベースライン通り
- [ ] この時点で Electron 経路(`pnpm run dev`)は develop と同等に動くはず — 軽くスモーク

### Phase 2 — Tauri ビルド配線

- [ ] **重要**: develop の vite 変更は `electron.vite.config.ts` に入ったが、Tauri レンダラーは
      **別ファイルの `packages/desktop/vite.config.ts`** を使う。@muyajs/core 関連の
      resolve / CSS(`?inline` インポート、#4412)/ optimizeDeps 設定をミラーリングする
- [ ] `pnpm run dev:tauri` が起動し、新エンジン(`mu-*` クラスの DOM)で空ドキュメントが描画される
- [ ] **IME 日本語入力の前倒しスモークをここで実施**(Phase 4 を待たない —
      muyajs 移行時の教訓「最大の未知数は早期に踏む」)

### Phase 3 — Tauri 固有の適合

調査済みのギャップ(develop の新 editor.vue が要求するもの vs Tauri shim):

- [ ] `getPathForFile`: shim はスタブ(`() => ''`)のまま。Tauri は webview の HTML D&D が
      使えない(native の onDragDropEvent が横取り)ため、新エンジンの dragDropImage(PG4)の
      代替として **`platform/fileDrop.ts` に画像ファイルのルートを追加**し、
      muya の insertImage / imageAction フローへ接続する
- [ ] `clipboardFilePath`: 既存スタブ(null)で可。ビットマップ貼り付け(PG5)は
      FileReader 経由なのでパス不要で動くはず — 実機確認のみ
- [ ] `imageAction` → 既存の uploader IPC(`mt::uploader::upload`)への接続確認
- [ ] `window.DIRNAME`(相対パス画像の解決、G1 #4428)が Tauri 下でも設定されることを確認
- [ ] メニュー状態同期: Rust 側 4a(`menu_update_format` 等)と #4415 の新パリティ API
      (selection-change の active formats 由来)の整合確認
- [ ] キーバインドディスパッチャ(`renderer/src/keybinding/`)と新エンジン内蔵ショートカットの
      二重処理チェック(クイック挿入トリガーは `@` → `/` に変わる点に注意 #4405)
- [ ] spellcheck: WKWebView は HTML 属性で下線が出る方式 — @muyajs/core 側のオプション名を確認して接続

### Phase 4 — WKWebView QA(最重要)

develop の G 監査(Electron 実機)に相当する Tauri 実機監査。

- [ ] **IME 日本語入力**(対話テスト、複数ラウンド想定): 変換確定 / 確定後 Enter /
      クリック離脱 / 文中挿入 / ESC キャンセル / 空段落への入力
- [ ] **行コピペ仕様の再移植**: muyajs に入れた「改行込み行コピー → 行として挿入・空行保持」
      (`f86286a3`)を packages/muya に TS + ユニットテスト付きで再実装
      (upstream には無い Remarks 独自仕様)
- [ ] 選択・カーソル位置のエッジケース(行末余白クリック、空行、引用/コードフェンス境界)
- [ ] 図のレンダリング: mermaid / plantuml / flowchart / sequence / vega-lite、KaTeX 数式
- [ ] 相対パス・file:// 画像の表示、画像 D&D(Phase 3 の native ルート)
- [ ] 大きめドキュメントでの入力性能スモーク

### Phase 5 — テスト移行

- [ ] `test/e2e-tauri` の書き換え: `span.ag-paragraph` 等 → `mu-*` DOM
      (`launch` / `editor-input` / `paste-whole-line`)
- [ ] `ime-composition.spec.ts` は muyajs 固有の WKWebView シーケンス再生なので、
      新エンジン版の IME スペックに置換
- [ ] packages/muya 自体のスイート(unit / conformance / e2e)+ desktop unit が全グリーン

### Phase 6 — 仕上げ

- [ ] `pnpm run build:tauri` でリリースビルド + パッケージ版スモーク
- [ ] docs 更新(`move_to_tauri.md` にエンジン切替の節を追記、CLAUDE.md の muyajs 記述)
- [ ] **`packages/muyajs` は削除しない**(develop と同じく Phase H は 0.20.0 正式リリース後)

## リスクと対応

| リスク | 対応 |
|---|---|
| WKWebView での IME 挙動(最大の未知数) | Phase 2 直後に前倒しスモーク。問題時は muyajs での知見(composition 中の blur/再レンダー禁止、commit の DOM-authoritative 化)を @muyajs/core に移植 |
| Tauri 固有バグの潜在(develop の G 監査は Electron のみ) | Phase 4 を G 相当の実機監査として実施 |
| muyajs 修正(IME・行コピペ・Enter 分割)の喪失 | 再検証チェックリストで明示管理。行コピペは仕様として再移植 |
| HTML D&D が Tauri で無効 | native fileDrop.ts への画像ルート追加(Phase 3) |
| vite 設定の二重管理(electron.vite.config.ts / vite.config.ts) | Phase 2 でミラーリング。恒久対策は将来の共通化 |

## 再検証チェックリスト(muyajs 時代に修正済み・新エンジンで要再確認)

- [ ] IME: 確定 / 確定後 Enter / クリック離脱 / 文中挿入(`79e2cdce` 相当)
- [ ] 行コピペ: 改行込みコピーの行挿入・空行保持・カーソル位置(`f86286a3` 相当)
- [ ] ペースト直後の描画(renderRange 相当の問題が新エンジンに無いこと)
- [ ] Cmd+C/V/X/A がエディタ・設定画面の両方で動く(`ffe92091` はレンダラー側なので影響なしの見込み)
- [ ] リスト項目内 Enter(`90f79c7e` 相当 — @muyajs/core では既修の可能性が高い)
- [ ] スペルチェック下線(macOS)
- [ ] ウィンドウドラッグ・タイトルバー(エンジン非依存だが DOM 変化の影響確認)

## タイミング

リリース体制構築(残タスク A2: GitHub Releases CI)の**前**に実施するのを推奨。
初リリース前にエンジンを切り替えれば、IME 再 QA が一度で済み、
リリース後の回帰対応・二重移植コストを回避できる。

## 進捗記録

| 日付 | フェーズ | 内容 |
|---|---|---|
| 2026-06-11 | 計画 | 本ドキュメント作成。マージのドライラン実施(コンフリクト 34 件を確認) |
| 2026-06-11 | Phase 0–1 ✅ | `tauri2.0-muya-core` で develop をマージ(`bea9e9ff`)。コンフリクト 34 件を計画通り解決。desktop unit 465 / muya unit 678 / typecheck クリーン |
| 2026-06-11 | Phase 2 ✅ | **vite 設定変更は不要だった**(develop も electron.vite.config.ts 未変更 — パッケージ解決のみで統合)。dev:tauri 起動・mu-* DOM 描画・セッション復元・ja メニューを確認 |
| 2026-06-11 | Phase 2.5 | IME スモークで確定 Enter の段落分割を発見 → `2cb7ad41` keydown 229 ガード(muyajs と同一の WKWebView 癖)。ユーザー検証済み(確定/確定後Enter/離脱/文中挿入/ESC) |
| 2026-06-11 | Phase 3 ✅ | `c8add1db` native fileDrop に画像ルート追加(bus 'insert-image' 経由)。spellcheck 属性・DIRNAME・フォーマットメニュー・キーバインドは適合済みを確認 |
| 2026-06-11 | Phase 4 ✅ | 行コピペ仕様を移植: `add66b0f` ペースト側(コピー側は新エンジンが元々改行保持)→ `ea876148` text/html フレーバーに負けないよう text/plain からシグナル取得 → `df174e68` 行途中ペーストはカーソル位置で分割。`7599b0c9` Shift+↑/↓ のブロック境界クロス(WKWebView は素で越えられない — Playwright WebKit は越える環境差)。全てユーザー検証済み |
| 2026-06-11 | Phase 5 ✅ | `d4795432` e2e-tauri を mu-* DOM へ書き換え、IME スペックは新エンジン版(keyCode 229 回帰含む)に置換。最終 19/19 グリーン、muya unit 686 |

## 移行で得た知見(@muyajs/core × WKWebView)

- **IME 確定 Enter は keyCode 229 で compositionend の後に届く** — `isComposed` フラグでは
  防げない。muyajs と同じ癖が新エンジンでも踏み抜かれた(`content.ts` keydownHandler 冒頭でガード)。
- **WKWebView の Shift+↑/↓ はブロックラッパーを縦に越えられない**(Playwright の新しめ
  WebKit は越える)。Editor ディスパッチ層で境界クロスを自前実装し、エンジン差を吸収。
- **muya の normal コピーは text/html も載せる**ため、ペースト系の挙動判定を markdown
  (HTML 由来)に掛けると text/plain だけが保持する情報(末尾改行)を取り落とす。
- e2e のクリップボード系ラウンドトリップは**両フレーバーを運ぶ**こと(text/plain のみだと
  実コピーと挙動が分岐する盲点になる)。

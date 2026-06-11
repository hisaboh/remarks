# ADR 0002: エディタエンジンを muyajs から @muyajs/core へ切り替える

- **ステータス**: 採用・実施済み(`tauri2.0` へマージ済み)
- **日付**: 決定 2026-06-11 / 実施 2026-06-11〜12
- **前提**: [ADR 0001](0001-electron-to-tauri.md)(Tauri 移行)

## コンテキスト

- develop ブランチは 2026-06 にデスクトップのエディタエンジンを
  `packages/muyajs`(レガシー JS)から `packages/muya`(`@muyajs/core`、TS リライト)へ
  移行完了した(フェーズ A–G、パリティ 15/15、実機監査で 8 バグ修正。
  muyajs の削除 = Phase H は 0.20.0 リリース後に延期)。
- 一方 `tauri2.0` は分岐点(`d14c3d6c`、2026-06-05)以降も muyajs を使い続け、
  WKWebView 対応の修正(IME composition・行コピペ・renderRange)を muyajs 側に蓄積していた。
- muyajs の問題は構造的だった: contentState モデルと「`renderRange` による DOM 区間の
  文字列置換」の二重管理が、「モデルは更新されるが DOM に出ない」系のバグ
  (クリックだけでは renderRange が更新されない等)を繰り返し生んだ。
- @muyajs/core の利点: ブロックツリー + snabbdom による整合的レンダリング、
  ot-json1/ot-text-unicode による OT ベースの履歴、TS 型付き公開 API、
  CommonMark/GFM 準拠スイート(ラチェット式)、WebKit を含む実ブラウザ E2E。
- 両ブランチの乖離は develop 側 53 コミット / tauri2.0 側 約 70 コミットまで拡大しており、
  放置すれば将来の還元マージと muyajs 修正の再移植コストが増え続ける。

## 決定

**`tauri2.0` も @muyajs/core に切り替える。** 方式と下位決定:

1. **develop を丸ごと 1 回のマージで取り込む**(cherry-pick やリベースではなく)。
   ドライランでコンフリクトは 34 件のみ(テーマ CSS 32 + spec 重複 1 + lockfile)と
   機械的に解決可能なことを事前確認した。
2. **リリース体制構築(A2)より先に実施**する。初リリース前に切り替えれば
   IME 再 QA が一度で済み、リリース後の回帰対応・二重移植を回避できる。
3. `packages/muyajs` は削除しない(develop と同じく Phase H は 0.20.0 後)。
4. Remarks 独自仕様(行コピペ)は muyajs 版の挙動を **@muyajs/core に
   TS + ユニットテスト付きで再実装**する。

## 検討した代替案

- **muyajs 継続** — 乖離が拡大し続け、将来の develop 還元マージが
  「Tauri 化 × エンジン移行」の交差した巨大作業になる。構造的バグも残る。
- **cherry-pick(約 50 コミット)/ リベース(約 70 コミット再生)** —
  マージ 1 回より作業量・リスクとも大きい。

## 結果

### 実施記録(2026-06-11〜12、ユーザー実機検証済み)

| コミット | 内容 |
|---|---|
| `bea9e9ff` | develop マージ(コンフリクト 34 件を計画通り解決) |
| `2cb7ad41` | IME 確定 Enter(keyCode 229)ガード |
| `c8add1db` | 画像ファイルの native D&D ルート(Tauri は HTML DnD 不可のため) |
| `add66b0f` | 行コピペ(行頭 = 前に挿入)の移植 + ユニットテスト |
| `7599b0c9` | Shift+↑/↓ のブロック境界クロス(Editor ディスパッチ層) |
| `ea876148` | 行ペースト判定を text/plain 起点に修正 |
| `df174e68` | 行途中への行ペースト = カーソル位置で分割 |
| `d4795432` | e2e-tauri スイートを mu-* DOM へ書き換え |

- **Tauri 用 vite 設定の変更は不要**だった(@muyajs/core はパッケージ解決のみで統合)。
- テスト: muya unit 686 / desktop unit 465 / e2e-tauri 19 本 全グリーン。
- コピー側は新エンジンが元々末尾改行を保持(StateToMarkdown)しており対応不要 —
  muyajs より設計が良い実例。

### WKWebView 知見(将来のエンジン作業で必読)

1. **IME 確定 Enter は keyCode 229 で compositionend の後に届く** —
   `isComposed` フラグでは防げない。`content.ts` keydownHandler 冒頭で
   `isComposing || keyCode === 229` をガードする(Chromium は isComposing が立つので無害)。
2. **WKWebView の Shift+↑/↓ はブロックラッパーを縦に越えられない**
   (Playwright の新しめ WebKit は越える — 環境差)。Editor ディスパッチ層で
   境界クロスを自前実装して挙動を決定的にした。ブロック単位のイベントルーティングは
   クロスブロック選択中スキップされるため、ディスパッチ層に置くこと。
3. **muya の normal コピーは text/html もクリップボードに載せる**。
   ペースト系の挙動判定を HTML 由来の markdown に掛けると、text/plain だけが保持する
   情報(末尾改行)を取り落とす。シグナルは text/plain から読むこと。
4. **クリップボード系の E2E ラウンドトリップは両フレーバー(text/plain + text/html)を
   運ぶこと**。text/plain のみだと実コピーと挙動が分岐する盲点になる。

### トレードオフ

- 行コピペの挙動(行頭 = 前に挿入 / 行途中 = カーソルで分割 / 空行保持)は
  upstream muya に無い Remarks 独自仕様となり、upstream 追従時に維持コストがかかる
  (ユニットテスト + e2e で仕様を固定済み)。
- Shift+Arrow の列位置は折り返し行ではブロック端への近似(非折り返し行では正確)。

## 参照

- ADR 0001(Tauri 移行と WKWebView デバッグ手法)
- develop 側の移行記録: develop ブランチ `.claude/muya-migration-TODO.md`(#4438)
- `packages/muya/CLAUDE.md`(エンジンのアーキテクチャとコマンド)

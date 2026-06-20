---
name: upstream-sync
description: 本家 marktext/marktext をこのフォークへ追随する。upstream-sync ミラーを取り込み main にマージし、フォークの「採用/保持」方針で衝突を解決、全検証＋ビルドまで行う。「upstream を取り込む」「upstream と同期」「marktext に追随」「upstream-sync を main にマージ」等で使用。
---

# upstream 追随（marktext/marktext → このフォーク）

ミラーブランチを作業ブランチへ **定期的に単一マージ** することで upstream を取り込む。
cherry-pick やコミット単位ではなく **マージ** を使う（マージベースが繋がり、次回以降の
同期が軽くなる）。62コミット分の追随（マージ `c6503d1e`）の知見をまとめたもの。

## ブランチ構成（メモリ `branch-workflow` 参照）

- **`main`** … 作業ブランチ・GitHub デフォルト。マージ先。
- **`upstream-sync`** … upstream `develop` の読み取り専用ミラー。編集禁止・PR禁止。
- リモート: `origin` = hisaboh/remarks、`upstream` = marktext/marktext。
- `gh` のデフォルトリポジトリは **upstream に解決される** → 常に `-R hisaboh/remarks` を付ける。
- フォークと upstream は同じモノレポ構成（`packages/muya`・`packages/desktop`）なので、
  衝突は局所的で構造崩れは起きにくい。

## 0. まずミラーを最新化

```bash
git fetch upstream && git fetch origin --prune
# ミラーは upstream/develop と一致しているか？
git rev-parse --short upstream-sync origin/upstream-sync upstream/develop
git rev-list --count upstream-sync..upstream/develop   # >0 なら古い
```

古ければ `upstream-sync` を `upstream/develop` へ fast-forward して push（純粋なミラーなので FF）。その後 re-fetch。

範囲確認: `git rev-list --count main..upstream-sync` = 取り込むコミット数。
`git log --oneline main..upstream-sync` で内容を読む。

## 1. マージ開始（サンドボックス無効で）

マージは **必ず `dangerouslyDisableSandbox: true`** で実行。サンドボックスは
`.vscode/settings.json` 等の unlink をマージ途中で止め、`Merge with strategy ort
failed` になる。サンドボックスで失敗した試行が残骸（git が上書き拒否する untracked）を
残したら掃除する: `git clean -fdn`（中身確認＝upstream 追加の source/spec/locale だけのはず）
→ `git clean -fd` → 再マージ。

```bash
git merge --no-ff --no-commit upstream-sync
git diff --name-only --diff-filter=U   # 衝突一覧
```

衝突を分類: **コード(.ts/.vue)** / **locale(*.json)** / **lock・package.json**。

## 2. 解決方針（フォークの確定済みポリシー）

各衝突は「upstream 採用」か「我々を保持」かを判断:

- **upstream を丸ごと採用** … 同領域の **上書きリファクタ**（新しく完全）のとき。前回:
  paste 再監査（`clipboard/paste.ts`）、quick-insert `buildReplacementBlock`、
  `createTable` の keep/replace 化、selection API の作り直し、private `_` プレフィックス
  リネーム、`_apply`。クリーンに採るなら `git checkout --theirs <file>`。

- **upstream が我々の修正を包含**（意図が同じで実装が別/良い）→ 我々を捨て upstream 採用。
  前回: `#4448` TOC オープン時表示（=我々の `#19`）、focus 復元（=`#2`）、quick-insert の
  テキスト引継ぎ（=code/math/html の `#7`）、テーブルのテキスト保持（upstream はブロックを
  残す方式）。

- **フォーク固有の直交機能を保持** … upstream の新構造へ **再適用** する（古いコードをそのまま
  残すと新リファクタに噛み合わない）。前回: 空行往復（`preserveEmptyLines` ＋ 見出し復元 ＋
  fence-info `meta.info` — **parse 側 `markdownToState` と serialize 側 `stateToMarkdown` の両方**）、
  PDF描画、`_extendSelectionAcrossBlocks`、`flushPendingChanges`、`main.ts` の Tauri boot、
  新規ドキュメントのキャレット（`#15/#16`）、whole-line paste（`df174e68`）。

- **upstream が我々の機能を包含しない**（挙動を削る/変える）→ **手を止めてユーザーに報告**。
  黙って落とさない。判断の前に **実エンジンプローブ（§5）で upstream の実挙動を検証** し、
  情報を揃える。前回 whole-line paste がこのケース: upstream は `alpha\n` をインライン統合
  （`be|ta`→`bealphata`、改行消失）。ユーザーは行分割挙動の再適用を選択した。

**ハマりどころ:**

- **locale（9ファイル）**: upstream は大規模再構成（#4516 系のキー移動＋minify＋新ロケール）。
  theirs を採用（`git checkout --theirs`）し、**Remarks リブランドを再適用**:
  `perl -i -pe 's/MarkText/Remarks/g'` で安全（upstream のキーに `MarkText` は無い＝
  `grep -c 'MarkText"[[:space:]]*:'` 各 locale で 0 を確認、リブランドは値のみ）。
  `.min.json` は gitignore（ビルドで再生成）。各ファイル `node -e "JSON.parse(...)"` で検証。
- **pnpm-lock.yaml**: 手マージ禁止。`git checkout --theirs pnpm-lock.yaml` →
  `pnpm install --no-frozen-lockfile`（ネット必要＝サンドボックス無効）でマージ後の
  package.json から再生成。
- **package.json**: 多くは両者が同じ箇所に別の依存を追加しただけ → 両方残す。

## 3. 波及を直す（typecheck 駆動）

upstream の広範なリネーム/再構成は **非衝突ファイルの** フォークコードも壊す。マーカー解決後、
typecheck で残りを洗い出す:

```bash
pnpm -C packages/muya run lint:types      # まず muya
pnpm -C packages/desktop run typecheck    # 次に desktop
```

前回の典型修正:
- フォークコード内の `this.muya`→`this._muya`、`this.apply`→`this._apply`（private リネーム）。
- 旧 selection 形状 `{ anchorBlock, focus }` → 新 `{ anchor:{block}, focus:{offset,block} }`
  （例 `_extendSelectionAcrossBlocks`）。
- 再導入するフォーク追加オプション（例 `preserveEmptyLines`）はインターフェースで **optional** に
  する（upstream の多数の呼び出し/spec を壊さないため）。
- **pdfjs-dist / `DOMMatrix is not defined`**: upstream の新テストが我々の `loadPdfPage.ts` を
  間接 import し、トップレベル `import 'pdfjs-dist'` が Node テスト環境で落ちる。**遅延 import**
  に変える（バンドルも code-split される）。polyfill ではなく遅延化。

## 4. lint（パッケージごと）

`muya` は antfu（4スペース・セミコロン）、`desktop` は 2スペース・セミコロン無し。
`eslint --fix` を **パッケージごと** に実行（`pnpm -C packages/muya exec eslint <files> --fix`）。
フォーク追加は muya の複雑度 ≤20 を守る（ヘルパー抽出 — 例: `space`/`heading` で共有する空行生成）。
`src-tauri/target/**`・`src-tauri/gen/**`（ビルド生成物）や、触っていない `platform/*` の既存
`no-void` は無視（マージ起因でなく CI のクリーン環境では出ない）。判定は `packages/*/src` のみ
lint する。

## 5. 型だけでなく挙動を検証

旧エンジン API 前提のフォークテストは、assertion 差分でなく **`TypeError`**（モック形状不一致）で
落ちる — これは挙動退行の証拠にならない。そうした spec は **実エンジン方式**（upstream のスタイル）
へ書き換える: happy-dom で `Muya` を起動 → 新形状で `getSelection()` をスタブ → 実ハンドラを駆動 →
`getMarkdown()` をアサート。雛形: `clipboard/__tests__/pasteBlockMerge.spec.ts`。
同じハーネスを使い捨てプローブにして、保持/破棄を決める前に upstream の実挙動を確認する。

## 6. フル検証バッテリ（全て緑であること）

```bash
pnpm -C packages/muya run lint:types
pnpm -C packages/muya run check-circular
pnpm -C packages/muya test                 # unit
pnpm -C packages/muya run test:spec        # CommonMark+GFM — 1347 固定、維持必須
pnpm -C packages/desktop run typecheck
pnpm -C packages/desktop run test:unit
```

その後、実アプリをビルドしてユーザーに触ってもらう（再適用したフォーク機能＋upstream の新機能を重点）:

```bash
export PATH="$HOME/.cargo/bin:$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
pnpm -C packages/desktop run build:tauri   # サンドボックス無効
open packages/desktop/src-tauri/target/release/bundle/macos/Remarks.app
```

## 7. コミット & push

`grep -rn '^<<<<<<<' packages/*/src` が空なのを確認 → `git add -A` → マージコミットに記載:
upstream が持ち込んだもの、採用/保持の判断、波及修正、緑の検証件数。**push はユーザー承認後のみ**
（`git push origin main`）。`git rev-list --count main..upstream-sync` が `0` になること。

## チェックリスト
- [ ] マージはサンドボックス無効で実行。失敗試行の残骸を `git clean` 済み。
- [ ] マージ前にミラーが `upstream/develop` と一致を確認。
- [ ] 「upstream が包含しない」機能は全てユーザーに報告し、upstream の実挙動を実エンジンプローブで検証。
- [ ] フォーク固有のエンジン機能は parse/serialize 両側に再適用（空行・fence-info 等）。
- [ ] 再導入したフォークオプションは optional 化し upstream 呼び出しをコンパイル可能に。
- [ ] locale はリブランド再適用・JSON 妥当・lock 再生成（手マージしない）。
- [ ] conformance 1347 維持。`test/spec/expected-failures.json` の固定が退行していない。
- [ ] `gh` コマンドは `-R hisaboh/remarks` 指定。

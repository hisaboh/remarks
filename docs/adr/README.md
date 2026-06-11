# Architecture Decision Records

Remarks on Markdown(marktext フォーク)の主要なアーキテクチャ決定の記録。
形式は Nygard スタイル(コンテキスト / 決定 / 検討した代替案 / 結果)。

| # | タイトル | ステータス |
|---|---|---|
| [0001](0001-electron-to-tauri.md) | デスクトップシェルを Electron から Tauri 2.0 へ移行する | 採用・実施済み |
| [0002](0002-muyajs-to-muyajs-core.md) | エディタエンジンを muyajs から @muyajs/core へ切り替える | 採用・実施済み |

運用手順(リリース・署名・鍵管理・ユーザーデータ移行)は ADR ではなく
[`docs/release_tauri.md`](../release_tauri.md) を参照。

  以下をターミナル（fish shell）で実行してセットアップしてください。

  # Node.js 22 をインストールして切り替え
  nodebrew install v22.21.1
  nodebrew use v22.21.1

  # corepack で pnpm を有効化
  corepack enable
  corepack prepare pnpm@10.33.4 --activate

  その後、このリポジトリで初回なら:

  pnpm install

  macOS ビルド:

  pnpm run build:mac
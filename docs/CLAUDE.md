# RustyClaw — Claude Code 開発ルール

## 開発コマンド (Claude Code 専用)

Claude Code が直接実行するための基本コマンドです。

- **ビルド**: `cargo build`
- **テスト実行**: `cargo test`
- **静的解析 (Clippy)**: `cargo clippy --all-targets`
- **フォーマット適用**: `cargo fmt --all`

---

※ 開発環境情報、デプロイ手順、および共通のコーディング規約（Rust 2024 Edition、エラーハンドリング、ログ設計、OpenSSL排除ルールなど）は、[docs/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/README.md) に集約されています。開発時は必ずそちらを参照してください。

---

## スキル実行時の制約

- **計画書の保存先**: `superpowers:writing-plans` を使用する場合、保存先は必ず `docs/plans/` を指定してください（スキルのデフォルト `docs/superpowers/plans/` より優先）。
- **マージの `--no-ff` 必須**: `superpowers:finishing-a-development-branch` 等のスキルがマージを実行する場合も、必ず `--no-ff` オプションを付与してください。

## ドキュメント・Git 管理ルール

実装・コミット・ADR 起票の前に以下の 2 ファイルを必ず参照してください。

- **`ai-rules.md`** — Git・コミット・ドキュメント管理・ADR の共通ルール（Workflow 全体）
- **`docs/README.md` Section 7** — このプロジェクトの案件管理番号形式とコミット例

このプロジェクトの案件管理番号: `Phase XX-Y`（機能実装）/ `ISSUE-XX`（バグ・保留課題）



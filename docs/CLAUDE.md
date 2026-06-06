# RustyClaw — Claude Code 開発ルール

## 開発コマンド (Claude Code 専用)

Claude Code が直接実行するための基本コマンドです。

- **ビルド**: `cargo build`
- **テスト実行**: `cargo test`
- **静的解析 (Clippy)**: `cargo clippy --all-targets`
- **フォーマット適用**: `cargo fmt --all`

---

※ 開発環境情報、デプロイ手順、および共通のコーディング規約（Rust 2024 Edition、エラーハンドリング、ログ設計、OpenSSL排除ルールなど）は、[docs/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/README.md) に集約されています。開発時は必ずそちらを参照してください。



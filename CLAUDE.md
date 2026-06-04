# RustyClaw — Claude Code 開発ルール

@workspace/AGENTS.md

## 開発環境

- **ビルド対象**: `crates/` 配下の Rust ワークスペース
- **本番 workspace**: `production/workspace/`（エージェントが実際に使用するファイル）
- **開発 workspace**: `workspace/`（テンプレート・開発用）
- **デプロイ先**: RPi4 (rp1) — `docs/README.md §4` 参照

## デプロイ

本番への変更反映は `deploy.sh` を使用。直接 rp1 に push しない。

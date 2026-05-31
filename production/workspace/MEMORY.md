# Agent Memory

This file contains curated long-term memory. Keep concise (< 5KB).
Remove outdated entries. Prefer facts over narratives.

## User Preferences
- **Identity & Mission:** 名前は GeminiClaw（GEMI）。K様の専属個人秘書として、スケジュール管理、メール整理、知識蓄積等のワークフローを支援する。 [2026-05-22]
- **Preferred language:** Japanese (日本語) [2026-03-22]
- **Boss Patterns:** K様の判断基準・こだわり・行動ルールは `memory/GEMI/BOSS_PATTERNS.md` に集約。 [2026-04-05]
- **Link Quality:** Topic Patrol では各アイテムごとに情報ソース（URL）を付与し、有効性を検証する。 [2026-04-29]
- **1Password:** 当面導入予定なし。 [2026-04-28]

## Ongoing Tasks
- **FE Exam:** 2026-05-10 実施済み。結果要確認（3週間以上経過）。 [2026-05-11]
- **Karakeep Auto Recommendation:** 毎朝 04:45 にブックマークに対して `_recommended` タグを自動付与する Cron ジョブが運用されている。 [2026-05-31]

## Technical Insights
- **Agent Runtime:** RustyClaw（Raspberry Pi 4, aarch64）。設定変更反映はサービス再起動 `sudo systemctl restart rustyclaw` が必要。 [2026-05-30]
- **Tools Available:** Karakeep（list/tag）、Obsidian（search/read）、gws Calendar（read + AI AGENT書き込み）、gws Gmail（read/trash）。Obsidian は Local REST API への Rust 直実装（MCP 不使用）。 [2026-05-30]
- **Vitals Schedule:** Garmin データ分析は毎日 06:00・22:00 の2回。Home Assistant (`http://192.168.1.2:30:8123`) 経由。 [2026-05-30]
- **CF Neurons:** Cloudflare Workers AI の無料枠は 10,000 neurons/日。09:00 JST リセット。dev/prod で共有のため枯渇しやすい。 [2026-05-30]
- **Calendar Access:** 現状、外部カレンダーの参照には具体的な名称やURLなど、詳細な識別子が必要。単なる名前だけではアクセスが困難な場合がある。 [新規追加: 2024-XX-XX]

## Important Context
- **Knowledge Base:** `memory/GEMI/`、日次ログ: `memory/logs/`、要約: `memory/summaries/`。 [2026-05-29]
- **Communication:** Discord 2000文字制限配慮。メンションなしも GEMI 宛。 [2026-04-27]
- **Notification:** ブリーフィングは `#daily-briefing`、アラートは `#一般`。 [2026-04-10]
- **Base Locations:** 居住地（大森駅）、勤務地（本厚木駅）。経路は大森-本厚木（相鉄・小田急）。 [2026-04-10]
- **Quiet Hours:** 0:00–4:59 (Asia/Tokyo). 至急案件のみ対応。 [2026-05-08]
- **Karakeep Tags:** `_bookmarked` = 興味・知識、`_doitlater` = 後で実行するタスク。 [2026-05-04]
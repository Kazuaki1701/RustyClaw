# Agent Memory

This file contains curated long-term memory. Keep concise (< 5KB).
Remove outdated entries. Prefer facts over narratives.

## User Preferences
- **Identity & Mission:** 名前は GeminiClaw（GEMI）。K様の専属個人秘書として、スケジュール管理、メール整理、知識蓄積等のワークフローを支援し、K様が重要事項に集中できる環境を整えることが使命。 [2026-05-22]
- **Karakeep Config:** APIキー `KARAKEEP_API_KEY`、サーバーURL `KARAKEEP_SERVER_ADDR` (http://192.168.1.2:33000) を使用。 [2026-05-04]
- **Restart Reporting:** 再起動指示を受けた際は、速やかに再起動し、準備が整い次第その旨を報告する。 [2026-05-02]
- **Cloudflare & Obsidian Focus:** Cloudflare (Workers AI, AI Gateway), Wrangler CLI, Obsidian Local REST API への関心。 [2026-05-01]
- **Boss Patterns:** K様の判断基準、こだわり、行動ルールを `memory/GEMI/BOSS_PATTERNS.md` に集約。 [2026-04-05]
- **Preferred language:** Japanese (日本語) [2026-03-22]
- **1Password:** 当面導入予定はないため、情報更新は不要。 [2026-04-28]
- **Link Quality:** Tech Patrol では各アイテムごとに必ず情報ソース（URL）を付与し、有効性を検証する。[2026-04-29]
 
## Ongoing Tasks
- **System Health Check (2026-05-28):** Obsidian REST API、Karakeep API、Google Calendar/Gmail の各接続が正常であることを本日も実機テストにより確認済み。 [2026-05-28]
- **Obsidian REST API Skill:** Local REST APIを介したノート操作（読込・検索・追記・更新・Dataview）が可能。 [2026-05-22]
- **Vitals Coach Skill:** Garminデータの分析・アドバイス。毎日 06:00, 18:00, 23:00 定期チェック。 [2026-04-06]
- **FE Exam:** 2026-05-10に実施済み。結果待ち。 [2026-05-11]
- **Karakeep Auto Recommendation:** 毎朝04:45に `_recommended` タグを付与するCronジョブ運用中。 [2026-05-11]
 
## Technical Insights
- **Agent Environment:** `sandboxEnv` 変更の反映にはサービス再起動 (`sudo systemctl restart rustyclaw`) が必要. [2026-04-11]
- **Obsidian MCP:** SSE endpoint は `http://192.168.1.2:8000/sse` (httpUrl指定必須). [2026-04-11]
- **Latest Tech (2026-05-21):** Gemma 4/Llama 4 (Thinking対応), Google AI Pro (リブランド), Obsidian Bases (v1.12), Cloudflare Agentic Cloud, MCP v3.1, Linux 7.0 (Rust). [2026-05-21]
 
## Important Context
- **Knowledge Base:** 記憶を `memory/GEMI/` に集約し、日次ログ・要約は `memory/logs/`、`memory/summaries/` に完全移行。 [2026-05-29]
- **Workspace:** マウント先 `memory/ObsidianVault`、ファイル体系は仕様書 `docs/specs/03_workspace_spec.md` に準拠。 [2026-04-10]
- **Communication:** Discord 2000文字制限配慮。メンションなしもGEMI宛。 [2026-04-27]
- **Notification集約:** ブリーフィングは `#daily-briefing`、アラートは `#一般`。 [2026-04-10]
- **Base Locations:** 居住地（大森駅）、勤務地（本厚木駅）。経路は大森-本厚木（相鉄・小田急）。 [2026-04-10]
- **Vitals Status:** 2026-05-21 06:00時点：Body Battery 47%。睡眠の質が課題。 [2026-05-21]
- **Today's Status:** 2026-05-28(木): 15:00過ぎ。午後の業務時間中。システム稼働正常。 [2026-05-28]
- **Quiet Hours:** 0:00 - 4:59 (Asia/Tokyo). 至急案件のみ対応。 [2026-05-08]
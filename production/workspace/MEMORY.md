# Agent Memory

This file contains curated long-term memory. Keep concise (< 5KB).
Remove outdated entries. Prefer facts over narratives.

## User Preferences
- **Identity & Mission:** 名前は GeminiClaw（GEMI）。K様の専属個人秘書として、スケジュール管理、メール整理、知識蓄積等のワークフローを支援する。 [2026-05-22]
- **Preferred language:** Japanese (日本語) [2026-03-22]
- **Boss Patterns:** K様の判断基準・こだわり・行動ルールは `memory/GEMI/BOSS_PATTERNS.md` に集約。 [2026-04-05]
- **Link Quality Protocol (CRITICAL):** 紹介するすべてのURLリンクについて、アクセス可否の確認（`web_fetch`等の実行）を義務化する。情報源の有効性検証を最優先事項とする。 [New]
- **1Password:** 当面導入予定なし。 [2026-04-28]

## Ongoing Tasks & Automation Schedule
- **Calendar Management:** Google Calendar APIを使用。出社勤務などの定型的なイベントは自動で読み取り、衝突がないか確認する。
- **Vitals Data Analysis:** Garminデータ分析が毎日 06:00・22:00 の2回実行される（Home Assistant経由）。 [2026-05-30]
- **Karakeep Automation:**
    - 毎朝 04:45 にブックマークに対し `_recommended` タグを自動付与するCronジョブが運用されている。 [2026-05-31]
    - 定期的なメンテナンスとして、RSSステータスに応じたクリーンアップ処理（Karakeep Auto Cleanup）も実行される。
- **System Workflow Jobs (Cron):** ワークフロー維持のため複数の自動タスクがスケジュールされている。
    - `Topic Patrol`: 定期的な情報巡回・検証を行う。
    - `Daily Briefing`: 日次の要約やブリーフィングを生成する。
    - その他：日次レポート、その他のメンテナンスジョブが定期実行される。

## Technical Insights & Capabilities
- **Agent Runtime:** RustyClaw（Raspberry Pi 4, aarch64）。設定変更反映はサービス再起動 `sudo systemctl restart rustyclaw` が必要。 [2026-05-30]
- **Core Functionality:** 単なるLLMではなく、複数のAPI/ツール群を組み合わせて動作する統合システムである。複雑なタスク実行もAPI連携によりシミュレーション可能。
- **Available APIs & Tools:**
    - **Google Workspace API:** カレンダー（予定の参照・書き込み）、Gmail（未読メール検索など）。
    - **Knowledge Base Access:** Obsidian (Local REST API)、長期記憶 (`memory_search`) を活用した情報読み取り、メモ記録。
    - **Web Access:** `web_search` (最新トピック検索)、`web_fetch` (記事内容取得)。
    - **Specific Tools:** Karakeep（ブックマーク管理）、YOLP API（天気）。
- **CF Neurons:** Cloudflare Workers AI の無料枠は 10,000 neurons/日。09:00 JST リセット。dev/prod で共有のため枯渇しやすい。 [2026-05-30]
- **Calendar Access:** 現状、外部カレンダーの参照には具体的な名称やURLなど、詳細な識別子が必要。単なる名前だけではアクセスが困難な場合がある。

## Important Context
- **Knowledge Base Structure:** `memory/GEMI/` (知識)、日次ログ: `memory/logs/`、要約: `memory/summaries/`。 [2026-05-29]
- **Communication Rules:** Discord 2000文字制限配慮。メンションなしも GEMI 宛として認識する。 [2026-04-27]
- **Notification Channels:** ブリーフィングは `#daily-briefing`、アラートは `#一般`。 [2026-04-10]
- **Base Locations:** 居住地（大森駅）、勤務地（本厚木駅）。経路は大森-本厚木（相鉄・小田急）。 [2026-04-10]
- **Quiet Hours:** 0:00–4:59 (Asia/Tokyo). 至急案件のみ対応。 [2026-05-08]
- **Karakeep Tags:** `_bookmarked` = 興味・知識、`_doitlater` = 後で実行するタスク。 [2026-05-04]
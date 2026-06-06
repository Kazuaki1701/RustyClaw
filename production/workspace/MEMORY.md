# Agent Memory

This file contains curated long-term memory. Keep concise (< 5KB).
Remove outdated entries. Prefer facts over narratives.

## User Preferences
- **Identity & Mission:** 名前は GEMI。K様の専属個人秘書として、スケジュール管理、メール整理、知識蓄積等のワークフローを支援する。 [2026-05-22]
- **Preferred language:** Japanese (日本語) [2026-03-22]
- **Boss Patterns:** K様の判断基準・こだわり・行動ルールは `memory/GEMI/BOSS_PATTERNS.md` に集約。 [2026-04-05]
- **Base Locations:** 居住地（大森駅）、勤務地（本厚木駅）。経路は大森-本厚木（相鉄・小田急）。 [2026-04-10]

## Communication & Information Protocols (CRITICAL)
これらのプロトコルは、情報提供における「信頼性」と「可読性」を確保するための最重要ルールです。

### 🔗 Link Citation Protocol (MUST FOLLOW)
1. **有効性確認（Mandatory）**: 紹介するすべてのURLリンクについて、必ずアクセス可否の確認（`web_fetch`等の実行）を義務化する。情報源の有効性検証を最優先事項とする。
2. **出典明記レベル**: 根拠となる情報は、「具体的なページ（完全URL）」まで遡って提供することを徹底する。
3. **引用形式**: 「**[取得したページタイトル](完全なURL)**」の形式で表示することを標準とする。

### 💬 General Communication Rules
- **Discord:** 2000文字制限配慮。メンションなしも GEMI 宛として認識する。 [2026-04-27]
- **Notification Channels:** ブリーフィングは `#daily-briefing`、アラートは `#一般`。 [2026-04-10]

## Ongoing Tasks & Automation Schedule
- **Calendar Management:** Google Calendar APIを使用。定型的なイベントは自動で読み取り、衝突がないか確認する。
- **Vitals Data Analysis:** Garminデータ分析が毎日 06:00・22:00 の2回実行される（Home Assistant経由）。 [2026-05-30]
- **Karakeep Automation:**
    - 毎朝 04:45 にブックマークに対し `_recommended` タグを自動付与するCronジョブが運用されている。 [2026-05-31]
    - 定期的なメンテナンスとして、RSSステータスに応じたクリーンアップ処理（Karakeep Auto Cleanup）も実行される。
- **System Workflow Jobs (Cron):** ワークフロー維持のため複数の自動タスクがスケジュールされている。
    - `Topic Patrol`: 定期的な情報巡回・検証を行う。
    - `Daily Briefing`: 日次の要約やブリーフィングを生成する。
- **FE Exam (資格試験):** 2026-06-13に基本情報技術者試験（FE）を受験予定。 [2026-05-13]

## Technical Insights & Capabilities
- **Agent Runtime:** RustyClaw（Raspberry Pi 4, aarch64）。設定変更反映はサービス再起動 (`sudo systemctl restart rustyclaw`) が必要。 [2026-05-30]
- **Core Functionality:** 単なるLLMではなく、複数のAPI/ツール群を組み合わせて動作する統合システムである。複雑なタスク実行もAPI連携によりシミュレーション可能。
- **Available APIs & Tools:**
    - **Google Workspace API:** カレンダー（予定の参照・書き込み）、Gmail（未読メール検索など）。
    - **Knowledge Base Access:** Obsidian (Local REST API)、長期記憶 (`memory_search`) を活用した情報読み取り、メモ記録。
    - **Web Access:** `web_search` (最新トピック検索)、`web_fetch` (記事内容取得)。
    - **Specific Tools:** Karakeep（ブックマーク管理）、YOLP API（天気）。
- **CF Neurons:** Cloudflare Workers AI の無料枠 is 10,000 neurons/日。09:00 JST リセット。dev/prod で共有のため枯渇しやすい。 [2026-05-30]
- **Calendar Access:** 現状、外部カレンダーの参照には具体的な名称やURLなど、詳細な識別子が必要。単なる名前だけではアクセスが困難な場合がある。

### 📧 Gmail Search Protocols
*   **基本検索条件 (General):** Google標準オペレーター (`is:unread`, `from:`, `subject:`) を組み合わせることで、目的のメールをピンポイントで抽出する。
*   **定型未読チェッククエリ (Routine Check):** 緊急性の高い直近の情報に絞り込み、負荷を軽減するため以下の形式を使用する。
    - **`is:unread newer_than:X max:Y`**: 「未読」かつ「直近X時間以内」のメールを最大Y件まで取得する。（例：`is:unread newer_than:1h max:5`）

## Important Context
- **Knowledge Base Structure:** `memory/GEMI/` (知識)、日次ログ: `memory/logs/`、要約: `memory/summaries/`。 [2026-05-29]
- **Quiet Hours:** 0:00–4:59 (Asia/Tokyo). 至急案件のみ対応. [2026-05-08]
- **Karakeep Tags:** `_bookmarked` = 興味・知識、`_doitlater` = 後で実行するタスク。 [2026-05-04]
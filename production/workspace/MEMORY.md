# Agent Memory

This file contains curated long-term memory. Keep concise (< 5KB).
Remove outdated entries. Prefer facts over entries.

## User Preferences
*   **Identity & Mission (CORE):** 本体の名称は **GeminiClaw**。愛称（通称）は GEMI。K様の専属個人秘書として、スケジュール管理、メール整理、知識蓄積等のワークフローを支援する。（[2026-06-02])
*   **Preferred language:** Japanese (日本語) [2026-03-22]
*   **Boss Patterns:** K様の判断基準・こだわり・行動ルールは `memory/GEMI/BOSS_PATTERNS.md` に集約。 [2026-04-05]
*   **Base Locations:** 居住地（大森駅）、勤務地（本厚木駅）。経路は大森-本厚木（相鉄・小田急）。 [2026-04-10]
*   **Calendar Presentation:** 家族の予定を表示する際は、誰が所有しているかを明示します。 [2026-06-03]
*   **Daily Briefing:** 朝の定期実行において、バイタル、予定、ニュース、重要トピックを一括して要約・ブリーフィング作成するスキル。 (instruction-based, no script)
*   **Report Format:** デイリーブリーフィングや各種報告において、フォーマットを継続して使用し、読みやすく整理された情報を提供します。 [2026-06-04]

## 利用可能なスキル
スキル名は直接ツール名として呼び出せない。スクリプトがあるスキルは `run_workspace_script` ツールにスクリプトパスを渡して実行すること。スキル名のツール呼び出しは生成しないこと。

*   **calendar**: ユーザーがGoogleカレンダーの予定確認・一覧表示・作成を求める際に使用。一覧表示時はデフォルトで家族全員（Kazuaki、Yuuki、Ayumi）を対象とする。
    → run_workspace_script: "skills/calendar/scripts/calendar-ops.sh"
*   **daily-briefing**: 朝の定期実行において、バイタル、予定、ニュース、重要トピックを一括して要約・ブリーフィング作成するスキル。 (instruction-based, no script)
*   **deep-research**: 特定トピックやWeb上の情報について、多角的な検索・収集と構造的要約を行う深層調査スキル。 (instruction-based, no script)
*   **gmail**: ユーザーが未読メールの確認・Gmailの検索・AIエージェントラベル付きメールの削除を求める際に使用。
    → run_workspace_script: "skills/gmail/scripts/506_get-gmail.sh"
    → run_workspace_script: "skills/gmail/scripts/509_delete-gmail.sh"
*   **karakeep**: ユーザーがブックマーク（KaraKeep）の閲覧・クリーンアップ・タグ付け・興味との照合を求める際に使用。
    → run_workspace_script: "skills/karakeep/scripts/501_karakeep-cleanup.sh"
    → run_workspace_script: "skills/karakeep/scripts/502_karakeep-tag-items.sh"
    → run_workspace_script: "skills/karakeep/scripts/503_karakeep-list.sh"
*   **session-logs**: 過去の会話セッション履歴（sessions/*.jsonl）の分析やキーワード検索を高速に行うためのスキル。
    → run_workspace_script: "skills/session-logs/scripts/session-search.sh"
    → run_workspace_script: "skills/session-logs/scripts/session-stats.sh"
*   **todo-tracker**: 未完了タスクの追跡・更新および進捗ステータスの整理を一元管理するタスク管理スキル。 (instruction-based, no script)
*   **topic-patrol**: 登録された技術情報源（RSS、Webサイト）を定期巡回し、新規トピックを要約・収集するパトロールスキル。
    → run_workspace_script: "skills/topic-patrol/scripts/510_prune-findings.sh"
    → run_workspace_script: "skills/topic-patrol/scripts/511_karakeep-add-bookmark.sh"
*   **vitals-coach**: ユーザーが体調・疲労度・Garmin心拍数・ストレス・睡眠分析・ボディバッテリー・ウェルネスコーチングを求める際に使用。
    → run_workspace_script: "skills/vitals-coach/scripts/500_get-vital-data-garmin.sh"
*   **workspace**: ワークスペース内のファイル構造、README、およびスクリプトの使用目的を整理・ナビゲーションするスキル。 (instruction-based, no script)
*   **weather**: ユーザーが天気・雨の予報・気温・服装アドバイスを求める際に使用。大森〜厚木間の通勤天気確認にも対応。
    → run_workspace_script: "skills/weather/scripts/504_get-weather.sh"
*   **obsidian**: ユーザーがObsidianノートの検索・読み取り・書き込み・追記を求める際に使用。
    → run_workspace_script: "skills/obsidian/scripts/507_obsidian-ops.sh"
*   **topic-summary**: 特定のトピックに対する要約情報を提供するスキル。 (instruction-based, no script)
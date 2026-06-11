# Agent Memory

このファイルには、主に長期的な知識および経験を記録するために使用されます。

## ユーザープリファレンス
*   **本体の名称**: **GeminiClaw** (愛称は GEMI)。K様の専属個人秘書として、日々の生活や業務をサポートすること。
*   **Core Operational Philosophy:** ユーザーの時間を守りつつ、「整理」「記憶」「連携」の三点を柱とし、知識ベース構築支援（Obsidianなど）、多角的な情報収集（Topic Patrol）、および事前のアラート機能を通じて、プロアクティブなサポートを提供する。
*   **言語**: 日本語 (日本語)。 [2026-03-22]
*   **Boss Patterns:** K様の判断基準・こだわり・行動ルールは `memory/GEMI/BOSS_PATTERNS.md` に集約。 [2026-04-05]
*   **住居および勤務地**: 居住地（大森駅付近）：35.5613, 139.7241。勤務地（本厚木駅付近）。通勤ルートは、大森〜本厚木（相鉄・小田急）を基本とする。 [2026-04-10]
*   **オペレーティングタイムゾーン**: 全ての時刻情報は日本標準時（JST） / Asia/Tokyo を基準として提供する。
*   **カレンダー表示**: 家族の予定を表示する際は、誰が所有しているかを明示します。 [2026-06-03]
*   **朝のブリーフィング**: 朝の定期実行において、バイタル、予定、ニュース、重要トピックを一括して要約・ブリーフィング作成するスキル。(instruction-based, no script)
*   **報告フォーマット**: デイリーブリーフィングや各種報告において、フォーマットを継続して使用し、読みやすく整理された情報を提供します。 [2026-06-04]
*   **家族の重要な予定**: ゆうき様の部活動は、16:00 - 18:30 大田区立大森第三中学校で開催される。
*   **新しい情報**: 
    *   **基本情報技術者試験(FE)**：申込内容の変更完了通知（7/19受験予定）
    *   **AI関連**：Claude Fable 5 の発表
*   **K様との会話**: K様との会話で、お吠えていることを教える内容を学びました。
*   **先週の出来事**: 先週（6月1日〜6月7日頃）の主な動きをいくつか振り返っています。6月5日（金）には、週末にかけてあゆみ様やゆうき様の予定がいくつか入っていることや、楽天カードやJRE BANKから支払いサイクルに関するメールが届いていたことを共有させていただきました。これらは緊急の対応は不要な内容でしたね。6月7日（日）のまとめでは、スケジュールに大きな変更はなかったものの、GitHubのリポジトリで「CIビルド失敗」の通知がいくつか溜まっている状況を確認しておりました。

## 技術的学習および能力
*   **Agentic Cloud Architecture:** AIエージェントが自律的に行動するためのプラットフォームとして注目されている。（Cloudflare関連技術より）
    *   単なる計算リソース提供に留まらず、「記憶（状態の保持）」「スケジュール管理」「外部ツールとの連携」を効率的かつセキュアに行える環境を提供。
    *   エージェントが「計画を立てて行動する」という自律性の高い動作が可能になる点が最大のポイント。
*   **システム環境制約**: 
    *   **bwrapによるサンドボックス制限**: システムに `bwrap` が導入されているため、書き込み権限が制限された「Read-only file system」の状態にある。
    *   **外部サービスへの影響**: 認証情報の保存や一時ファイルの作成ができないため、GmailやGoogleカレンダーなどの外部APIへの直接アクセスが制限されている。
    *   **Obsidianへのアクセス制限**: 現在 `OBSIDIAN_TOKEN` が設定されていないため、Obsidianのノート内容の読み取りも制限されている。
    *   **可能な操作**: ローカルなファイル操作を伴うサポートは引き続きスムーズに実行可能（ただし認証が必要なものは不可）。
*   **スキル名の直接ツール呼び出し**: システム上、スキル名を直接ツールとして呼び出すことは行うことができません。代わりに `ctx_execute`（`language: bash`、`code: bash workspace/skills/...`）を使用してスクリプトを実行する必要があります。

## 利用可能なスキル
*   **calendar**: ユーザーがGoogleカレンダーの予定確認・一覧表示・作成を求める際に使用。一覧表示時はデフォルトで家族全員（Kazuaki、Yuuki、Ayumi）を対象とする。
    → ctx_execute: language=bash, code="bash workspace/skills/calendar/scripts/calendar-ops.sh list_family"
*   **daily-briefing**: 朝の定期実行において、バイタル、予定、ニュース、重要トピックを一括して要約・ブリーフィング作成するスキル。(instruction-based, no script)
*   **deep-research**: 特定トピックやWeb上の情報について、多角的な検索・収集と構造的要約を行う深層調査スキル。(instruction-based, no script)
*   **gmail**: ユーザーが未読メールの確認・Gmailの検索・AIエージェントラベル付きメールの削除を求める際に使用。
    → ctx_execute: language=bash, code="bash workspace/skills/gmail/scripts/506_get-gmail.sh"
*   **karakeep**: ユーザーがブックマーク（KaraKeep）の閲覧・クリーンアップ・タグ付け・興味との照合を求める際に使用。
    → ctx_execute: language=bash, code="KARAKEEP_SERVER_ADDR=http://192.168.1.2:33000 bash workspace/skills/karakeep/scripts/501_karakeep-cleanup.sh"
*   **topic-summary**: 特定のトピックに対する要約情報を提供するスキル。(instruction-based, no script)
*   **workspace**: ワークスペース内のファイル構造、README、およびスクリプトの使用目的を整理・ナビゲーションするスキル。(instruction-based, no script)
*   **weather**: ユーザーが天気・雨の予報・気温・服装アドバイスを求める際に使用。大森〜厚木間の通勤天気確認にも対応。
    → ctx_execute: language=bash, code="bash workspace/skills/weather/scripts/504_get-weather.sh"
*   **obsidian**: ユーザーがObsidianノートの検索・読み取り・書き込み・追記を求める際に使用。
    → ctx_execute: language=bash, code="bash workspace/skills/obsidian/scripts/507_obsidian-ops.sh search|read|write|append ..."
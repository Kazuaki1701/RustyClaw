# Completed Phases (24, 36, 38) — RustyClaw

アーカイブ化された完了済み・スキップ済みのフェーズ一覧です。
アーカイブ日: 2026-06-04

---

### Phase 38: Topic Patrol の品質改善 🔴
> 2026-06-03 ログ点検で判明した実施状況とルールのギャップ（調査記録: `docs/2026-06-02-log-inspection-report.md`）。
> 根本方針: 「モデルに計算・判断を任せない。Rust が全ての状態管理・制御を担い、モデルは与えられた情報で動くだけにする」

- `[x]` **1. トピック選択を SKILL.md 内の指示に変更（rotationIndex 廃止）**
  - Rust 側での算術管理を廃止。モデルが自身で選択するシンプルな方式に変更。
  - SKILL.md Step 1 に「`patrol/findings.md` の直近 `##` セクションに登場しない2件を選ぶ」指示を追加。自然なローテーションを算術なしで実現。
  - `patrol/state.json` から `rotationIndex` フィールドを削除（`lastRun` のみ残す）。
  - 対象: `production/workspace/skills/topic-patrol/SKILL.md`（実装済み）

- `[x]` **2. 探索ジョブ（深夜）と配信ジョブ（日中）に分離**
  - `topic-patrol`（interval:360）を廃止し `topic-patrol-explore`（cron:02:00）と `topic-patrol-deliver`（cron:09:00）に分離。
  - `prompt` フィールドに `配信: スキップ / 許可` を直接埋め込む方式。Rust 変更不要。
  - 対象: `production/workspace/cron.json`（実装済み）

- `[x]` **3. findings.md のプルーニングをスクリプトで実施**
  - `skills/topic-patrol/scripts/510_prune-findings.sh` として実装済み。SKILL.md の Step 5-0 で呼び出す。

- `[x]` **4. SKILL.md を 2モード対応に更新**
  - 配信モード（Deliver Mode）独立フローを追加。`配信: 許可` 時は deferred findings を読んで Discord 送信 → KaraKeep 登録 → delivered 記録。
  - `配信: スキップ` は探索モード（既存フロー）。
  - 対象: `production/workspace/skills/topic-patrol/SKILL.md`（実装済み）

- `[x]` **P1-A: 探索件数の数値不整合を修正（Step 1 を 3件に変更した際の取りこぼし）**
  - Step 2 ヘッダー `each of the 2 selected topics` → `3`
  - Step 2 Work-adjacent `After investigating the 2 selected topics` → `3`
  - Prohibited Patterns 末尾 `Picking the same 2 topics` → `3`
  - 関連ファイル:
    - `production/workspace/skills/topic-patrol/SKILL.md`（修正対象）
    - `docs/2026-06-04-topic-patrol-deliver-skill-review.md`（調査詳細）

- `[x]` **P1-B: KaraKeep 二重登録リスクの解消**
  - 現状: Deliver Mode Step 7 と Step 5-2 の両方に `511_karakeep-add-bookmark.sh` の呼び出しが存在し、配信モードで二重登録が起きる恐れがある。
  - 修正: KaraKeep 登録を Deliver Mode Step 7 に一本化。Step 5-2 の適用条件を探索モード（`配信: スキップ`）限定に限定するか削除する。
  - 関連ファイル:
    - `production/workspace/skills/topic-patrol/SKILL.md`（修正対象）
    - `production/workspace/skills/topic-patrol/scripts/511_karakeep-add-bookmark.sh`（登録スクリプト）
    - `production/workspace/patrol/findings.md`（delivered 記録先）
    - `docs/2026-06-04-topic-patrol-deliver-skill-review.md`（調査詳細）

- `[x]` **P2-A: Execution Flow と Deliver Mode Step 8 の参照矛盾を統一**
  - 「skip to Step 5」と「Skip Steps 1–5」の記述が矛盾。配信モードでは Step 5-3（state.json 更新）のみ実行が正しい。
  - 関連ファイル:
    - `production/workspace/skills/topic-patrol/SKILL.md`（修正対象）
    - `production/workspace/patrol/state.json`（更新対象の state ファイル）
    - `docs/2026-06-04-topic-patrol-deliver-skill-review.md`（調査詳細）

- `[x]` **P2-B: `配信: スキップ (quiet hours)` の表記を cron.json に合わせる**
  - Step 4 の表記を `配信: スキップ`（括弧なし）に統一。
  - 関連ファイル:
    - `production/workspace/skills/topic-patrol/SKILL.md`（修正対象）
    - `production/workspace/cron.json`（実際のプロンプト値の参照元）
    - `docs/2026-06-04-topic-patrol-deliver-skill-review.md`（調査詳細）

- `[x]` **P2-C: 「配信済み」判定基準を Deliver Mode Step 2 に明記**
  - `patrol/findings.md` 内に `delivered` ステータスで記録済みのエントリは配信済みとみなす旨を明記。
  - 関連ファイル:
    - `production/workspace/skills/topic-patrol/SKILL.md`（修正対象）
    - `production/workspace/patrol/findings.md`（判定対象のデータファイル）
    - `docs/2026-06-04-topic-patrol-deliver-skill-review.md`（調査詳細）

- `[x]` **P3-A: 配信モードの選択基準を独立記述し Step 3 参照を廃止**（任意）
  - Deliver Mode Step 3 の「Step 3 below 参照」を廃止し、配信モード内に選択基準を直接記述。
  - 関連ファイル:
    - `production/workspace/skills/topic-patrol/SKILL.md`（修正対象）
    - `docs/2026-06-04-topic-patrol-deliver-skill-review.md`（調査詳細）

- `[-]` **5. patrol の primary モデルを cf-gemma-4-26b に変更**（オプション）
  - 現在 `["lms-gemma-4-e4b", "groq-llama-8b"]` → `["cf-gemma-4-26b", "lms-gemma-4-e4b"]` に変更。
  - 今回の改善効果を確認してから判断。
  - 対象: `production/config/config.release.json`（`agents.patrol`）

- `[x]` **6. GeminiClaw からの Source Routing 拡張移植**
  - `github:{owner}/{repo}` と `rss:{url}` を SKILL.md Source Routing テーブルに追加。
  - 対象: `production/workspace/skills/topic-patrol/SKILL.md`（実装済み）

- `[x]` **7. クエリカテゴリの拡張（Work-adjacent）**
  - Work Context に基づくオプショナルクエリを Step 2 末尾に追加。
  - 対象: `production/workspace/skills/topic-patrol/SKILL.md`（実装済み）

- `[x]` **8. USER.md Interests への sources: 指定の整備**
  - 全9トピックに `sources:` を追記（HN / Reddit / github: / URL）。
  - 対象: `production/workspace/USER.md`（実装済み）

---

### Phase 36: 残存するネイティブツールの完全スキル化・疎結合化 🔴
> 設計書・実装計画策定済み（2026-05-31）。各 Phase の spec/plan は `docs/superpowers/` に保存。

| Phase | 設計書 | 実装計画 |
|---|---|---|
| A: Weather | `specs/2026-05-31-weather-skill-design.md` | `plans/2026-05-31-weather-skill-migration.md` |
| B: Calendar | `specs/2026-05-31-calendar-gmail-skill-design.md` | `plans/2026-05-31-calendar-gmail-skill-migration.md` |
| C: Gmail | ↑ 同上 | ↑ 同上 |
| D: Obsidian | `specs/2026-05-31-obsidian-skill-design.md` | `plans/2026-05-31-obsidian-skill-migration.md` |

- `[x]` **1. 天気予報のスキル化（Phase A）**
  - `skills/weather/` スキルフォルダを新設。
  - `504_get-weather.sh`（大森・厚木2地点、気温・風速・今日最高/最低・60分降水量）。
  - `YolpWeatherTool` 構造体およびテストの削除。

- `[x]` **2. Googleカレンダーの予定管理スキル化（Phase B）**
  - `skills/calendar/` スキルフォルダを新設。
  - `505_get-calendar.sh`（7日間予定、title/start/end/location のみ抽出）。
  - `508_write-calendar.sh`（許可 Calendar ID 2件ハードコードガード内蔵）。
  - `GwsCalendarTool`, `GwsCalendarWriteTool` 構造体およびテストの削除。

- `[x]` **3. Gmailメッセージ取得・ゴミ箱化のスキル化（Phase C）**
  - `skills/gmail/` スキルフォルダを新設。
  - `506_get-gmail.sh`（id/sender/subject/date/snippet の5フィールド抽出）。
  - `509_delete-gmail.sh`（`_ai-agent` ラベル存在検証ガード内蔵）。
  - `GwsGmailTool`, `GwsGmailDeleteTool` 構造体およびテストの削除。

- `[x]` **4. Obsidian 操作の統一スキル化（Phase D）**
  - `skills/obsidian/` スキルフォルダを新設。
  - `507_obsidian-ops.sh`（search/read/write/append サブコマンド統合、`$vault:obsidian-api-key` 注入）。
  - `ObsidianSearchTool`, `ObsidianReadTool`, `ObsidianWriteTool` および `percent_encode()` の削除。

- `[x]` **5. ゲートウェイ自動登録の解除と cargo test のオールグリーン検証**

- `[-]` **6. RPi4 実機検証と deploy.sh による配備**

---

### Phase 24: LLM 接続プロバイダ層の耐障害性（レジリエンス）強化 🔴
> GeminiClaw は 429 検知・バックオフおよびモデルフォールバックを実装済み。RustyClaw での同等機能。
>
> **2026-06-01 再検討結果**: Item 1（指数バックオフ）は `complete_with_fallback()` の多段フォールバックチェーンで実質カバー済みのため不要と判断。Item 2 は設計上のバグとして再定義（下記参照）。

- `[x]` **1. LLM プロバイダ層へのネットワークリトライ**
  - `complete_with_fallback()` の多段モデルチェーンが実質的に同等の役割を担っており、追加実装不要と判断。

- `[x]` **2. GLOBAL_COOLDOWN を Per-provider クールダウンへリファクタ（GLOBAL_COOLDOWN 削除）**
  - `PROVIDER_COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>>` による per-provider 管理に変更。`set_provider_cooldown_from_error()` / `set_provider_cooldown()` / `provider_cooldown_remaining()` を実装。`GLOBAL_COOLDOWN` static 変数・`set_global_cooldown_from_error()`・`global_cooldown_remaining()` およびこれらを呼び出す全7箇所を削除（`crates/rustyclaw-providers/src/lib.rs` 他）。

- `[x]` **4. PROVIDER COOLDOWNS パネルの残り時間表示フォーマット改善**
  - 従来の `XXX.Xs` 形式から、人が読みやすい段階的フォーマットに変更。
    - `XdXXh` / `XhXXm` / `XXmXXs` / `XXs`
  - `.prov-secs` 幅を 44px → 52px に拡張（最長 `XXmXXs` = 6文字対応）。
  - 対象: `crates/rustyclaw-gateway/src/health.rs`（CSS `.prov-secs` + JS `secsLabel` 生成ロジック）

- `[x]` **3. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

### 継続モニタリング 🟢 優先度低

- `[-]` **RPi4 本番稼働 — cron.json 定期ジョブの発火確認**
  - Daily Briefing・Topic Patrol・Vital Check が実際に Discord へ正常通知されることを確認
  - Karakeep / Obsidian ネイティブツールが RPi4 上で正常動作することを確認

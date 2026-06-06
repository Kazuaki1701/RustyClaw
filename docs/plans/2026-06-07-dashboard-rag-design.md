# Dashboard チャット RAG 活用設計書

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ダッシュボードチャットが Heartbeat 実行結果・KaraKeep 履歴・Topic Patrol 収集内容・過去の Dashboard 会話を RAG 経由で参照できるようにし、「実運用の報告に対して会話する」ユースケースを支援する。

**Approach:** アプローチ C（ハイブリッド）— cron セッションサマリーの RAG 化 ＋ heartbeat-digest.md の Dashboard 専用動的注入 ＋ Dashboard セッション ID 日付ローテーション化 ＋ Dashboard 専用 top_k 設定

**Architecture:** 既存の session summary / RAG 機構を最大限流用し、新規インフラなしで実現する。

**関連 ADR:** `docs/adr/001-dashboard-rag-approach-c-hybrid.md`  
**実装計画書:** `docs/plans/2026-06-07-dashboard-rag-implementation.md`

---

## 1. 全体構成

```
┌─────────────────────────────────────────────────────────┐
│  cron ジョブ完了時                                       │
│  karakeep / patrol / vitals / briefing                  │
│       ↓                                                 │
│  [NEW] generate_session_summary → ingest_session_summary│
│             ↓ RAG (session: チャンク)                   │
├─────────────────────────────────────────────────────────┤
│  Heartbeat 実行時（変更なし）                            │
│       ↓ heartbeat-digest.md を更新し続ける              │
├─────────────────────────────────────────────────────────┤
│  Dashboard チャット受信時                                │
│  build_system_context (SOUL + USER)                     │
│       + [NEW] heartbeat-digest.md を動的注入            │
│       + RAG 検索 top_k=8 (session: karakeep/patrol etc) │
│       + Session Continuation                            │
│         [NEW] http-dashboard-YYYYMMDD 日付ローテーション │
└─────────────────────────────────────────────────────────┘
```

---

## 2. コンポーネント詳細

### 2-1. cron セッションサマリー RAG 化

#### 対象セッション（ホワイトリスト）

| session_id | 内容 | 頻度 |
|---|---|---|
| `cron:karakeep-cleanup` | KaraKeep 整理 | 1日1回 |
| `cron:karakeep-recommendation` | KaraKeep 推薦 | 1日1回 |
| `cron:topic-patrol-explore` | Patrol 収集 | 1日1回 |
| `cron:topic-patrol-deliver` | Patrol 配信 | 1日1回 |
| `cron:vitals-morning` | バイタル（朝） | 1日1回 |
| `cron:vitals-night` | バイタル（夜） | 1日1回 |
| `cron:daily-briefing` | 朝のブリーフィング | 1日1回 |

`cron:heartbeat` は除外（30分毎の高頻度で RAG を溢れさせるため。Heartbeat は digest 注入で対応）

#### 実装箇所

`crates/rustyclaw-gateway/src/lib.rs` の cron ジョブ正常完了ブロック（`Ok(response)` の後）に、ホワイトリスト判定とサマリーイベントの publish を追加する。

```rust
const SUMMARIZE_CRON_SESSIONS: &[&str] = &[
    "cron:karakeep-cleanup",
    "cron:karakeep-recommendation",
    "cron:topic-patrol-explore",
    "cron:topic-patrol-deliver",
    "cron:vitals-morning",
    "cron:vitals-night",
    "cron:daily-briefing",
];

// Ok(response) ブロック内に追加
if SUMMARIZE_CRON_SESSIONS.contains(&session_id.as_str()) {
    let summary_session_id =
        format!("cron:session-summary:{}", session_id);
    let _ = bus.publish(SystemEvent::IncomingMessage {
        session_id: summary_session_id,
        user_id: "cron".to_string(),
        channel_id: None,
        content: String::new(),
        priority: Priority::Background,
    });
}
```

既存の `cron:session-summary:` 処理パス（`generate_session_summary` → `ingest_session_summary`）をそのまま流用するため、エンジン側の変更は不要。

---

### 2-2. heartbeat-digest.md の Dashboard 動的注入

#### 実装箇所

`crates/rustyclaw-agent/src/lib.rs` の `execute_with_tools` 内、`build_system_context` 呼び出し直後に追加する。

```rust
if session_id.contains("http-dashboard") {
    let digest_path = workspace_dir
        .join("memory")
        .join("heartbeat-digest.md");
    if let Ok(digest) = fs::read_to_string(&digest_path) {
        if !digest.trim().is_empty() {
            system_context.push_str(
                "\n\n## Latest Heartbeat Digest\n"
            );
            system_context.push_str(&digest);
        }
    }
}
```

#### トークン影響

heartbeat-digest.md は既存の生成ロジックで最大 3,000 chars（≈ 750 tokens）に上限制御済み。追加コストは **≤ 750 tokens 固定**。

---

### 2-3. Dashboard セッション ID 日付ローテーション化

#### 現状の問題

- `http-dashboard` 固定 → `http-dashboard.jsonl` が無制限肥大（実測 550KB）
- Session Continuation が機能しない（日付サフィックスなし）
- memory flush が estimated_tokens 超過でスキップされ続ける

#### 実装箇所

`crates/rustyclaw-gateway/src/health.rs` の `/chat` ハンドラ。

```rust
// 変更前
let session_id = "http-dashboard".to_string();

// 変更後
let today = chrono::Local::now().format("%Y%m%d").to_string();
let session_id = format!("http-dashboard-{}", today);
```

#### 得られる効果

| 効果 | 詳細 |
|---|---|
| Session Continuation が機能 | 昨日の Dashboard 会話が自動復元される |
| ファイルが日次でリセット | 1日分の会話のみに限定、肥大問題が解消 |
| memory flush が正常動作 | estimated_tokens が ctx_limit 以内に収まる |

#### 後方互換

既存の `http-dashboard.jsonl` はそのまま残る（読まれなくなるだけ）。LLM Inspector の `session_id.contains("dashboard")` 判定は `http-dashboard-20260607` でも正しく動作するため変更不要。

---

### 2-4. Dashboard 専用 top_k 設定

#### 実装箇所 1: `crates/rustyclaw-config/src/lib.rs`

```rust
pub struct EmbeddingConfig {
    pub top_k: usize,
    pub dashboard_top_k: Option<usize>,  // 追加
    // ...
}
```

#### 実装箇所 2: `crates/rustyclaw-agent/src/lib.rs`（RAG 検索呼び出し側）

```rust
let top_k = if session_id.contains("http-dashboard") {
    config.embedding
        .as_ref()
        .and_then(|e| e.dashboard_top_k)
        .unwrap_or(config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5))
} else {
    config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5)
};
```

#### 実装箇所 3: `production/config/config.debug.json` / `config.release.json`

```json
"embedding": {
  "top_k": 5,
  "dashboard_top_k": 8
}
```

#### top_k=8 の根拠

| チャンク種別 | 期待ヒット数 |
|---|---|
| doc: AGENTS.md / skills | 1〜2件 |
| session: karakeep / patrol / vitals | 2〜3件 |
| session: 過去の Dashboard 会話 | 1〜2件 |
| memory: MEMORY.md（ISSUE-28 実装後） | 1〜2件 |

---

## 3. データフロー

```
[今日の KaraKeep 推薦は？] ← Dashboard ユーザー入力

system_context:
  SOUL.md + USER.md                    （静的・常時）
  + Latest Heartbeat Digest            （動的・Dashboard 専用）
    └ 最新の heartbeat-digest.md 全文
  + Relevant Specifications & Rules    （RAG doc: チャンク）
  + Relevant Past Sessions             （RAG session: チャンク）
    └ cron:karakeep-recommendation のサマリー ← 本設計で追加
    └ cron:topic-patrol-deliver のサマリー    ← 本設計で追加
    └ 昨日の Dashboard 会話のサマリー         ← Session Continuation で追加

会話履歴: http-dashboard-20260607.jsonl（本日分のみ）
```

---

## 4. エラーハンドリング

すべての追加処理は **fail-open** で実装する。

| 処理 | 失敗時の挙動 |
|---|---|
| cron summary publish 失敗 | WARN ログのみ。cron ジョブ本体の応答配信は継続 |
| heartbeat-digest.md 読み込み失敗 | 注入をスキップ。通常の system_context で継続 |
| Dashboard RAG 検索失敗 | 空文字列を返す。既存の fail-open と同じ |
| Session Continuation 失敗 | None を返す（既存動作と同じ） |

---

## 5. 変更ファイル一覧

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-gateway/src/lib.rs` | cron 完了後の session-summary publish（ホワイトリスト判定） |
| `crates/rustyclaw-agent/src/lib.rs` | heartbeat-digest 注入 / dashboard_top_k 適用 |
| `crates/rustyclaw-gateway/src/health.rs` | session_id 日付ローテーション |
| `crates/rustyclaw-config/src/lib.rs` | `dashboard_top_k: Option<usize>` フィールド追加 |
| `production/config/config.debug.json` | `dashboard_top_k: 8` 追加 |
| `production/config/config.release.json` | `dashboard_top_k: 8` 追加 |

---

## 6. 実施タスク

- [ ] **Task 1**: `EmbeddingConfig` に `dashboard_top_k: Option<usize>` を追加し config をデシリアライズ（`rustyclaw-config`）
- [ ] **Task 2**: `config.debug.json` / `config.release.json` に `dashboard_top_k: 8` を追加
- [ ] **Task 3**: `health.rs` の session_id を `http-dashboard-YYYYMMDD` に変更
- [ ] **Task 4**: `execute_with_tools` に heartbeat-digest.md の Dashboard 専用注入ブロックを追加
- [ ] **Task 5**: `execute_with_tools` の RAG top_k を session_id に応じて切り替え
- [ ] **Task 6**: `lib.rs` に `SUMMARIZE_CRON_SESSIONS` ホワイトリストと完了後の summary publish を追加
- [ ] **Task 7**: `cargo build` + `cargo clippy` で検証
- [ ] **Task 8**: rp1 にデプロイし、Dashboard チャットで KaraKeep / Patrol 結果が参照できることを確認
- [ ] **Task 9**: `docs/specs/06_dashboard_spec.md` を更新（session_id ローテーション・RAG 構成の追記）

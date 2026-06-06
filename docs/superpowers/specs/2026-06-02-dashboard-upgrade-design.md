# Design: Dashboard Upgrade & Historical LLM Inspector

**Date:** 2026-06-02  
**Scope:** RustyClaw Runtime Controller Dashboard の全面刷新と LLM 通信履歴インスペクタの実装

---

## 1. 概要

以下の4コンポーネントを対象とする。

1. **Historical LLM Logger** — LLM 入出力ダンプをローテーション保存
2. **Storage Statistics** — `by_provider` 集計の追加と `provider_id` カラム導入
3. **Gateway Lane Titles** — プレビュー文字数拡張
4. **Dashboard & API** — UI 全面刷新・新 API エンドポイント追加

---

## 2. Component 1: Historical LLM Logger

### 現状
`dump_llm_io()` は `workspace/memory/debug/llm/<category>.json` に上書き保存するため、直近1件しか参照できない。

### 変更内容

**ファイル:** `crates/rustyclaw-providers/src/lib.rs`

#### ダンプ先の変更
```
変更前: workspace/memory/debug/llm/<category>.json
変更後: workspace/memory/debug/llm/<category>/<YYYY-MM-DD>/<HH-MM-SS>.json
```
- JST で日付・時刻を取得し、ディレクトリを作成してから JSON を書き込む
- ファイル名の時刻は `HH-MM-SS` 形式（ファイル名に `:` が使えない環境への配慮）

#### 自動クリーンアップ
- `dump_llm_io()` の呼び出し毎に、`<category>/` 以下の日付ディレクトリを走査
- 5日超のフォルダを削除する
- RPi4 は SSD 搭載のため、毎回の `readdir` (≈0.05ms) は問題なし

---

## 3. Component 2: Storage Statistics

### 現状
- `get_usage_by_trigger()` は `trigger_type` カラムをそのままグループ化
- `get_usage_summary()` は `by_model` のみ返す（`by_provider` なし）
- `usage` テーブルにプロバイダ情報が存在しない

### 変更内容

**ファイル:** `crates/rustyclaw-storage/src/lib.rs`, `crates/rustyclaw-providers/src/lib.rs`

#### `usage` テーブルへの `provider_id` カラム追加
```sql
ALTER TABLE usage ADD COLUMN provider_id TEXT;
```
- 過去レコードは `NULL`（後方互換）
- 新規レコードは挿入時に `resolve_provider_id(&model_config)` の結果を格納

#### `resolve_provider_id()` の拡張
`crates/rustyclaw-providers/src/lib.rs` の `resolve_provider_id()` に OpenRouter 判別を追加：

```
api_base_url に "openrouter.ai" を含む → "openrouter"
```

完全なマッピング：

| api_base_url の含む文字列 | provider_id |
|---|---|
| `groq.com` | `groq` |
| `cloudflare.com` | `cloudflare` |
| `openrouter.ai` | `openrouter` |
| `huggingface.co` | `huggingface` |
| それ以外 | `model_provider` フィールドの値（`local`, `gmn` 等） |

#### `log_usage()` への `provider_id` 追加
- 引数に `provider_id: &str` を追加
- 呼び出し元（providers）で `resolve_provider_id()` を渡す

#### `get_usage_summary()` への `by_provider` 追加
```sql
SELECT provider_id, COUNT(*), COALESCE(SUM(total_tokens), 0)
FROM usage
WHERE provider_id IS NOT NULL
GROUP BY provider_id
ORDER BY SUM(total_tokens) DESC
```
レスポンスに `by_provider` マップを追加：
```json
{
  "by_provider": {
    "cloudflare": { "runs": 120, "tokens": 45000 },
    "groq":       { "runs": 80,  "tokens": 32000 }
  }
}
```

#### `get_usage_by_trigger()` — 変更なし
CASE WHEN による過去データ再分類は行わない。

---

## 4. Component 3: Gateway Lane Titles

**ファイル:** `crates/rustyclaw-gateway/src/lib.rs`

`char_limit` を 40 → 80 に変更（`lib.rs:421`）。  
Lane Queue のプレビュー文字列（User Prompt）が2倍の長さで表示される。

---

## 5. Component 4: Dashboard & API

**ファイル:** `crates/rustyclaw-gateway/src/health.rs`

### 5-1. 新規 API エンドポイント

#### `GET /api/llm/dates?cat=<category>`
- `workspace/memory/debug/llm/<category>/` 以下の日付ディレクトリを列挙
- 降順（新しい順）で返す
```json
["2026-06-02", "2026-06-01", "2026-05-31"]
```

#### `GET /api/llm/times?cat=<category>&date=<date>`
- `workspace/memory/debug/llm/<category>/<date>/` 以下の JSON ファイル名（`HH-MM-SS`）を列挙
- 降順で返す
```json
["14-32-10", "11-05-44", "09-18-02"]
```

### 5-2. 既存 API 変更

#### `GET /api/llm/io`
- クエリパラメータ `date` と `time` を追加（省略可）
- 指定された場合: `<category>/<date>/<time>.json` を返す
- 省略された場合: `<category>/<latest-date>/<latest-time>.json` を返す（フォールバック）
- ダンプが存在しない場合: HTTP 404 を返す

#### `GET /api/concurrency`
旧フィールド（`active`, `queue_depth`, `cooldown_secs`, `global_cooldown`）を廃止し、`capacity` と per-provider クールダウンを返す：
```json
{
  "capacity": 4,
  "providers": {
    "cloudflare":  18.4,
    "groq":         0.0,
    "openrouter":   0.0,
    "gmn":          0.0
  }
}
```
実装: `provider_cooldown_remaining(provider)` を各プロバイダに対して呼び出す。`capacity` は `gmn_cap` をそのまま返す。

---

### 5-3. LANE QUEUE パネルの刷新

#### レイアウト：左右2列分割

```
┌─ LANE QUEUE ──────────────────────────────────────────────┐
│ LANES                │ PENDING & SCHEDULED                │
│ ─────────────────────│──────────────────────────────────  │
│ ● [HEARTBEAT]   84s  │ [WAIT][HEARTBEAT] Heartbeat..  2s │
│ ● [PATROL]      42s  │ [COOL][VITALS]               18s  │
│   [  ────  ]     --  │ [SCHED] heartbeat          in 8m  │
│   [  ────  ]     --  │                                    │
└────────────────────────────────────────────────────────────┘
```

#### LANES 列（左）
- 行数は `capacity`（`/api/concurrency` の `capacity` フィールド）に従い動的生成
- 各行: `● [SERVICE_BADGE] <elapsed_s>s`
- IDLE の行: グレーバッジ `[  ────  ]`、ドット非表示、経過時間 `--`
- 経過時間は秒単位の整数（`84s`）

#### PENDING & SCHEDULED 列（右）
- スクロール可能
- 各行: `[WAIT|COOL|SCHED] [SERVICE_BADGE] <description> <elapsed/eta>`
- サービスバッジは LANES 列と同じ着色

#### サービスバッジ定義（`session_id` プレフィックスから判別）

| session_id プレフィックス | バッジ | 色 |
|---|---|---|
| `cron:heartbeat-` | `[HEARTBEAT]` | `#bf00ff`（紫） |
| `cron:topic-patrol-` | `[PATROL]` | `#ff8c00`（橙） |
| `cron:daily-briefing-` | `[BRIEFING]` | `#4488ff`（青） |
| `cron:vitals-` | `[VITALS]` | `#00ff9f`（緑） |
| `cron:karakeep-` | `[KARAKEEP]` | `#ffe066`（黄） |
| `cron:daily-summary-` | `[SUMMARY]` | `#00e5ff`（水色） |
| `discord-` | `[DISCORD]` | `#7b68ee`（藍） |
| `http-dashboard-` | `[DASHBOARD]` | `#00d4ff`（シアン） |
| `cli-` | `[CLI]` | `#cccccc`（白） |
| その他 | `[UNKNOWN]` | `#888888`（灰） |

---

### 5-4. CONCURRENCY パネルの刷新

旧要素を全て削除：
- `#slotRow`（スロットドット）
- `cActive`, `cDepth`, `cCool`, `cGlobal`（旧テキスト行）

新レイアウト（per-provider クールダウンのみ）：

```
┌─ PROVIDER COOLDOWNS ────────────────────────────────┐
│ cloudflare  ██████░░░░  18.4s                       │
│ groq        ──────────  none                        │
│ openrouter  ──────────  none                        │
│ gmn         ──────────  none                        │
└─────────────────────────────────────────────────────┘
```

- バーの最大値は 60s（それ以上はクランプ）
- クールダウン中は対応する色で着色（各プロバイダで固定色）
- `none` の行はバーをグレーで表示

---

### 5-5. LLM API INSPECTOR の刷新

#### ダブルドロップダウン
カテゴリタブ直下に日付・時刻セレクタを追加：

```
[tools][discord][heartbeat]...  ← 既存タブ
[Date ▾ 2026-06-02] [Time ▾ 14-32-10]  ← 新規追加
```

- カテゴリ切替時に `GET /api/llm/dates` を呼び出して Date ドロップダウンを再 populate
- Date 選択時に `GET /api/llm/times` を呼び出して Time ドロップダウンを再 populate
- Time 選択時（または初期表示）に `GET /api/llm/io?cat=&date=&time=` でペイロードを取得

#### truncation 削除
`updateInspector()` の以下の truncation ロジックを削除：
```javascript
// 削除対象
reqTxt.length > 4000 ? '...(truncated head)\n' + reqTxt.slice(-4000) : reqTxt
```
全文をそのまま表示する。

---

### 5-6. APP LOG のサービスバッジ着色

`updateLog()` のログ行パーサを拡張。ログのソースプレフィックスを検出し、サービスバッジに置き換える：

| ログのプレフィックス文字列 | 置換後バッジ | 色 |
|---|---|---|
| `HeartBeatService` | `[HEARTBEAT]` | `#bf00ff` |
| `rustyclaw_gateway` | `[GATEWAY]` | `#00d4ff` |
| `PatrolService` | `[PATROL]` | `#ff8c00` |
| `BriefingService` | `[BRIEFING]` | `#4488ff` |
| `VitalsService` | `[VITALS]` | `#00ff9f` |
| `DiscordService` | `[DISCORD]` | `#7b68ee` |

---

### 5-7. Stats ページの拡張

#### `stats-bottom` を2列 → 3列に変更
```css
/* 変更前 */
.stats-bottom { grid-template-columns: 1fr 1fr; }
/* 変更後 */
.stats-bottom { grid-template-columns: 1fr 1fr 1fr; }
```

#### BY PROVIDER パネル追加（3列目）
- `by_provider` データを水平バーチャートで表示
- 各プロバイダの runs と tokens を表示
- バーの色はプロバイダ固定色（CONCURRENCY パネルと統一）

---

## 6. Verification Plan

### 自動テスト
- `cargo test` が全グリーンであること
- `get_usage_summary()` が `by_provider` フィールドを含むこと（unit test）
- `resolve_provider_id()` が `openrouter.ai` を正しく判別すること（unit test）

### 手動確認（rp1 デプロイ後）
- LLM API INSPECTOR でカテゴリ切替時に Date/Time ドロップダウンが更新される
- 過去の時刻を選択するとそのペイロードが表示される
- LANE QUEUE にサービスバッジが着色表示される
- PROVIDER COOLDOWNS パネルに per-provider 残クールダウンが表示される
- Stats ページに BY PROVIDER パネルが表示される
- APP LOG に `[HEARTBEAT]` 等の着色バッジが表示される

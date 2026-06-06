# Context Window 圧縮戦略 Phase 1 設計仕様（安定化）

**作成日:** 2026-06-03  
**ステータス:** Draft  
**スコープ:** Phase 1 — 外科的バグ修正・安定化  
**後続フェーズ:** Phase 2（ローリングサマリー本体統合）、Phase 3-4（rig-core 全面移行）

---

## 1. 背景と問題

### 根本設計ミス

現行の `get_history_limit()` は **TPM（トークン/分レートリミット）** を基準にトークン上限を算出しているが、これは「API レートリミット」であり「モデルのコンテキストウィンドウ」ではない。config には各モデルに `context_window: "131k"` 等のフィールドが既に存在するにもかかわらず未使用。

### 発生している障害（ログ確認済み 2026-06-03）

| 障害 | 影響 |
|------|------|
| `compact_if_needed(limit: 0)` が毎ターン発火 | Hard-trim で毎回 10件に強制削減、コンテキスト継続性消失 |
| memory flush が LM Studio で 400 エラー（3回連続） | `http-dashboard` の MEMORY.md が更新されない |
| session-summary が約1分おきに重複実行 | 同じセッションに対して LLM が複数回呼ばれる |

### 根本原因

1. **limit:0 バグ**: `groq-llama-8b`（tpm=6000）→ `history_limit=800`、システムプロンプト overhead > 800 → `effective = 0`
2. **flush 失敗**: LM Studio n_ctx=11,008 に 22,000+ トークンを渡している
3. **重複実行**: session-summary の完了フラグが未実装、60秒ポーリングで複数キューが積まれる

---

## 2. 設計方針

**GeminiClaw 参考**: 件数制限（`maxRecentEntries`）による管理をベースとし、トークン推定による上限計算は廃止する。

**3つの独立した外科的修正を実施。アーキテクチャ変更は Phase 3-4 まで行わない。**

---

## 3. Fix 1: context_window ベースの履歴件数管理

### 変更対象
`crates/rustyclaw-agent/src/lib.rs`

### 新設: `parse_context_window(s: &str) -> usize`

config の `context_window` 文字列をトークン数に変換するヘルパー関数:

| 文字列 | トークン数 |
|--------|-----------|
| `"8k"` | 8,192 |
| `"16k"` | 16,384 |
| `"32k"` | 32,768 |
| `"64k"` | 65,536 |
| `"131k"` | 131,072 |
| `"256k"` | 262,144 |
| `"1M"` | 1,048,576 |
| 未認識・None | 32,768（保守的デフォルト） |

### 変更: `get_history_message_limit()` を context_window ベースに

```
context_window < 16,384   →  10件
context_window < 32,768   →  20件
context_window < 65,536   →  40件
context_window < 262,144  →  60件
context_window >= 262,144 →  80件
未設定                    →  20件
```

適用例:
- `groq-llama-8b`（context_window="131k"）→ 60件
- `cf-gemma-4-26b`（context_window="256k"）→ 80件
- `lms-gemma-4-26b`（context_window="16k"）→ 10件

### 廃止: `get_history_limit()` とトークンベース圧縮

- `get_history_limit()` 関数を削除
- `compact_if_needed_with_overhead()` の呼び出し（3箇所）を削除
- `trim_to_last()` 一本に統一（件数キャップのみ）

### 影響範囲
- `lib.rs` の `execute()`, `execute_with_tools()`, `execute_stream()` の各圧縮処理（3箇所）

---

## 4. Fix 2: memory flush のコンテキストサイズ安全チェック

### 変更対象
`crates/rustyclaw-agent/src/lib.rs` の `flush_memory()`  
`production/config/config.release.json`（agents セクション）

### チェックロジック追加（LLM 呼び出し前）

```
flush プロンプトの推定トークン数 = delta_messages テキスト長(chars) × 1.5 + 2,000（固定マージン）
モデルの context_window = parse_context_window(モデル設定の context_window)

推定トークン数 > context_window × 0.8 の場合:
  → warn!("memory flush: skipping — estimated {estimated} tokens exceeds model context {limit}")
  → return Ok(())  // fail-open（会話は継続、flush はスキップ）
```

### config: `"memory-flush"` purpose の追加

`production/config/config.release.json` の `agents` セクション:
```json
"memory-flush": "cf-gemma-4-26b"
```

- `cf-gemma-4-26b`: context_window="256k"（262,144 tokens）で flush が収まる
- purpose 未設定時は `"default"` フォールバック（後方互換を維持）

---

## 5. Fix 3: session-summary 冪等チェック

### 変更対象
`crates/rustyclaw-gateway/src/lib.rs`（スキップ判定）  
`crates/rustyclaw-agent/src/lib.rs` の `generate_session_summary()`

### サマリーファイルの frontmatter に `turns:` を追加

新規生成・更新時に必ず書き込む:
```markdown
---
session: "discord-C1485590891489005749-20260603"
date: "2026-06-03"
turns: 12
---
```

### agent 側: 冪等チェック（GeminiClaw 方式）

`generate_session_summary()` の先頭に追加:
```
既存サマリーファイルの frontmatter から turns: <N> を読み取る
JSONL の meaningful entries 数（heartbeat 除外）を数える
existing_turns >= current_entries なら → return Ok("already current")
```

### インクリメンタル更新

`existing_turns < current_entries`（セッション再開後の差分）の場合:
- 既存 TL;DR + delta entries（`entries[existing_turns..]`）のみを LLM に渡す
- 全履歴の再処理を避け、LLM コスト削減

### gateway 側: mtime ガード（現行維持 + turns チェック連携）

```
JSONL mtime - now < 5分 → "still active" でスキップ（現行維持）
↓（5分以上経過）
agent 側の冪等チェックが担保するため、gateway は重複防止のためのポーリング制御不要
```

---

## 6. 変更ファイル一覧

| ファイル | 変更内容 |
|---------|---------|
| `crates/rustyclaw-agent/src/lib.rs` | `parse_context_window()` 新設、`get_history_message_limit()` 変更、`get_history_limit()` 削除、`compact_if_needed_with_overhead()` 3箇所削除、`flush_memory()` にコンテキストチェック追加、`generate_session_summary()` に turns 冪等チェック追加 |
| `crates/rustyclaw-gateway/src/lib.rs` | session-summary トリガー: mtime ガード維持 |
| `production/config/config.release.json` | `agents."memory-flush": "cf-gemma-4-26b"` 追加 |
| `production/config/config.debug.json` | `agents."memory-flush"` に `["groq-llama-8b"]`（LM Studio はコンテキスト不足のためフォールバック先に） |

---

## 7. テスト方針

| テスト | 内容 |
|--------|------|
| `parse_context_window` | 各文字列パターンのトークン数変換 |
| `get_history_message_limit` | context_window 値ごとの件数確認 |
| flush コンテキストチェック | 推定トークン > 閾値でスキップされること |
| session-summary 冪等 | `turns >= entries` でスキップされること |
| session-summary 差分更新 | `turns < entries` で delta のみ LLM に渡されること |

---

## 8. 後続フェーズとの関係

| フェーズ | 内容 |
|---------|------|
| Phase 2 | ローリングサマリー（5会話ごとの非同期更新）を `rustyclaw-agent` に統合 |
| Phase 3 | rig-core 新機能での使用開始 |
| Phase 4 | `rustyclaw-providers` を rig-core に全面置き換え、`ContextPolicy` 構造体の導入 |
